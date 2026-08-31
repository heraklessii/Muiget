//! Tek bir segmenti indiren async task.
//!
//! Bir worker'ın tüm bildiği: "şu URL'nin şu byte aralığını şu dosyaya yaz".
//! Kaç segment olduğunu, diğerlerinin ne yaptığını bilmiyor — orkestrasyon
//! [`super::manager`]'ın işi.
//!
//! İki incelik var:
//!
//! * **Aralık daralabilir.** `end` bir [`AtomicU64`]; yönetici yavaş bir
//!   segmenti bölerken bu değeri küçültüyor ve worker kendi kendine erken
//!   duruyor (karar #5). Bu yüzden her chunk'ta sınır yeniden okunuyor.
//! * **`200 OK` bir başarı değil.** Sunucu `Range` başlığını yok sayıp dosyanın
//!   tamamını göndermeye başladıysa ve biz dosyanın ortasına yazıyorsak, yazmaya
//!   devam etmek dosyayı sessizce bozar. Bu durum ölümcül hata sayılıyor.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::throttle::{host_of, HostLimiter, RateLimiter};
use super::writer::SegmentWriter;
// Hangi hatanın yeniden denemeye değdiği kararı motorun kökünde duruyor:
// akış (HLS/DASH) boru hattı da aynı kuralı kullanıyor (bkz. `media::pipeline`).
use super::{yeniden_denenebilir, DownloadError, Result};

/// Worker'ın yöneticiye gönderdiği olaylar.
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    /// Chunk yazıldı. Yönetici bunu hem ilerleme hem hız ölçümü için kullanıyor.
    Progress { index: usize, bytes: u64 },
    Finished { index: usize },
    Failed { index: usize, error: String },
    /// Hata alındı, yeniden deneniyor. Arayüzde "3. deneme..." göstermek için.
    Retrying { index: usize, attempt: u32, delay_ms: u64 },
}

/// Worker'ın çalışma zamanı durumu. `end` ve `downloaded` atomik çünkü yönetici
/// bunları worker çalışırken okuyor/yazıyor.
#[derive(Debug, Clone)]
pub struct SegmentContext {
    pub index: usize,
    pub url: String,
    /// Segmentin mutlak başlangıcı — indirme boyunca değişmez.
    pub start: u64,
    /// Segmentin mutlak bitişi (dahil). Bölünme sırasında **küçülebilir**.
    pub end: Arc<AtomicU64>,
    /// Bu segmentten inen byte. Yönetici ilerleme anlık görüntüsü için okuyor.
    pub downloaded: Arc<AtomicU64>,
    /// Bölme ile byte rezervasyonunu birbirine karşı koruyan kilit.
    ///
    /// Bu kilit olmadan şöyle bir yarış var: yönetici `cursor`u okuyup bölme
    /// noktasını hesaplarken worker yazmaya devam ediyor ve bölme noktasını
    /// geçebiliyor. O zaman aynı byte'lar hem kurbanda hem çalınan segmentte
    /// sayılıyor, ilerleme %100'ü aşıyor. Kilit "sınırı oku + yazacağın kadarını
    /// ayır" adımını bölünmez yapıyor.
    ///
    /// Kilit **yazma boyunca tutulmuyor**: rezervasyon senkron, disk yazması
    /// ondan sonra ve kilitsiz. Yani yönetici en fazla birkaç nanosaniye bekler.
    pub split_lock: Arc<std::sync::Mutex<()>>,
    /// Her isteğe eklenen ek başlıklar (`Referer`, `Cookie`, ...).
    /// Tarayıcı uzantısından gelen indirmelerde şart.
    pub headers: Vec<(String, String)>,
}

impl SegmentContext {
    pub fn cursor(&self) -> u64 {
        self.start + self.downloaded.load(Ordering::Relaxed)
    }

    pub fn end(&self) -> u64 {
        self.end.load(Ordering::Relaxed)
    }

    pub fn is_complete(&self) -> bool {
        self.cursor() > self.end()
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub max_retries: u32,
    /// İlk yeniden deneme gecikmesi; her denemede ikiye katlanıyor.
    pub retry_base_delay: Duration,
    pub max_retry_delay: Duration,
    /// Tek bir chunk'ın gelmesi için tanınan süre. Toplam indirme süresi
    /// sınırlanmıyor (büyük dosya saatler sürebilir); takılan bağlantıyı
    /// yakalayan sınır bu.
    pub read_timeout: Duration,
    /// Bu kadar byte yazıldıktan sonra diske flush.
    pub flush_threshold: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        WorkerConfig {
            max_retries: 5,
            retry_base_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            flush_threshold: 4 * 1024 * 1024,
        }
    }
}

