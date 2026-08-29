//! İndirme yöneticisi — tüm segmentleri orkestre eden katman.
//!
//! Sorumlulukları:
//! * Sunucuyu yoklamak, resume metasını okumak ve segment planına karar vermek
//! * Worker'ları başlatmak, ilerlemeyi toplamak, periyodik olarak metayı yazmak
//! * Duraklat / devam et / iptal et
//! * **Adaptif bölme** (karar #5): bir segment bitince, en yavaş segmentin kalan
//!   aralığını ikiye bölüp boşta kalan slotu değerlendirmek
//!
//! Duraklatma bilinçli olarak "bağlantıları kapat + metayı yaz" şeklinde
//! uygulandı; akan stream'i dondurmaya çalışmak yerine. Devam etmek zaten
//! resume yolunu kullanıyor, yani duraklatma ile çökme sonrası devam aynı kodu
//! paylaşıyor — en çok test edilen yol da o oluyor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use super::http::{self, ServerCapabilities};
use super::resume::{Freshness, ResumeMeta};
use super::segmenter::{self, Segment};
use super::speed::{eta_seconds, SpeedMeter};
use super::category;
use super::throttle::{self, BandwidthRule, HostLimiter, RateLimiter};
use super::worker::{self, SegmentContext, WorkerConfig, WorkerEvent};
use super::writer;
use super::{DownloadError, Result};

/// Arayüzün ilerleme yenileme aralığı. Daha sık göndermek CPU'yu boşa yakıyor,
/// daha seyrek göndermek hız göstergesini tembel gösteriyor.
const TICK: Duration = Duration::from_millis(500);