/// Bir segmenti indirir; hata alırsa üstel geri çekilmeyle yeniden dener.
#[allow(clippy::too_many_arguments)]
pub async fn run_segment(
    client: Client,
    ctx: SegmentContext,
    part_path: PathBuf,
    config: WorkerConfig,
    events: mpsc::UnboundedSender<WorkerEvent>,
    cancel: CancellationToken,
    rate: Arc<RateLimiter>,
    hosts: Arc<HostLimiter>,
) -> Result<()> {
    let mut attempt = 0u32;

    loop {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        if ctx.is_complete() {
            let _ = events.send(WorkerEvent::Finished { index: ctx.index });
            return Ok(());
        }

        match indir(&client, &ctx, &part_path, &config, &events, &cancel, &rate, &hosts).await {
            Ok(()) => {
                let _ = events.send(WorkerEvent::Finished { index: ctx.index });
                return Ok(());
            }
            Err(e) if !yeniden_denenebilir(&e) => {
                let _ = events.send(WorkerEvent::Failed {
                    index: ctx.index,
                    error: e.to_string(),
                });
                return Err(e);
            }
            Err(e) => {
                attempt += 1;
                if attempt > config.max_retries {
                    let mesaj = format!("{} deneme sonrası vazgeçildi: {e}", config.max_retries);
                    let _ = events.send(WorkerEvent::Failed { index: ctx.index, error: mesaj.clone() });
                    return Err(DownloadError::Other(mesaj));
                }

                let gecikme = geri_cekilme(&config, attempt);
                let _ = events.send(WorkerEvent::Retrying {
                    index: ctx.index,
                    attempt,
                    delay_ms: gecikme.as_millis() as u64,
                });

                // Beklerken iptal gelirse hemen çık — kullanıcı "durdur" dedikten
                // sonra 30 saniye daha beklemesi kabul edilemez.
                tokio::select! {
                    _ = tokio::time::sleep(gecikme) => {}
                    _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
                }
            }
        }
    }
}

/// Kaç byte biriktikten sonra ilerleme olayı gönderilsin.
///
/// Eskiden her chunk bir olay gönderiyordu. reqwest'in verdiği chunk tipik
/// olarak 8–64 KB: 100 MB/s'lik bir bağlantıda segment başına saniyede binlerce
/// mesaj, sekiz segmentle on binlerce. Her mesaj sınırsız kanala girip
/// süpervizör görevini uyandırıyordu — hepsi yalnızca **hız göstergesi** için,
/// çünkü ilerlemenin gerçek kaynağı `SegmentContext.downloaded` atomiği.
///
/// 256 KB ve 100 ms: hangisi önce dolarsa. Bayt eşiği hızlı bağlantıda mesaj
/// sayısını sabitliyor, süre eşiği yavaş bağlantıda göstergenin donmasını
/// önlüyor — 20 KB/s'de yalnızca bayt eşiğine bakan bir kod on saniyede bir
/// güncelleme yapardı.
pub(crate) const ILERLEME_ESIGI_BYTE: u64 = 256 * 1024;
pub(crate) const ILERLEME_ESIGI_SURE: Duration = Duration::from_millis(100);

/// İlerleme olaylarını biriktirip seyrek gönderir.
///
/// Hız ölçümü için toplam byte ve zaman önemli, tek tek chunk'lar değil:
/// EWMA'nın yarı-ömrü 3 saniye, yani 100 ms'lik toplama penceresi ölçümü
/// gözle görülür biçimde değiştirmiyor.
struct IlerlemeBiriktirici<'a> {
    events: &'a mpsc::UnboundedSender<WorkerEvent>,
    index: usize,
    birikmis: u64,
    son_gonderim: Instant,
}

impl<'a> IlerlemeBiriktirici<'a> {
    fn yeni(events: &'a mpsc::UnboundedSender<WorkerEvent>, index: usize) -> Self {
        IlerlemeBiriktirici { events, index, birikmis: 0, son_gonderim: Instant::now() }
    }