/// Meta dosyasının diske yazılma aralığı. Her tick'te yazmak SSD'yi gereksiz
/// yoruyor; 2 saniye, çökme hâlinde kaybedilecek ilerlemeyi kabul edilebilir
/// tutuyor.
const META_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Queued,
    Probing,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, DownloadStatus::Queued | DownloadStatus::Probing | DownloadStatus::Running)
    }

    /// Devam ettirilebilir mi? Tamamlanan ve iptal edilen indirmeler hariç.
    pub fn is_resumable(&self) -> bool {
        matches!(self, DownloadStatus::Paused | DownloadStatus::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentSnapshot {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub active: bool,
}

/// Arayüze giden tam durum. Her tick'te yeniden üretiliyor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSnapshot {
    pub id: String,
    pub url: String,
    pub file_name: String,
    pub target_path: String,
    pub status: DownloadStatus,
    pub total_size: u64,
    pub downloaded: u64,
    pub speed: f64,
    pub eta_seconds: Option<u64>,
    pub segments: Vec<SegmentSnapshot>,
    pub error: Option<String>,
    /// Ölümcül olmayan uyarı (ör. sunucu doğrulayıcı vermiyor).
    pub warning: Option<String>,
    pub supports_ranges: bool,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

impl DownloadSnapshot {
    pub fn progress(&self) -> f64 {
        if self.total_size == 0 {
            return 0.0;
        }
        (self.downloaded as f64 / self.total_size as f64).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerConfig {
    pub segments: usize,
    pub min_segment_size: u64,
    pub max_retries: u32,
    /// Adaptif segment bölme açık mı (karar #5).
    pub adaptive: bool,
    /// Bir segmentin bölünebilmesi için kalan aralığın en az bu kadar olması
    /// gerekiyor. Küçük parçalar için yeni TCP+TLS el sıkışması zarar.
    pub min_steal_size: u64,
    pub max_connections_per_host: usize,
    /// Aynı anda kaç indirme çalışsın. Fazlası kuyrukta `Queued` bekler.
    ///
    /// `0` = sınırsız. Varsayılan 3: on indirmeyi aynı anda başlatmak toplam
    /// süreyi kısaltmıyor, yalnızca bant genişliğini bölüp hepsinin bitişini
    /// geciktiriyor. Sıraya koymak ilk dosyayı erken teslim ediyor.
    #[serde(default = "varsayilan_es_zamanli")]
    pub max_concurrent_downloads: usize,
    pub global_speed_limit: u64,
    /// İnen dosyayı türüne göre alt klasöre koy (`Video`, `Müzik`, …).
    ///
    /// Varsayılan kapalı: indirmenin nereye düştüğünü sürüm yükseltmesiyle
    /// sessizce değiştirmek, kullanıcıya dosyasını kaybettirmek gibi gelirdi.
    /// `serde(default)` eski `settings.json` dosyaları için — alan yoksa
    /// kapalı sayılıyor.
    #[serde(default)]
    pub categorize: bool,
    pub bandwidth_rules: Vec<BandwidthRule>,
    pub user_agent: String,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        ManagerConfig {
            segments: segmenter::DEFAULT_SEGMENTS,
            min_segment_size: segmenter::MIN_SEGMENT_SIZE,
            max_retries: 5,
            adaptive: true,
            min_steal_size: 2 * 1024 * 1024,
            max_connections_per_host: 8,
            max_concurrent_downloads: varsayilan_es_zamanli(),
            global_speed_limit: 0,
            categorize: false,
            bandwidth_rules: Vec::new(),
            user_agent: http::DEFAULT_USER_AGENT.to_string(),
            connect_timeout_secs: 15,
            read_timeout_secs: 30,
        }
    }
}

/// Eski `settings.json` dosyalarında bu alan yok; serde varsayılanı buradan
/// geliyor ki alan eksikken 0 (= sınırsız) okunmasın.
fn varsayilan_es_zamanli() -> usize {
    3
}

/// İndirmeye özgü seçenekler motorun kökünde tanımlı: resume metası da onları
/// yazdığı için ortak bir yerde durması gerekiyordu. Burada yeniden dışa
/// aktarılıyor ki çağıranların yolu (`manager::DownloadOptions`) değişmesin.
pub use super::DownloadOptions;

/// Worker'ları durdurma sebebi. Aynı iptal mekanizması iki farklı son duruma
/// yol açtığı için ayrıca saklanıyor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    Pause,
    Cancel,
}

/// Kuyrukta sırasını bekleyen bir başlatma isteği.
///
/// `start` ve `resume` süpervizörü doğrudan başlatmıyor; isteği buraya yazıp
/// [`DownloadManager::pump`]'a bırakıyorlar. Böylece "kaç indirme çalışıyor"
/// kararı tek bir yerde veriliyor.
#[derive(Debug, Clone)]
struct PendingStart {
    directory: PathBuf,
    /// Yeni indirme mi (hedef yol yeniden hesaplanacak), yoksa devam mı.
    fresh: bool,
}

#[derive(Debug)]
struct SegmentRuntime {
    index: usize,
    start: u64,
    end: Arc<AtomicU64>,
    downloaded: Arc<AtomicU64>,
    /// Bölme ile worker'ın byte rezervasyonunu birbirine karşı koruyan kilit
    /// (gerekçe: [`super::worker::SegmentContext::split_lock`]).
    split_lock: Arc<Mutex<()>>,
    speed: f64,
    finished: bool,
}

impl SegmentRuntime {
    fn from_segment(s: &Segment) -> Self {
        SegmentRuntime {
            index: s.index,
            start: s.start,
            end: Arc::new(AtomicU64::new(s.end)),
            downloaded: Arc::new(AtomicU64::new(s.downloaded)),
            split_lock: Arc::new(Mutex::new(())),
            speed: 0.0,
            finished: s.is_complete(),
        }
    }

    fn end(&self) -> u64 {
        self.end.load(Ordering::Relaxed)
    }

    fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    fn total(&self) -> u64 {
        self.end().saturating_sub(self.start) + 1
    }

    fn remaining(&self) -> u64 {
        self.total().saturating_sub(self.downloaded())
    }

    fn cursor(&self) -> u64 {
        self.start + self.downloaded()
    }

    fn to_segment(&self) -> Segment {
        Segment {
            index: self.index,
            start: self.start,
            end: self.end(),
            downloaded: self.downloaded(),
        }
    }
}

#[derive(Debug)]
struct EntryState {
    status: DownloadStatus,
    file_name: String,
    target: PathBuf,
    /// `target` gerçek dosya yolunu mu gösteriyor?
    ///
    /// Yeni bir indirme eklendiğinde yalnızca hedef **klasör** biliniyor;
    /// dosya adı sunucu yoklandıktan sonra belli oluyor. Süpervizörü hiç
    /// çalışmadan duraklatılan bir indirme (kuyrukta beklerken) devam ederken
    /// bunu bilmek zorunda — yoksa klasörün kendi yoluna yazmaya çalışır.
    resolved: bool,
    total_size: u64,
    supports_ranges: bool,
    error: Option<String>,
    warning: Option<String>,
    segments: Vec<SegmentRuntime>,
    speed: f64,
    cancel: Option<CancellationToken>,
    stop_reason: Option<StopReason>,
    /// Doluysa: indirme kuyrukta, süpervizörü henüz başlamadı.
    pending: Option<PendingStart>,
    created_at: u64,
    completed_at: Option<u64>,
}

#[derive(Debug)]
struct Entry {
    id: String,
    url: String,
    /// İndirmeye özgü başlıklar ve ad ezmesi. Duraklat/devam et arasında
    /// korunuyor: `Referer` olmadan devam etmek 403 alırdı.
    options: DownloadOptions,
    state: Mutex<EntryState>,
}

impl Entry {
    fn snapshot(&self) -> DownloadSnapshot {
        let state = self.state.lock().unwrap();

        let segments: Vec<SegmentSnapshot> = state
            .segments
            .iter()
            .map(|s| SegmentSnapshot {
                index: s.index,
                start: s.start,
                end: s.end(),
                downloaded: s.downloaded(),
                total: s.total(),
                speed: s.speed,
                active: !s.finished,
            })
            .collect();

        let downloaded: u64 = segments.iter().map(|s| s.downloaded).sum();
        let kalan = state.total_size.saturating_sub(downloaded);

        DownloadSnapshot {
            id: self.id.clone(),
            url: self.url.clone(),
            file_name: state.file_name.clone(),
            target_path: state.target.to_string_lossy().into_owned(),
            status: state.status,
            total_size: state.total_size,
            downloaded,
            speed: state.speed,
            eta_seconds: if state.status == DownloadStatus::Running {
                eta_seconds(kalan, state.speed)
            } else {
                None
            },
            segments,
            error: state.error.clone(),
            warning: state.warning.clone(),
            supports_ranges: state.supports_ranges,
            created_at: state.created_at,
            completed_at: state.completed_at,
        }
    }
}

struct Inner {
    client: RwLock<Client>,
    entries: RwLock<HashMap<String, Arc<Entry>>>,
    order: RwLock<Vec<String>>,
    rate: Arc<RateLimiter>,
    hosts: RwLock<Arc<HostLimiter>>,
    config: RwLock<ManagerConfig>,
    events: broadcast::Sender<DownloadSnapshot>,
    /// Görevlerin başlatılacağı çalışma zamanı.
    ///
    /// `tokio::spawn` **kullanılamaz**: o, çağıran thread'in ortam bağlamına
    /// bakıyor ve bağlam yoksa panikliyor. Yönetici ise Tauri'nin senkron
    /// komut işleyicilerinden, tek-örnek eklentisinin geri çağırmasından ve
    /// kurulum kancasından da çağrılıyor — bunların hiçbiri çalışma zamanı
    /// bağlamında değil. Handle kurulumda bir kez alınıp saklanıyor; böylece
    /// motor kimin hangi thread'den çağırdığından bağımsız çalışıyor.
    runtime: tokio::runtime::Handle,
    /// Kuyruğu boşaltan kod aynı anda tek yerden çalışsın diye.
    ///
    /// Olmasaydı iki indirme aynı anda bitince iki `pump` aynı boş slotu görüp
    /// eşzamanlılık sınırının üstüne çıkabilirdi.
    pump: Mutex<()>,
}

/// İndirme yöneticisi. Klonlanabilir (içi `Arc`), Tauri state olarak tutuluyor.
#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<Inner>,
}

impl DownloadManager {
    /// Yöneticiyi kurar.
    ///
    /// **Bir Tokio çalışma zamanı içinden çağrılmalı** — handle burada
    /// alınıyor (bkz. [`Inner::runtime`]). Bağlam yoksa panik yerine hata
    /// dönüyor: kurulumda anlaşılır bir mesaj, indirmeye basıldığında
    /// açıklamasız bir çökmeden iyi.
    pub fn new(config: ManagerConfig) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            DownloadError::Other(
                "indirme motoru bir Tokio çalışma zamanı içinden kurulmalı".into(),
            )
        })?;

        let client = http::build_client(
            &config.user_agent,
            Duration::from_secs(config.connect_timeout_secs),
        )?;
        let rate = RateLimiter::new(config.global_speed_limit);
        let hosts = HostLimiter::new(config.max_connections_per_host);
        let (events, _) = broadcast::channel(256);

        Ok(DownloadManager {
            inner: Arc::new(Inner {
                client: RwLock::new(client),
                entries: RwLock::new(HashMap::new()),
                order: RwLock::new(Vec::new()),
                rate,
                hosts: RwLock::new(hosts),
                config: RwLock::new(config),
                events,
                pump: Mutex::new(()),
                runtime,
            }),
        })
    }

    /// İlerleme olaylarına abone olur. Tauri katmanı bunu dinleyip frontend'e
    /// aktarıyor; motor Tauri'yi hiç tanımıyor.
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadSnapshot> {
        self.inner.events.subscribe()
    }

    pub fn config(&self) -> ManagerConfig {
        self.inner.config.read().unwrap().clone()
    }

    /// Ayarları günceller. Hız sınırı ve host kotası anında geçerli olur;
    /// segment sayısı yalnızca yeni indirmelere uygulanır.
    pub fn update_config(&self, config: ManagerConfig) -> Result<()> {
        let yeni_client = http::build_client(
            &config.user_agent,
            Duration::from_secs(config.connect_timeout_secs),
        )?;

        let host_degisti = {
            let mevcut = self.inner.config.read().unwrap();
            mevcut.max_connections_per_host != config.max_connections_per_host
        };
        if host_degisti {
            *self.inner.hosts.write().unwrap() = HostLimiter::new(config.max_connections_per_host);
        }

        *self.inner.client.write().unwrap() = yeni_client;
        *self.inner.config.write().unwrap() = config;
        self.apply_bandwidth_schedule();
        // Sınır yükseltilmiş olabilir; kuyrukta bekleyenler hemen başlasın.
        self.pump();
        Ok(())
    }

    /// Zaman bazlı kuralları o anki saate göre uygular. Hem ayar değişiminde
    /// hem de dakikalık zamanlayıcıdan çağrılıyor.
    pub fn apply_bandwidth_schedule(&self) {
        let config = self.inner.config.read().unwrap();
        let limit = super::throttle::resolve_limit(
            &config.bandwidth_rules,
            config.global_speed_limit,
            super::throttle::current_minute_of_day(),
        );
        self.inner.rate.set_rate(limit);
    }

    pub fn effective_speed_limit(&self) -> u64 {
        self.inner.rate.rate()
    }

    pub fn list(&self) -> Vec<DownloadSnapshot> {
        self.ordered_entries().iter().map(|e| e.snapshot()).collect()
    }

    /// Kayıtları listeye eklenme sırasıyla verir.
    fn ordered_entries(&self) -> Vec<Arc<Entry>> {
        let entries = self.inner.entries.read().unwrap();
        self.inner
            .order
            .read()
            .unwrap()
            .iter()
            .filter_map(|id| entries.get(id).cloned())
            .collect()
    }

    /// Kuyruğu boşaltır: boş slot varsa sıradaki bekleyen indirmeyi başlatır.
    ///
    /// Bir slotun "dolu" sayılması için indirmenin aktif olması **ve** artık
    /// beklemiyor olması gerekiyor. Bir kayıt `pending`i alındıktan sonra
    /// süpervizörü durumu `Probing` yapana kadar hâlâ `Queued` görünüyor;
    /// bu aralıkta da slotu tutmalı, yoksa sınırın üstüne çıkardık.
    ///
    /// Kuyruğa girmiş bir indirme bu arada duraklatılmış olabilir: `stop`
    /// `pending`i temizliyor, dolayısıyla burada bir daha görünmüyor.
    fn pump(&self) {
        let _guard = self.inner.pump.lock().unwrap();
        let limit = self.inner.config.read().unwrap().max_concurrent_downloads;

        loop {
            let kayitlar = self.ordered_entries();

            if limit != 0 {
                let calisan = kayitlar
                    .iter()
                    .filter(|e| {
                        let state = e.state.lock().unwrap();
                        state.status.is_active() && state.pending.is_none()
                    })
                    .count();
                if calisan >= limit {
                    break;
                }
            }

            // Sıradaki bekleyeni al. `take` kilit altında: aynı isteği iki kez
            // başlatmak iki süpervizörün aynı dosyaya yazması demek olurdu.
            let siradaki = kayitlar.iter().find_map(|e| {
                let mut state = e.state.lock().unwrap();
                state.pending.take().map(|p| (e.clone(), p))
            });

            let Some((entry, bekleyen)) = siradaki else {
                break; // Kuyruk boş.
            };

            self.spawn_supervisor(entry, bekleyen.directory, bekleyen.fresh);
        }
    }

    pub fn get(&self, id: &str) -> Option<DownloadSnapshot> {
        self.inner.entries.read().unwrap().get(id).map(|e| e.snapshot())
    }

    /// Yeni indirme başlatır ve indirmenin kimliğini döner.
    pub fn start(&self, url: String, directory: PathBuf) -> Result<String> {
        self.start_with(url, directory, DownloadOptions::default())
    }

    /// [`start`](Self::start)'ın seçenekli hâli — uzantı köprüsü bunu kullanıyor.
    pub fn start_with(
        &self,
        url: String,
        directory: PathBuf,
        options: DownloadOptions,
    ) -> Result<String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(DownloadError::InvalidUrl(url));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let baslangic_adi = options
            .file_name
            .as_deref()
            .map(http::sanitize_file_name)
            .or_else(|| http::file_name_from_url(&url).map(|n| http::sanitize_file_name(&n)))
            .unwrap_or_else(|| http::FALLBACK_FILE_NAME.to_string());

        let entry = Arc::new(Entry {
            id: id.clone(),
            url: url.clone(),
            options: options.clone(),
            state: Mutex::new(EntryState {
                status: DownloadStatus::Queued,
                file_name: baslangic_adi,
                // Henüz yalnızca klasör belli; dosya adını süpervizör çözecek.
                target: directory.clone(),
                resolved: false,
                total_size: 0,
                supports_ranges: false,
                error: None,
                warning: None,
                segments: Vec::new(),
                speed: 0.0,
                cancel: None,
                stop_reason: None,
                pending: Some(PendingStart { directory, fresh: true }),
                created_at: unix_now(),
                completed_at: None,
            }),
        });

        self.inner.entries.write().unwrap().insert(id.clone(), entry.clone());
        self.inner.order.write().unwrap().push(id.clone());
        self.emit(&entry);

        // Doğrudan başlatmak yerine kuyruğa bırak: eşzamanlılık sınırı tek
        // yerden uygulansın.
        self.pump();
        Ok(id)
    }

    /// Duraklatılmış / başarısız bir indirmeyi kaldığı yerden sürdürür.
    pub fn resume(&self, id: &str) -> Result<()> {
        let entry = self.entry(id)?;

        {
            let mut state = entry.state.lock().unwrap();
            if state.status.is_active() {
                return Ok(()); // Zaten çalışıyor ya da kuyrukta.
            }
            if state.status == DownloadStatus::Completed {
                return Err(DownloadError::Other("indirme zaten tamamlandı".into()));
            }
            state.status = DownloadStatus::Queued;
            state.error = None;
            state.stop_reason = None;

            // Hedef yol çözülmüşse süpervizörün yeniden ad seçmesi gerekmiyor;
            // yalnızca üst klasörü istiyor. Çözülmemişse (kuyrukta beklerken
            // duraklatılmış yeni bir indirme) `target` hâlâ klasörün kendisi ve
            // indirme baştan kurulmalı.
            let bekleyen = if state.resolved {
                let dizin = state
                    .target
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| state.target.clone());
                PendingStart { directory: dizin, fresh: false }
            } else {
                PendingStart { directory: state.target.clone(), fresh: true }
            };
            state.pending = Some(bekleyen);
        }

        self.emit(&entry);
        self.pump();
        Ok(())
    }

    pub fn pause(&self, id: &str) -> Result<()> {
        self.stop(id, StopReason::Pause)
    }

    /// Çalışan ve kuyrukta bekleyen tüm indirmeleri duraklatır; kaç tanesinin
    /// etkilendiğini döner.
    ///
    /// Motorda tek geçişte yapılıyor: arayüzün tek tek `pause` çağırması hem
    /// N tur demek olurdu hem de araya biten bir indirme girdiğinde kuyruktan
    /// yeni bir tanesi başlayıp duraklatılmadan kalabilirdi.
    pub fn pause_all(&self) -> usize {
        let hedefler: Vec<String> = self
            .ordered_entries()
            .iter()
            .filter(|e| e.state.lock().unwrap().status.is_active())
            .map(|e| e.id.clone())
            .collect();

        hedefler.iter().filter(|id| self.pause(id).is_ok()).count()
    }

    /// Duraklatılmış ve başarısız tüm indirmeleri kuyruğa alır; kaç tanesinin
    /// etkilendiğini döner. Eşzamanlılık sınırı yine geçerli — hepsi birden
    /// çalışmıyor, sırayla başlıyorlar.
    pub fn resume_all(&self) -> usize {
        let hedefler: Vec<String> = self
            .ordered_entries()
            .iter()
            .filter(|e| e.state.lock().unwrap().status.is_resumable())
            .map(|e| e.id.clone())
            .collect();

        hedefler.iter().filter(|id| self.resume(id).is_ok()).count()
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        self.stop(id, StopReason::Cancel)
    }

    fn stop(&self, id: &str, reason: StopReason) -> Result<()> {
        let entry = self.entry(id)?;
        let token = {
            let mut state = entry.state.lock().unwrap();
            state.stop_reason = Some(reason);
            // Kuyrukta bekliyorsa isteği düşür: sırası gelince başlaması,
            // kullanıcının az önce verdiği durdur komutunu yok saymak olurdu.
            state.pending = None;
            // Henüz worker başlamadıysa (Queued/Probing) durumu doğrudan yaz;
            // süpervizör başladığında iptal edilmiş token'ı görüp çıkacak.
            if !state.status.is_active() {
                state.status = match reason {
                    StopReason::Pause => DownloadStatus::Paused,
                    StopReason::Cancel => DownloadStatus::Cancelled,
                };
            }
            state.cancel.clone()
        };

        if let Some(token) = token {
            token.cancel();
        } else {
            // Worker yok: durumu hemen sonlandır.
            let mut state = entry.state.lock().unwrap();
            state.status = match reason {
                StopReason::Pause => DownloadStatus::Paused,
                StopReason::Cancel => DownloadStatus::Cancelled,
            };
        }

        self.emit(&entry);
        Ok(())
    }

    /// Listeden kaldırır. `delete_files` ise yarım dosya ve meta da silinir.
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        let entry = self.entry(id)?;
        let _ = self.stop(id, StopReason::Cancel);

        let target = entry.state.lock().unwrap().target.clone();

        self.inner.entries.write().unwrap().remove(id);
        self.inner.order.write().unwrap().retain(|x| x != id);

        if delete_files {
            let _ = tokio::fs::remove_file(writer::part_path(&target)).await;
            ResumeMeta::cleanup(&target).await;
        }
        Ok(())
    }

    /// Diskte kalan yarım indirmeleri listeye geri yükler ve kaç tane
    /// bulunduğunu döner.
    ///
    /// Uygulama kapanınca liste bellekte kalmıyor. `.muiget` meta dosyaları
    /// ise indirilen dosyanın yanında duruyor; açılışta indirme klasörünü
    /// tarayıp onları okumak, listeyi oturumlar arası taşımaya yetiyor.
    ///
    /// Geri yüklenen kayıtlar **duraklatılmış** başlıyor, kendiliğinden
    /// devam etmiyorlar: kullanıcı uygulamayı açar açmaz bant genişliğinin
    /// dolmasını beklemiyor olabilir. Otomatik sürdürme ayrı bir ayar
    /// (`resumeOnStart`) ve çağıranın işi.
    ///
    /// Aynı klasör iki kez taranırsa yinelenen kayıt oluşmuyor: hem indirme
    /// kimliği hem hedef yol karşılaştırılıyor.
    pub async fn restore(&self, directory: &Path) -> usize {
        let bulunan = super::resume::scan_directory(directory).await;
        let mut sayi = 0;

        for (target, meta) in bulunan {
            if self.zaten_listede(&meta.id, &target) {
                continue;
            }

            let entry = Arc::new(Entry {
                id: meta.id.clone(),
                url: meta.url.clone(),
                options: meta.options.clone(),
                state: Mutex::new(EntryState {
                    status: DownloadStatus::Paused,
                    file_name: meta.file_name.clone(),
                    target: target.clone(),
                    resolved: true,
                    total_size: meta.total_size,
                    supports_ranges: meta.supports_ranges,
                    error: None,
                    warning: None,
                    segments: meta.segments.iter().map(SegmentRuntime::from_segment).collect(),
                    speed: 0.0,
                    cancel: None,
                    stop_reason: None,
                    pending: None,
                    created_at: meta.created_at,
                    completed_at: None,
                }),
            });

            self.inner.entries.write().unwrap().insert(meta.id.clone(), entry.clone());
            self.inner.order.write().unwrap().push(meta.id.clone());
            self.emit(&entry);
            sayi += 1;
        }

        sayi
    }

    /// Bu kimlik ya da bu hedef dosya zaten listede mi?
    ///
    /// Hedefe de bakılıyor: aynı dosyanın metası elle kopyalanmış olabilir ya
    /// da kullanıcı aynı klasörü ikinci kez taratmış olabilir. İki kayıt aynı
    /// dosyaya yazarsa dosya bozulur.
    fn zaten_listede(&self, id: &str, target: &Path) -> bool {
        let entries = self.inner.entries.read().unwrap();
        if entries.contains_key(id) {
            return true;
        }
        entries.values().any(|e| e.state.lock().unwrap().target == target)
    }

    /// Bu hedef dosyaya yazan, bitmemiş **başka** bir kayıt var mı?
    ///
    /// Dönen değer o kaydın dosya adı — hata mesajında kullanıcıya hangi
    /// satırı kastettiğimizi söyleyebilmek için.
    fn baska_kayit_ayni_hedefte(&self, id: &str, target: &Path) -> Option<String> {
        let entries = self.inner.entries.read().unwrap();
        entries
            .values()
            .find(|e| {
                if e.id == id {
                    return false;
                }
                let state = e.state.lock().unwrap();
                state.resolved
                    && state.target == target
                    && !matches!(
                        state.status,
                        DownloadStatus::Completed | DownloadStatus::Cancelled
                    )
            })
            .map(|e| e.state.lock().unwrap().file_name.clone())
    }

    fn entry(&self, id: &str) -> Result<Arc<Entry>> {
        self.inner
            .entries
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| DownloadError::NotFound(id.to_string()))
    }

    fn emit(&self, entry: &Entry) {
        // Abone yoksa hata döner; bu normal (headless test, UI kapalı).
        let _ = self.inner.events.send(entry.snapshot());
    }

    /// Süpervizörü başlatır. Yalnızca [`pump`](Self::pump) çağırıyor —
    /// eşzamanlılık sınırını atlamamak için başka yerden çağrılmamalı.
    fn spawn_supervisor(&self, entry: Arc<Entry>, directory: PathBuf, fresh: bool) {
        let manager = self.clone();
        self.inner.runtime.spawn(async move {
            if let Err(e) = manager.supervise(entry.clone(), directory, fresh).await {
                let mut state = entry.state.lock().unwrap();
                // İptal/duraklama zaten doğru durumu yazdı; üzerine hata yazma.
                if state.status.is_active() {
                    state.status = match e {
                        DownloadError::Cancelled => match state.stop_reason {
                            Some(StopReason::Pause) => DownloadStatus::Paused,
                            _ => DownloadStatus::Cancelled,
                        },
                        _ => {
                            state.error = Some(e.to_string());
                            DownloadStatus::Failed
                        }
                    };
                }
                state.speed = 0.0;
                state.cancel = None;
                drop(state);
                manager.emit(&entry);
            }

            // Bu indirme bitti (başarıyla ya da değil): slot boşaldı,
            // kuyrukta bekleyen varsa sırası geldi.
            manager.pump();
        });
    }

    /// Bir indirmenin tüm yaşam döngüsü.
    async fn supervise(&self, entry: Arc<Entry>, directory: PathBuf, fresh: bool) -> Result<()> {
        let cancel = CancellationToken::new();
        {
            let mut state = entry.state.lock().unwrap();
            if state.stop_reason.is_some() && !state.status.is_active() {
                return Ok(()); // Başlamadan durduruldu.
            }
            state.status = DownloadStatus::Probing;
            state.cancel = Some(cancel.clone());
        }
        self.emit(&entry);

        let client = self.inner.client.read().unwrap().clone();
        let mut config = self.config();

        // Host kotasını indirmeler arasında bölüştür.
        //
        // Kota tek başına yetmiyordu: aynı sunucudan üç indirme başlatılınca
        // ilki sekiz iznin hepsini alıyor, diğer ikisi ilk indirme bitene
        // kadar sıfır byte'ta bekliyordu. İzin segment boyunca tutulduğu için
        // sıra hiç dönmüyordu. Çözüm izin dağıtımında değil planda: her
        // indirme yalnızca payı kadar segment açıyor.
        //
        // Kayıt süpervizör yaşadığı sürece duruyor. İndirme bitince ya da
        // duraklayınca düşüyor ve kalanların payı büyüyor — bu büyümeyi
        // adaptif bölme (`try_steal`) kendiliğinden değerlendiriyor.
        let hosts = self.inner.hosts.read().unwrap().clone();
        let host = throttle::host_of(&entry.url);
        let _host_kaydi = hosts.register(&host);
        config.segments = config.segments.min(hosts.fair_share(&host));

        // --- 1. Sunucuyu yokla ---
        let caps = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
            sonuc = http::probe_with(&client, &entry.url, &entry.options.headers) => sonuc?,
        };

        // --- 2. Dosya adına ve hedef yola karar ver ---
        //
        // Uzantıdan bir ad geldiyse **o kazanıyor**: tarayıcı adı
        // `Content-Disposition`, yönlendirme zinciri ve kendi indirme
        // kurallarıyla çözüyor, sunucunun ham adından daha doğru oluyor.
        // (Bu, `DownloadOptions::file_name`in zaten belgelenmiş sözüydü ama
        // yoklama sonucu üzerine yazdığı için tutulmuyordu.)
        let dosya_adi = entry
            .options
            .file_name
            .as_deref()
            .map(http::sanitize_file_name)
            .filter(|ad| !ad.is_empty())
            .unwrap_or_else(|| caps.file_name.clone());

        let target = {
            let mevcut = entry.state.lock().unwrap().target.clone();
            if fresh {
                // Kategori klasörü yalnızca yeni indirmelerde uygulanıyor:
                // devam eden bir indirmenin yolu metada yazılı ve dosyayı
                // oraya taşımak resume'u bozardı.
                let klasor = kategori_klasoru(&directory, &dosya_adi, config.categorize).await;
                benzersiz_yol(&klasor, &dosya_adi, &entry.url).await
            } else {
                mevcut
            }
        };
        // Aynı dosyaya yazan ikinci bir kayıt olmamalı.
        //
        // `benzersiz_yol` aynı URL'nin yarım indirmesini bilerek tanıyor ve
        // aynı yolu döndürüyor — resume'un çalışması buna dayanıyor. Ama o
        // yarım indirme listede duruyorsa (oturumlar arası geri yükleme
        // sayesinde artık genelde duruyor) ve kullanıcı bağlantıyı ikinci kez
        // yapıştırırsa iki süpervizör aynı dosyaya yazmaya başlardı.
        if fresh {
            if let Some(ad) = self.baska_kayit_ayni_hedefte(&entry.id, &target) {
                return Err(DownloadError::Other(format!(
                    "{ad} zaten listede; devam etmek için o satırdaki devam et düğmesini kullan"
                )));
            }
        }

        let part = writer::part_path(&target);

        // --- 3. Resume metasını değerlendir ---
        let (segments, mut meta, warning) =
            self.plan(&entry, &caps, &target, &config, fresh).await?;

        {
            let mut state = entry.state.lock().unwrap();
            state.file_name = dosya_adi.clone();
            state.target = target.clone();
            state.resolved = true;
            state.total_size = caps.content_length.unwrap_or(0);
            state.supports_ranges = caps.supports_ranges;
            state.warning = warning;
            state.segments = segments.iter().map(SegmentRuntime::from_segment).collect();
            state.status = DownloadStatus::Running;
        }
        self.emit(&entry);

        // Boyutu bilinen dosyayı baştan ayır (karar #3).
        if let Some(size) = caps.content_length {
            writer::allocate(&part, size).await?;
        } else {
            writer::allocate(&part, 0).await?;
        }

        // Boş dosya: indirilecek byte yok, doğrudan tamamla.
        if segments.is_empty() {
            self.finalize(&entry, &part, &target).await?;
            return Ok(());
        }

        // --- 4. Worker'ları başlat ---
        let (tx, mut rx) = mpsc::unbounded_channel::<WorkerEvent>();
        let worker_config = WorkerConfig {
            max_retries: config.max_retries,
            read_timeout: Duration::from_secs(config.read_timeout_secs),
            ..WorkerConfig::default()
        };

        let mut aktif = 0usize;
        {
            let state = entry.state.lock().unwrap();
            for segment in &state.segments {
                if segment.finished {
                    continue;
                }
                aktif += 1;
                self.spawn_worker(
                    &client,
                    &caps.final_url,
                    segment,
                    &part,
                    &worker_config,
                    &tx,
                    &cancel,
                    &entry.options.headers,
                );
            }
        }
        // `tx` bilinçli olarak burada canlı tutuluyor: adaptif bölme sırasında
        // yeni worker'lara verilecek gönderici bu. Döngü kanalın kapanmasına
        // değil, aktif worker sayacının sıfırlanmasına bakıyor.

        if aktif == 0 {
            // Meta'ya göre her şey inmiş.
            self.finalize(&entry, &part, &target).await?;
            return Ok(());
        }

        // --- 5. İlerleme döngüsü ---
        let sonuc = self
            .progress_loop(
                &entry, &mut rx, &tx, &client, &caps, &part, &worker_config, &cancel, &config,
                &mut meta, &target, aktif,
            )
            .await;
        drop(tx);

        // Ne olursa olsun metayı diske yaz: duraklatma ve çökme sonrası devam
        // etmenin tek dayanağı bu.
        self.persist(&entry, &mut meta, &target).await;

        sonuc?;
        self.finalize(&entry, &part, &target).await
    }

    /// Segment planını çıkarır: resume metası varsa ve tazeyse onu, yoksa yeni plan.
    async fn plan(
        &self,
        entry: &Arc<Entry>,
        caps: &ServerCapabilities,
        target: &Path,
        config: &ManagerConfig,
        fresh: bool,
    ) -> Result<(Vec<Segment>, ResumeMeta, Option<String>)> {
        let mevcut = ResumeMeta::load(target).await.unwrap_or(None);

        if let Some(mut meta) = mevcut {
            let tazelik = meta.freshness(caps);
            if tazelik.can_resume() {
                // Bu indirme başlıklarla geldiyse metadaki kopyayı tazele.
                // Boş seçeneklerle ezmiyoruz: elle yeniden eklenen bir URL,
                // metada duran tarayıcı başlıklarını silmemeli.
                if !entry.options.is_empty() {
                    meta.options = entry.options.clone();
                }
                meta.supports_ranges = caps.supports_ranges;
                let uyari = match tazelik {
                    Freshness::Unverifiable => Some(
                        "Sunucu ETag/Last-Modified vermiyor; dosya değişmişse devam etmek bozuk \
                         sonuç verebilir."
                            .to_string(),
                    ),
                    _ => None,
                };
                return Ok((meta.segments.clone(), meta, uyari));
            }

            // Bayat meta: baştan başla. Yarım dosyayı da sil, yoksa eski
            // içerikle yeni içerik karışır.
            let _ = tokio::fs::remove_file(writer::part_path(target)).await;
            ResumeMeta::cleanup(target).await;
        }

        let toplam = caps.content_length.unwrap_or(0);
        let segments = if caps.can_segment() {
            segmenter::plan_segments(toplam, config.segments, config.min_segment_size)
        } else {
            // Range yok ya da boyut bilinmiyor: tek bağlantı, tek segment.
            segmenter::single_segment(toplam)
        };

        let uyari = if !fresh && !caps.can_segment() {
            Some("Sunucu Range desteklemiyor; tek bağlantıyla ve baştan iniyor.".to_string())
        } else if !caps.can_segment() {
            Some("Sunucu çoklu bağlantı desteklemiyor; tek bağlantıyla iniyor.".to_string())
        } else {
            None
        };

        let meta = ResumeMeta::new(entry.id.clone(), entry.url.clone(), caps, segments.clone())
            .with_options(entry.options.clone());
        Ok((segments, meta, uyari))
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_worker(
        &self,
        client: &Client,
        url: &str,
        segment: &SegmentRuntime,
        part: &Path,
        config: &WorkerConfig,
        tx: &mpsc::UnboundedSender<WorkerEvent>,
        cancel: &CancellationToken,
        headers: &[(String, String)],
    ) {
        let ctx = SegmentContext {
            index: segment.index,
            url: url.to_string(),
            start: segment.start,
            end: segment.end.clone(),
            downloaded: segment.downloaded.clone(),
            split_lock: segment.split_lock.clone(),
            headers: headers.to_vec(),
        };

        let client = client.clone();
        let part = part.to_path_buf();
        let config = config.clone();
        let tx = tx.clone();
        let cancel = cancel.clone();
        let rate = self.inner.rate.clone();
        let hosts = self.inner.hosts.read().unwrap().clone();

        self.inner.runtime.spawn(async move {
            let _ = worker::run_segment(client, ctx, part, config, tx, cancel, rate, hosts).await;
        });
    }

    /// Worker olaylarını toplar, hız ölçer, metayı yazar, adaptif bölme yapar.
    #[allow(clippy::too_many_arguments)]
    async fn progress_loop(
        &self,
        entry: &Arc<Entry>,
        rx: &mut mpsc::UnboundedReceiver<WorkerEvent>,
        tx: &mpsc::UnboundedSender<WorkerEvent>,
        client: &Client,
        caps: &ServerCapabilities,
        part: &Path,
        worker_config: &WorkerConfig,
        cancel: &CancellationToken,
        config: &ManagerConfig,
        meta: &mut ResumeMeta,
        target: &Path,
        mut aktif: usize,
    ) -> Result<()> {
        let simdi = Instant::now();
        let mut toplam_hiz = SpeedMeter::new(simdi);
        let mut segment_hizlari: HashMap<usize, SpeedMeter> = HashMap::new();
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut son_meta = Instant::now();
        let mut hata: Option<String> = None;

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    return Err(DownloadError::Cancelled);
                }

                olay = rx.recv() => {
                    match olay {
                        Some(WorkerEvent::Progress { index, bytes }) => {
                            toplam_hiz.record(bytes);
                            segment_hizlari
                                .entry(index)
                                .or_insert_with(|| SpeedMeter::new(Instant::now()))
                                .record(bytes);
                        }
                        Some(WorkerEvent::Finished { index }) => {
                            aktif = aktif.saturating_sub(1);
                            {
                                let mut state = entry.state.lock().unwrap();
                                if let Some(s) = state.segments.iter_mut().find(|s| s.index == index) {
                                    s.finished = true;
                                    s.speed = 0.0;
                                }
                            }
                            segment_hizlari.remove(&index);

                            // Boşalan slotu değerlendir (karar #5).
                            if config.adaptive && !cancel.is_cancelled() {
                                if let Some(yeni) = self.try_steal(entry, config) {
                                    aktif += 1;
                                    let state = entry.state.lock().unwrap();
                                    if let Some(s) = state.segments.iter().find(|s| s.index == yeni) {
                                        self.spawn_worker(
                                            client, &caps.final_url, s, part, worker_config, tx,
                                            cancel, &entry.options.headers,
                                        );
                                    }
                                }
                            }

                            if aktif == 0 {
                                break;
                            }
                        }
                        Some(WorkerEvent::Failed { index, error }) => {
                            aktif = aktif.saturating_sub(1);
                            hata.get_or_insert(format!("segment {index}: {error}"));
                            if aktif == 0 {
                                break;
                            }
                        }
                        Some(WorkerEvent::Retrying { index, attempt, .. }) => {
                            log::warn!("segment {index}: {attempt}. yeniden deneme");
                            if let Some(m) = segment_hizlari.get_mut(&index) {
                                m.reset(Instant::now());
                            }
                        }
                        None => break, // Tüm worker'lar bitti.
                    }
                }

                _ = tick.tick() => {
                    let simdi = Instant::now();
                    let hiz = toplam_hiz.sample_at(simdi);
                    {
                        let mut state = entry.state.lock().unwrap();
                        state.speed = hiz;
                        for segment in state.segments.iter_mut() {
                            if let Some(m) = segment_hizlari.get_mut(&segment.index) {
                                segment.speed = m.sample_at(simdi);
                            }
                        }
                    }
                    self.emit(entry);

                    if son_meta.elapsed() >= META_INTERVAL {
                        self.persist(entry, meta, target).await;
                        son_meta = Instant::now();
                    }
                }
            }
        }

        // Segmentlerin hepsi gerçekten doldu mu?
        let eksik = {
            let state = entry.state.lock().unwrap();
            state.segments.iter().any(|s| s.remaining() > 0)
        };

        if let Some(mesaj) = hata {
            return Err(DownloadError::Other(mesaj));
        }
        if eksik {
            return Err(DownloadError::Other(
                "indirme eksik tamamlandı; devam etmeyi deneyin".into(),
            ));
        }

        Ok(())
    }

    /// En yavaş segmentin kalan aralığını ikiye böler ve yeni segmenti kaydeder.
    ///
    /// "Yavaş"ın ölçüsü hız değil **tahmini bitiş süresi**: 10 MB kalan ve hızlı
    /// bir segment, 1 MB kalan ve yavaş bir segmentten daha geç biter. Bölmenin
    /// amacı toplam bitiş süresini kısaltmak, o yüzden doğru ölçüt bu.
    fn try_steal(&self, entry: &Arc<Entry>, config: &ManagerConfig) -> Option<usize> {
        let mut state = entry.state.lock().unwrap();

        if state.segments.len() >= segmenter::MAX_SEGMENTS {
            return None;
        }

        // Aynı sunucudan başka indirmeler varsa büyümek onların izinlerini
        // yer. Pay dolduysa bölme yok; pay büyüdüyse (bir indirme bitti)
        // burası kendiliğinden yeniden büyümeye izin veriyor.
        let pay = self.inner.hosts.read().unwrap().fair_share(&throttle::host_of(&entry.url));
        if state.segments.iter().filter(|s| !s.finished).count() >= pay {
            return None;
        }

        let kurban = state
            .segments
            .iter()
            .filter(|s| !s.finished && s.remaining() >= config.min_steal_size * 2)
            .max_by(|a, b| {
                let sure = |s: &SegmentRuntime| s.remaining() as f64 / s.speed.max(1.0);
                sure(a).partial_cmp(&sure(b)).unwrap_or(std::cmp::Ordering::Equal)
            })?
            .index;

        // Bölme, kurbanın rezervasyon kilidi altında yapılıyor: worker aynı anda
        // "sınırı oku + byte ayır" yapamasın diye. Yoksa kurban bölme noktasını
        // geçebilir ve aynı byte'lar iki segmentte birden sayılır.
        let (calinan_start, calinan_end) = {
            let s = state.segments.iter().find(|s| s.index == kurban)?;
            let kilit = s.split_lock.clone();
            let _guard = kilit.lock().unwrap();

            let (yeni_end, calinan_start, calinan_end) =
                segmenter::split_remaining(s.cursor(), s.end(), config.min_steal_size)?;

            // Sınırı kilit altında daralt — worker bir sonraki chunk'ta görüp duracak.
            s.end.store(yeni_end, Ordering::Relaxed);
            (calinan_start, calinan_end)
        };

        let yeni_index = state.segments.iter().map(|s| s.index).max().unwrap_or(0) + 1;
        state.segments.push(SegmentRuntime {
            index: yeni_index,
            start: calinan_start,
            end: Arc::new(AtomicU64::new(calinan_end)),
            downloaded: Arc::new(AtomicU64::new(0)),
            split_lock: Arc::new(Mutex::new(())),
            speed: 0.0,
            finished: false,
        });

        log::info!(
            "segment {kurban} bölündü → yeni segment {yeni_index} ({calinan_start}-{calinan_end})"
        );
        Some(yeni_index)
    }

    async fn persist(&self, entry: &Arc<Entry>, meta: &mut ResumeMeta, target: &Path) {
        let segments: Vec<Segment> = {
            let state = entry.state.lock().unwrap();
            state.segments.iter().map(SegmentRuntime::to_segment).collect()
        };
        meta.segments = segments;
        if let Err(e) = meta.save(target).await {
            log::warn!("resume metası yazılamadı: {e}");
        }
    }

    /// İndirme bitti: `.mgpart` dosyasını nihai adına taşı, metayı sil.
    async fn finalize(&self, entry: &Arc<Entry>, part: &Path, target: &Path) -> Result<()> {
        if part.exists() {
            // Hedefte eski bir dosya varsa üzerine yaz: kullanıcı bu adı zaten
            // `benzersiz_yol` ile onaylamış durumda.
            let _ = tokio::fs::remove_file(target).await;
            tokio::fs::rename(part, target).await?;
        }
        ResumeMeta::cleanup(target).await;

        {
            let mut state = entry.state.lock().unwrap();
            state.status = DownloadStatus::Completed;
            state.speed = 0.0;
            state.cancel = None;
            state.completed_at = Some(unix_now());
            for segment in state.segments.iter_mut() {
                segment.finished = true;
                segment.speed = 0.0;
            }
        }
        self.emit(entry);
        Ok(())
    }
}