    fn ekle(&mut self, bytes: u64) {
        self.birikmis += bytes;
        if self.birikmis >= ILERLEME_ESIGI_BYTE
            || self.son_gonderim.elapsed() >= ILERLEME_ESIGI_SURE
        {
            self.bosalt();
        }
    }

    /// Birikeni gönderir. Segment biterken ya da hata dönerken çağrılıyor:
    /// aksi hâlde son birkaç yüz KB hız ölçümüne hiç girmezdi.
    fn bosalt(&mut self) {
        if self.birikmis == 0 {
            return;
        }
        let _ = self.events.send(WorkerEvent::Progress {
            index: self.index,
            bytes: self.birikmis,
        });
        self.birikmis = 0;
        self.son_gonderim = Instant::now();
    }
}

impl Drop for IlerlemeBiriktirici<'_> {
    /// Fonksiyondan hangi yoldan çıkılırsa çıkılsın birikeni gönder.
    ///
    /// `indir` yedi ayrı noktadan `return` ediyor; her birine elle bir çağrı
    /// koymak, ileride eklenecek sekizincinin unutulması demekti.
    fn drop(&mut self) {
        self.bosalt();
    }
}

/// Tek denemenin gövdesi.
#[allow(clippy::too_many_arguments)]
async fn indir(
    client: &Client,
    ctx: &SegmentContext,
    part_path: &Path,
    config: &WorkerConfig,
    events: &mpsc::UnboundedSender<WorkerEvent>,
    cancel: &CancellationToken,
    rate: &Arc<RateLimiter>,
    hosts: &Arc<HostLimiter>,
) -> Result<()> {
    // Host kotası: izin alınana kadar bekle. İzin bu fonksiyondan çıkarken
    // (hata dâhil) otomatik bırakılıyor.
    let _permit = hosts.acquire(&host_of(&ctx.url)).await;

    let cursor = ctx.cursor();
    let end = ctx.end();
    if cursor > end {
        return Ok(());
    }

    let bastan_mi = cursor == 0 && ctx.start == 0;

    let mut istek = client.get(&ctx.url).header(RANGE, format!("bytes={cursor}-{end}"));
    for (ad, deger) in &ctx.headers {
        istek = istek.header(ad, deger);
    }
    let response = istek.send().await?;

    let status = response.status();
    match status {
        StatusCode::PARTIAL_CONTENT => {}
        StatusCode::OK => {
            // Sunucu Range'i yok saydı. Dosyanın başındaysak ve hiç byte
            // yazmadıysak gövde zaten istediğimiz yerden başlıyor — devam
            // edilebilir. Aksi hâlde yazmaya devam etmek dosyayı bozar.
            if !bastan_mi {
                return Err(DownloadError::RangeIgnored { segment: ctx.index });
            }
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // İstenen aralık dosyanın dışında: ya dosya küçüldü ya da meta
            // bayat. Yeniden denemek düzeltmez.
            return Err(DownloadError::HttpStatus { status: status.as_u16() });
        }
        s if s.is_success() => {}
        s => return Err(DownloadError::HttpStatus { status: s.as_u16() }),
    }

    let mut writer = SegmentWriter::open(part_path, cursor).await?;
    let mut stream = response.bytes_stream();
    let mut ilerleme = IlerlemeBiriktirici::yeni(events, ctx.index);

    loop {
        if cancel.is_cancelled() {
            writer.flush().await?;
            return Err(DownloadError::Cancelled);
        }

        let sonraki = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                writer.flush().await?;
                return Err(DownloadError::Cancelled);
            }
            chunk = tokio::time::timeout(config.read_timeout, stream.next()) => chunk,
        };

        let chunk = match sonraki {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(e))) => {
                // Ağ hatası: yazılanları koru, yeniden denemeye bırak.
                writer.flush().await?;
                return Err(DownloadError::Network(e));
            }
            Ok(None) => break, // Gövde bitti.
            Err(_) => {
                writer.flush().await?;
                return Err(DownloadError::Other(format!(
                    "segment {}: {} saniyedir veri gelmiyor",
                    ctx.index,
                    config.read_timeout.as_secs()
                )));
            }
        };

        if chunk.is_empty() {
            continue;
        }

        // --- Rezervasyon (kilit altında) ---
        // Sınır her chunk'ta yeniden okunuyor: yönetici bu segmenti bölmüş
        // olabilir. Sınırı okumak ve yazılacak byte'ları `downloaded`a eklemek
        // tek bir bölünmez adım olmak zorunda; gerekçe `split_lock`ta.
        let (ayrilan, son_chunk) = {
            let _kilit = ctx.split_lock.lock().unwrap();
            let guncel_end = ctx.end.load(Ordering::Relaxed);
            let konum = ctx.start + ctx.downloaded.load(Ordering::Relaxed);

            if konum > guncel_end {
                (0u64, true)
            } else {
                // Chunk daralmış sınırı aşıyorsa fazlası atılıyor — o byte'ları
                // artık çalınan segmentin worker'ı indirecek.
                let izinli = guncel_end - konum + 1;
                let ayrilan = (chunk.len() as u64).min(izinli);
                ctx.downloaded.fetch_add(ayrilan, Ordering::Relaxed);
                (ayrilan, ayrilan < chunk.len() as u64)
            }
        };

        if ayrilan == 0 {
            break;
        }

        // Yazma kilitsiz. Başarısız olursa rezervasyon geri alınıyor: aksi
        // hâlde `downloaded` diskteki gerçekten ileride kalır ve yeniden deneme
        // dosyada delik bırakır.
        if let Err(e) = writer.write_chunk(&chunk[..ayrilan as usize]).await {
            ctx.downloaded.fetch_sub(ayrilan, Ordering::Relaxed);
            return Err(e);
        }
        writer.flush_if_needed(config.flush_threshold).await?;

        ilerleme.ekle(ayrilan);

        // Hız sınırı chunk yazıldıktan SONRA uygulanıyor; bekleme soketten
        // okumayı yavaşlatıyor ve TCP akış kontrolü karşı tarafa yansıtıyor.
        rate.consume(ayrilan).await;

        if son_chunk || ctx.is_complete() {
            break;
        }
    }

    writer.flush().await?;

    // Gövde bitti ama aralık dolmadıysa bağlantı erken kapanmış demektir;
    // hata döndürüp yeniden denemeye bırak.
    if !ctx.is_complete() {
        return Err(DownloadError::Other(format!(
            "segment {}: bağlantı erken kapandı ({}/{} byte)",
            ctx.index,
            ctx.downloaded.load(Ordering::Relaxed),
            ctx.end() - ctx.start + 1
        )));
    }

    Ok(())
}