/// Kategori açıksa dosya türüne göre alt klasör, değilse verilen klasörün
/// kendisi.
///
/// Klasör burada oluşturuluyor: [`benzersiz_yol`] var olmayan bir klasörde
/// çakışma arayamaz. Oluşturma başarısız olursa köke düşülüyor — kategori bir
/// kolaylık; onun yüzünden indirmenin hiç başlamaması orantısız olurdu.
async fn kategori_klasoru(kok: &Path, dosya_adi: &str, acik: bool) -> PathBuf {
    if !acik {
        return kok.to_path_buf();
    }

    let Some(ad) = category::folder_for(dosya_adi) else {
        return kok.to_path_buf();
    };

    let hedef = kok.join(ad);
    match tokio::fs::create_dir_all(&hedef).await {
        Ok(()) => hedef,
        Err(e) => {
            log::warn!("kategori klasörü oluşturulamadı ({}): {e}", hedef.display());
            kok.to_path_buf()
        }
    }
}

/// Hedef yolu belirler.
///
/// İki farklı durum aynı kontrolden geçiyor:
/// * Aynı URL'nin yarım kalmış indirmesi varsa **o dosyaya devam edilir** —
///   yanına `dosya (1)` açmak, kullanıcının yarım indirmesini çöpe atardı.
/// * Adı çakışan **başka** bir dosya varsa `ad (1).uzanti`, `ad (2).uzanti`...
async fn benzersiz_yol(directory: &Path, file_name: &str, url: &str) -> PathBuf {
    let aday = directory.join(file_name);
    if ayni_indirmenin_devami(&aday, url).await {
        return aday;
    }
    if !aday.exists() && !writer::part_path(&aday).exists() {
        return aday;
    }

    let (govde, uzanti) = match file_name.rsplit_once('.') {
        Some((g, u)) if !g.is_empty() => (g.to_string(), format!(".{u}")),
        _ => (file_name.to_string(), String::new()),
    };

    for sayi in 1..1000 {
        let aday = directory.join(format!("{govde} ({sayi}){uzanti}"));
        if ayni_indirmenin_devami(&aday, url).await {
            return aday;
        }
        if !aday.exists() && !writer::part_path(&aday).exists() {
            return aday;
        }
    }

    directory.join(file_name)
}

/// Bu yolda, **aynı URL'ye ait** bir resume metası var mı?
///
/// URL karşılaştırması şart: aynı adı taşıyan ama başka bir kaynaktan gelen
/// yarım dosyanın üzerine yazmak iki indirmeyi birbirine karıştırırdı.
async fn ayni_indirmenin_devami(aday: &Path, url: &str) -> bool {
    matches!(ResumeMeta::load(aday).await, Ok(Some(meta)) if meta.url == url)
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durum_siniflandirmasi() {
        assert!(DownloadStatus::Running.is_active());
        assert!(DownloadStatus::Queued.is_active());
        assert!(DownloadStatus::Probing.is_active());
        assert!(!DownloadStatus::Paused.is_active());
        assert!(!DownloadStatus::Completed.is_active());

        assert!(DownloadStatus::Paused.is_resumable());
        assert!(DownloadStatus::Failed.is_resumable());
        assert!(!DownloadStatus::Completed.is_resumable());
        assert!(!DownloadStatus::Cancelled.is_resumable());
    }

    #[tokio::test]
    async fn benzersiz_yol_cakismayi_cozuyor() {
        let dir = tempfile::tempdir().unwrap();

        // Boş dizinde ad olduğu gibi kullanılır.
        let ilk = benzersiz_yol(dir.path(), "film.mkv", "https://ornek.com/film.mkv").await;
        assert_eq!(ilk.file_name().unwrap(), "film.mkv");

        tokio::fs::write(&ilk, b"x").await.unwrap();
        let ikinci = benzersiz_yol(dir.path(), "film.mkv", "https://ornek.com/film.mkv").await;
        assert_eq!(ikinci.file_name().unwrap(), "film (1).mkv");

        tokio::fs::write(&ikinci, b"x").await.unwrap();
        let ucuncu = benzersiz_yol(dir.path(), "film.mkv", "https://ornek.com/film.mkv").await;
        assert_eq!(ucuncu.file_name().unwrap(), "film (2).mkv");
    }

    #[tokio::test]
    async fn yarim_indirme_de_cakisma_sayiliyor() {
        let dir = tempfile::tempdir().unwrap();
        // Sadece .mgpart var: aynı ada ikinci bir indirme başlatmak yarım
        // dosyayı bozardı.
        tokio::fs::write(dir.path().join("film.mkv.mgpart"), b"x").await.unwrap();

        let yol = benzersiz_yol(dir.path(), "film.mkv", "https://ornek.com/film.mkv").await;
        assert_eq!(yol.file_name().unwrap(), "film (1).mkv");
    }

    #[tokio::test]
    async fn uzantisiz_ad_da_numaralaniyor() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("LICENSE"), b"x").await.unwrap();

        let yol = benzersiz_yol(dir.path(), "LICENSE", "https://ornek.com/LICENSE").await;
        assert_eq!(yol.file_name().unwrap(), "LICENSE (1)");
    }

    /// Önceki oturumdan kalmış bir yarım indirme kurar: `.mgpart` dosyası +
    /// yarısı inmiş meta.
    async fn yarim_indirme_yaz(
        dir: &Path,
        ad: &str,
        url: &str,
        secenekler: DownloadOptions,
    ) -> PathBuf {
        let target = dir.join(ad);
        tokio::fs::write(writer::part_path(&target), b"yarim").await.unwrap();

        let caps = ServerCapabilities {
            final_url: url.to_string(),
            supports_ranges: true,
            content_length: Some(1000),
            etag: Some("\"abc\"".into()),
            last_modified: None,
            file_name: ad.to_string(),
            content_type: None,
        };

        let mut segments = segmenter::plan_segments(1000, 2, 1);
        segments[0].downloaded = segments[0].total(); // İlk yarısı inmiş.

        ResumeMeta::new(format!("id-{ad}"), url.to_string(), &caps, segments)
            .with_options(secenekler)
            .save(&target)
            .await
            .unwrap();

        target
    }

    #[tokio::test]
    async fn geri_yukleme_yarim_indirmeyi_duraklatilmis_getiriyor() {
        let dir = tempfile::tempdir().unwrap();
        let hedef =
            yarim_indirme_yaz(dir.path(), "film.mkv", "https://ornek.com/film.mkv", DownloadOptions::default())
                .await;

        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        assert_eq!(manager.restore(dir.path()).await, 1);

        let liste = manager.list();
        assert_eq!(liste.len(), 1);
        let indirme = &liste[0];

        assert_eq!(indirme.status, DownloadStatus::Paused, "geri yüklenen indirme kendiliğinden başlamamalı");
        assert_eq!(indirme.file_name, "film.mkv");
        assert_eq!(indirme.url, "https://ornek.com/film.mkv");
        assert_eq!(PathBuf::from(&indirme.target_path), hedef);
        assert_eq!(indirme.total_size, 1000);
        assert_eq!(indirme.downloaded, 500, "inmiş yarı korunmalı");
        assert_eq!(indirme.segments.len(), 2);
        assert!(indirme.supports_ranges);
        // Sürdürülebilir olmalı, yoksa geri yüklemenin bir anlamı kalmaz.
        assert!(indirme.status.is_resumable());
    }

    #[tokio::test]
    async fn geri_yukleme_ayni_klasoru_iki_kez_tararsa_yinelemiyor() {
        let dir = tempfile::tempdir().unwrap();
        yarim_indirme_yaz(dir.path(), "a.bin", "https://ornek.com/a.bin", DownloadOptions::default()).await;

        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        assert_eq!(manager.restore(dir.path()).await, 1);
        assert_eq!(manager.restore(dir.path()).await, 0, "ikinci tarama aynı dosyayı yeniden eklememeli");
        assert_eq!(manager.list().len(), 1);
    }

    #[tokio::test]
    async fn geri_yukleme_tarayici_basliklarini_koruyor() {
        let dir = tempfile::tempdir().unwrap();
        let secenekler = DownloadOptions {
            headers: vec![("Referer".into(), "https://ornek.com/sayfa".into())],
            file_name: None,
        };
        yarim_indirme_yaz(dir.path(), "b.bin", "https://ornek.com/b.bin", secenekler.clone()).await;

        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        manager.restore(dir.path()).await;

        // Başlıklar `Entry`ye taşınmazsa devam eden indirme `Referer` olmadan
        // gider ve çoğu sitede 403 alır.
        let id = &manager.list()[0].id;
        let entry = manager.entry(id).unwrap();
        assert_eq!(entry.options, secenekler);
    }

    #[tokio::test]
    async fn geri_yuklenen_indirme_listenin_kendi_sirasini_koruyor() {
        let dir = tempfile::tempdir().unwrap();
        // Diskten okuma sırası işletim sistemine bağlı; sıralamayı `createdAt`
        // belirlemeli.
        let ilk = yarim_indirme_yaz(dir.path(), "ilk.bin", "https://ornek.com/1", DownloadOptions::default()).await;
        let son = yarim_indirme_yaz(dir.path(), "son.bin", "https://ornek.com/2", DownloadOptions::default()).await;

        for (yol, zaman) in [(&ilk, 100u64), (&son, 900u64)] {
            let mut meta = ResumeMeta::load(yol).await.unwrap().unwrap();
            meta.created_at = zaman;
            let json = serde_json::to_vec_pretty(&meta).unwrap();
            tokio::fs::write(super::super::resume::meta_path(yol), json).await.unwrap();
        }

        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        manager.restore(dir.path()).await;

        let adlar: Vec<_> = manager.list().into_iter().map(|d| d.file_name).collect();
        assert_eq!(adlar, vec!["ilk.bin", "son.bin"]);
    }

    /// **Regresyon (çökme):** yönetici, kendisini kuran çalışma zamanının
    /// bağlamı DIŞINDAN çağrıldığında da çalışmak zorunda.
    ///
    /// Tauri'nin senkron komutları (`start_download`, `resume_download`,
    /// `resume_all_downloads`), tek-örnek eklentisinin geri çağırması ve
    /// kurulum kancası — hiçbiri Tokio bağlamında değil. Motor `tokio::spawn`
    /// kullanırken bunların hepsi "there is no reactor running" paniğiyle
    /// uygulamayı düşürüyordu: kullanıcı "İndir"e basar basmaz çökme.
    #[test]
    fn calisma_zamani_baglami_disindan_baslatilabiliyor() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let manager = rt.block_on(async { DownloadManager::new(test_config()).unwrap() });

        // Kritik nokta: bu çağrı `block_on` DIŞINDA, yani bağlamsız.
        let dir = tempfile::tempdir().unwrap();
        let sonuc = manager.start("http://127.0.0.1:1/a.bin".into(), dir.path().to_path_buf());
        assert!(sonuc.is_ok(), "bağlam dışından başlatma hata verdi: {sonuc:?}");

        // Sürdürme yolu da aynı paniği veriyordu.
        let id = sonuc.unwrap();
        manager.pause(&id).unwrap();
        assert!(manager.resume(&id).is_ok(), "bağlam dışından sürdürme hata verdi");
    }

    #[test]
    fn calisma_zamani_yoksa_kurulum_panik_yerine_hata_veriyor() {
        // Panik yerine hata: kurulumda anlaşılır bir mesaj, indirmeye
        // basıldığında açıklamasız bir çökmeden iyi.
        // `DownloadManager` `Debug` türetmiyor (içinde `Client` var), o yüzden
        // `unwrap_err` yerine elle eşleştiriliyor.
        match DownloadManager::new(test_config()) {
            Err(hata) => assert!(
                hata.to_string().contains("çalışma zamanı"),
                "anlaşılmayan hata: {hata}"
            ),
            Ok(_) => panic!("çalışma zamanı yokken kurulum başarılı olmamalıydı"),
        }
    }

    /// Testlerde ağa çıkılmasın diye kısa zaman aşımları.
    fn test_config() -> ManagerConfig {
        ManagerConfig { connect_timeout_secs: 3, ..ManagerConfig::default() }
    }

    #[tokio::test]
    async fn gecersiz_url_reddediliyor() {
        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        let hata = manager.start("ftp://ornek.com/a.zip".into(), PathBuf::from(".")).unwrap_err();
        assert!(matches!(hata, DownloadError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn olmayan_indirme_bulunamadi_veriyor() {
        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        assert!(matches!(manager.pause("yok"), Err(DownloadError::NotFound(_))));
        assert!(manager.get("yok").is_none());
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn ayar_guncellemesi_hiz_sinirini_uyguluyor() {
        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();
        assert_eq!(manager.effective_speed_limit(), 0);

        let mut config = manager.config();
        config.global_speed_limit = 2_000_000;
        manager.update_config(config).unwrap();

        assert_eq!(manager.effective_speed_limit(), 2_000_000);
    }

    #[tokio::test]
    async fn zaman_kurali_genel_siniri_eziyor() {
        let manager = DownloadManager::new(ManagerConfig::default()).unwrap();

        let mut config = manager.config();
        config.global_speed_limit = 1_000_000;
        // Tüm günü kapsayan sınırsız kural.
        config.bandwidth_rules = vec![BandwidthRule {
            start_minute: 0,
            end_minute: 1440,
            limit_bytes: 0,
            enabled: true,
        }];
        manager.update_config(config).unwrap();

        assert_eq!(manager.effective_speed_limit(), 0, "zaman kuralı genel sınırı ezmeli");
    }

    #[test]
    fn ilerleme_yuzdesi() {
        let mut snapshot = DownloadSnapshot {
            id: "1".into(),
            url: "https://ornek.com/a".into(),
            file_name: "a".into(),
            target_path: "a".into(),
            status: DownloadStatus::Running,
            total_size: 1000,
            downloaded: 250,
            speed: 0.0,
            eta_seconds: None,
            segments: Vec::new(),
            error: None,
            warning: None,
            supports_ranges: true,
            created_at: 0,
            completed_at: None,
        };

        assert!((snapshot.progress() - 0.25).abs() < f64::EPSILON);

        snapshot.total_size = 0;
        assert_eq!(snapshot.progress(), 0.0, "boyut bilinmiyorsa yüzde 0");

        snapshot.total_size = 100;
        snapshot.downloaded = 500;
        assert_eq!(snapshot.progress(), 1.0, "yüzde 100'ü aşmamalı");
    }
}