/// Üstel geri çekilme: 500ms, 1s, 2s, 4s... tavanla sınırlı.
fn geri_cekilme(config: &WorkerConfig, attempt: u32) -> Duration {
    let kat = 2u32.saturating_pow(attempt.saturating_sub(1).min(16));
    config
        .retry_base_delay
        .saturating_mul(kat)
        .min(config.max_retry_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WorkerConfig {
        WorkerConfig::default()
    }

    /// Biriktiriciden çıkan olayların byte'larını toplar.
    fn olaylari_topla(rx: &mut mpsc::UnboundedReceiver<WorkerEvent>) -> (usize, u64) {
        let mut adet = 0;
        let mut toplam = 0;
        while let Ok(WorkerEvent::Progress { bytes, .. }) = rx.try_recv() {
            adet += 1;
            toplam += bytes;
        }
        (adet, toplam)
    }

    #[test]
    fn ilerleme_biriktiricisi_esik_altinda_olay_gondermiyor() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut b = IlerlemeBiriktirici::yeni(&tx, 0);

        // 16 KB'lık on chunk = 160 KB; eşik 256 KB. Süre eşiğine takılmasınlar
        // diye zamanı ileri sarmıyoruz — test milisaniyeler içinde bitiyor.
        for _ in 0..10 {
            b.ekle(16 * 1024);
        }
        assert_eq!(olaylari_topla(&mut rx).0, 0, "eşik altında olay çıkmamalı");

        // Eşiği aşınca tek bir olayda toplu gidiyor.
        b.ekle(128 * 1024);
        let (adet, toplam) = olaylari_topla(&mut rx);
        assert_eq!(adet, 1, "on bir chunk için tek olay bekleniyordu");
        assert_eq!(toplam, 288 * 1024);
    }

    #[test]
    fn biriktirici_dusunce_kalan_byte_kaybolmuyor() {
        // Asıl risk bu: segment eşiğe ulaşmadan biterse (ya da hata dönerse)
        // son kırıntı hız ölçümüne hiç girmezdi.
        let (tx, mut rx) = mpsc::unbounded_channel();
        {
            let mut b = IlerlemeBiriktirici::yeni(&tx, 3);
            b.ekle(1024);
        }
        let (adet, toplam) = olaylari_topla(&mut rx);
        assert_eq!(adet, 1);
        assert_eq!(toplam, 1024);
    }

    #[test]
    fn bos_biriktirici_olay_uretmiyor() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        drop(IlerlemeBiriktirici::yeni(&tx, 0));
        assert_eq!(olaylari_topla(&mut rx).0, 0);
    }

    #[test]
    fn geri_cekilme_ustel_ve_tavanli() {
        let c = WorkerConfig {
            retry_base_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(30),
            ..cfg()
        };

        assert_eq!(geri_cekilme(&c, 1), Duration::from_millis(500));
        assert_eq!(geri_cekilme(&c, 2), Duration::from_secs(1));
        assert_eq!(geri_cekilme(&c, 3), Duration::from_secs(2));
        assert_eq!(geri_cekilme(&c, 4), Duration::from_secs(4));
        // Tavan aşılmamalı.
        assert_eq!(geri_cekilme(&c, 20), Duration::from_secs(30));
    }

    #[test]
    fn kalici_hatalar_yeniden_denenmiyor() {
        assert!(!yeniden_denenebilir(&DownloadError::Cancelled));
        assert!(!yeniden_denenebilir(&DownloadError::Paused));
        assert!(!yeniden_denenebilir(&DownloadError::RangeIgnored { segment: 2 }));
        assert!(!yeniden_denenebilir(&DownloadError::HttpStatus { status: 404 }));
        assert!(!yeniden_denenebilir(&DownloadError::HttpStatus { status: 403 }));
        assert!(!yeniden_denenebilir(&DownloadError::HttpStatus { status: 416 }));
        assert!(!yeniden_denenebilir(&DownloadError::InvalidUrl("x".into())));
    }

    #[test]
    fn gecici_hatalar_yeniden_deneniyor() {
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 500 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 503 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 408 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 429 }));
        assert!(yeniden_denenebilir(&DownloadError::Other("takıldı".into())));
    }

    #[test]
    fn izin_hatasi_yeniden_denenmiyor() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "yasak");
        assert!(!yeniden_denenebilir(&DownloadError::Io(io)));

        let io = std::io::Error::new(std::io::ErrorKind::WouldBlock, "meşgul");
        assert!(yeniden_denenebilir(&DownloadError::Io(io)));
    }

    #[test]
    fn segment_context_ilerlemeyi_dogru_hesapliyor() {
        let ctx = SegmentContext {
            index: 1,
            url: "https://ornek.com/a".into(),
            start: 1000,
            end: Arc::new(AtomicU64::new(1999)),
            downloaded: Arc::new(AtomicU64::new(0)),
            split_lock: Arc::new(std::sync::Mutex::new(())),
            headers: Vec::new(),
        };

        assert_eq!(ctx.cursor(), 1000);
        assert!(!ctx.is_complete());

        ctx.downloaded.store(500, Ordering::Relaxed);
        assert_eq!(ctx.cursor(), 1500);
        assert!(!ctx.is_complete());

        ctx.downloaded.store(1000, Ordering::Relaxed);
        assert_eq!(ctx.cursor(), 2000);
        assert!(ctx.is_complete(), "cursor end'i geçince segment bitmiş sayılır");
    }

    #[test]
    fn aralik_daraltilinca_segment_erken_bitiyor() {
        let ctx = SegmentContext {
            index: 0,
            url: "https://ornek.com/a".into(),
            start: 0,
            end: Arc::new(AtomicU64::new(9999)),
            downloaded: Arc::new(AtomicU64::new(5000)),
            split_lock: Arc::new(std::sync::Mutex::new(())),
            headers: Vec::new(),
        };
        assert!(!ctx.is_complete());

        // Yönetici segmenti böldü: sınır 4999'a çekildi.
        ctx.end.store(4999, Ordering::Relaxed);
        assert!(ctx.is_complete(), "daralan sınırla segment tamamlanmış sayılmalı");
    }
}
