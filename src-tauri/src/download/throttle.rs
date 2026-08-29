//! Bant genişliği sınırlama ve host bağlantı kotası (Faz 2).
//!
//! İki ayrı sınır var, ikisi de meşru istemci tarafı davranış:
//!
//! * **Hız sınırı** — token bucket. Kullanıcı "indirme 2 MB/s'yi geçmesin"
//!   dediğinde diğer uygulamalar için bant genişliği bırakır.
//! * **Host başına bağlantı kotası** — aynı sunucuya aynı anda açılan segment
//!   sayısı. Bu bir *kısıtlama*, bir aşma değil: sunucuya nazik davranmak için.
//!
//! Not (`CLAUDE.md` → Kapsam Dışı): buradaki hiçbir mekanizma sunucunun koyduğu
//! bir limiti aşmaya çalışmaz; tam tersine kendi tarafımızda tavan koyar.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

/// Token bucket ile hız sınırlayıcı.
///
/// Kova saniyede `rate` token doluyor, her inen byte bir token harcıyor. Kova
/// kapasitesi 1 saniyelik dolum: kısa süreli patlamalara izin verip ortalamayı
/// hedefte tutuyor. Sabit pencereli ("her saniye N byte") bir sayaç yerine bu
/// seçildi çünkü pencere sınırında iki kat hız sızdırmıyor.
#[derive(Debug)]
pub struct RateLimiter {
    /// Byte/saniye. `0` = sınırsız (sıcak yolda kilit bile alınmıyor).
    rate: AtomicU64,
    bucket: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(rate_bytes_per_sec: u64) -> Arc<Self> {
        Arc::new(RateLimiter {
            rate: AtomicU64::new(rate_bytes_per_sec),
            bucket: Mutex::new(Bucket { tokens: rate_bytes_per_sec as f64, last_refill: Instant::now() }),
        })
    }

    pub fn unlimited() -> Arc<Self> {
        Self::new(0)
    }

    /// Sınırı çalışma anında değiştirir — ayarlar penceresinden ya da zaman
    /// kuralından gelen değişiklik indirmeleri yeniden başlatmadan uygulanır.
    pub fn set_rate(&self, rate_bytes_per_sec: u64) {
        self.rate.store(rate_bytes_per_sec, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    pub fn is_unlimited(&self) -> bool {
        self.rate() == 0
    }

    /// `bytes` kadar token harcanana dek bekler.
    ///
    /// Chunk zaten ağdan okunduktan **sonra** çağrılıyor: TCP akış kontrolü
    /// bizim beklememizi karşı tarafa doğal olarak yansıtıyor, ayrıca soketten
    /// okumayı durdurmak gerekmiyor.
    pub async fn consume(&self, bytes: u64) {
        if self.is_unlimited() || bytes == 0 {
            return;
        }

        // Kalan borç. Kova bir seferde yetmezse döngü, dolum bekleyerek
        // borcu parça parça kapatıyor.
        let mut kalan = bytes as f64;

        while kalan > 0.0 {
            let bekleme = {
                let rate = self.rate();
                if rate == 0 {
                    return; // Bekleme sırasında sınır kaldırılmış.
                }

                let mut bucket = self.bucket.lock().await;
                let simdi = Instant::now();
                let gecen = simdi.saturating_duration_since(bucket.last_refill).as_secs_f64();
                bucket.last_refill = simdi;

                let kapasite = rate as f64;
                bucket.tokens = (bucket.tokens + gecen * rate as f64).min(kapasite);

                if bucket.tokens >= kalan {
                    bucket.tokens -= kalan;
                    return;
                }

                // Kovadaki her şeyi harca, kalanı bir sonraki tura bırak.
                kalan -= bucket.tokens;
                bucket.tokens = 0.0;

                // Uyku bir saniyeyle sınırlı: chunk kovanın kapasitesinden çok
                // büyük olduğunda tek seferde uyumak yerine döngüye dönmek,
                // aradaki sınır değişikliklerinin (zaman kuralı devreye girmesi)
                // fark edilmesini sağlıyor.
                Duration::from_secs_f64((kalan / rate as f64).min(1.0))
            };

            tokio::time::sleep(bekleme).await;
        }
    }
}

/// Aynı sunucuya açılan eşzamanlı bağlantı sayısını sınırlar.
///
/// Segment sayısı 8 iken üç dosyayı aynı sunucudan indirmek 24 bağlantı demek;
/// çoğu sunucu bunu reddediyor ve indirmeler hata alıyor. Kota bunu önlüyor.
///
/// Kota tek başına yetmiyordu: aynı host'tan üç indirme başlatılınca ilki
/// sekiz iznin hepsini alıyor, diğer ikisi sıfır byte'ta bekliyordu. Bu yüzden
/// limiter host başına **kaç indirme** olduğunu da biliyor ve
/// [`fair_share`](Self::fair_share) ile her indirmeye düşen payı veriyor.
/// İzinler segment boyunca tutulduğu için pay, izin dağıtımında değil
/// **segment planında** uygulanıyor: az segment aç, kimse aç kalmasın.
#[derive(Debug)]
pub struct HostLimiter {
    max_per_host: usize,
    semaphores: Mutex<HashMap<String, Arc<Semaphore>>>,
    /// Host başına aktif **indirme** sayısı (segment değil).
    ///
    /// `std::sync::Mutex`: sayaç senkron bağlamlardan da okunuyor
    /// (`try_steal` async değil) ve kilit altında yalnızca birkaç komut var.
    active: std::sync::Mutex<HashMap<String, usize>>,
}

impl HostLimiter {
    pub fn new(max_per_host: usize) -> Arc<Self> {
        Arc::new(HostLimiter {
            max_per_host: max_per_host.max(1),
            semaphores: Mutex::new(HashMap::new()),
            active: std::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Host için bir bağlantı izni alır. İzin `Drop` olunca sıradaki worker
    /// devralıyor; worker hata alıp çıksa bile kota sızmıyor.
    pub async fn acquire(&self, host: &str) -> OwnedSemaphorePermit {
        let semaphore = {
            let mut map = self.semaphores.lock().await;
            map.entry(host.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.max_per_host)))
                .clone()
        };

        // `acquire_owned` yalnızca semafor kapatılırsa hata verir; biz hiç
        // kapatmıyoruz, o yüzden bu bekleme her zaman başarılı dönüyor.
        semaphore.acquire_owned().await.expect("host semaforu kapatılmadı")
    }

    /// Bir indirmeyi host'a kaydeder. Dönen kayıt düşünce sayaç azalıyor, yani
    /// indirme biterse/duraklarsa kalanların payı kendiliğinden büyüyor.
    pub fn register(self: &Arc<Self>, host: &str) -> HostRegistration {
        *self.active.lock().unwrap().entry(host.to_string()).or_insert(0) += 1;
        HostRegistration { limiter: Arc::clone(self), host: host.to_string() }
    }

    /// Host'taki her indirmeye düşen bağlantı payı.
    ///
    /// Hiç kayıt yoksa da en az 1 dönüyor: pay sıfır olsaydı indirme hiç
    /// segment açamaz, yani hiç başlayamazdı.
    pub fn fair_share(&self, host: &str) -> usize {
        let kayitli = self.active.lock().unwrap().get(host).copied().unwrap_or(0).max(1);
        (self.max_per_host / kayitli).max(1)
    }

    pub fn max_per_host(&self) -> usize {
        self.max_per_host
    }
}

/// Bir indirmenin bir host üzerindeki varlığı. Süpervizör hayatta olduğu sürece
/// tutuluyor; düşünce host'un pay hesabından çıkıyor.
#[derive(Debug)]
pub struct HostRegistration {
    limiter: Arc<HostLimiter>,
    host: String,
}

impl Drop for HostRegistration {
    fn drop(&mut self) {
        let mut map = self.limiter.active.lock().unwrap();
        if let Some(sayi) = map.get_mut(&self.host) {
            *sayi = sayi.saturating_sub(1);
            if *sayi == 0 {
                map.remove(&self.host);
            }
        }
    }
}

/// URL'den host çıkarır. Kota anahtarı olarak kullanılıyor, bu yüzden port ve
/// kullanıcı bilgisi atılıyor: `a.com:443` ile `a.com` aynı sunucu.
pub fn host_of(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let authority = without_scheme.split(['/', '?', '#']).next().unwrap_or(without_scheme);
    // `kullanici:parola@host` biçiminde kimlik varsa at.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    // IPv6 köşeli parantez içinde: `[::1]:8080`
    if let Some(kapanis) = host.find(']') {
        return host[..=kapanis].to_ascii_lowercase();
    }
    host.split(':').next().unwrap_or(host).to_ascii_lowercase()
}

/// Zaman bazlı hız kuralı: "02:00-08:00 arası sınırsız, diğer saatler 2 MB/s".
///
/// Dakika cinsinden gece yarısından itibaren. `start > end` ise kural gece
/// yarısını aşıyor demektir (23:00-06:00 gibi) ve iki parça hâlinde değerlendirilir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthRule {
    /// Gece yarısından itibaren dakika (0..1440).
    pub start_minute: u32,
    pub end_minute: u32,
    /// Byte/saniye. `0` = bu aralıkta sınırsız.
    pub limit_bytes: u64,
    #[serde(default = "varsayilan_etkin")]
    pub enabled: bool,
}

fn varsayilan_etkin() -> bool {
    true
}

impl BandwidthRule {
    pub fn contains(&self, minute: u32) -> bool {
        if !self.enabled {
            return false;
        }
        if self.start_minute <= self.end_minute {
            minute >= self.start_minute && minute < self.end_minute
        } else {
            // Gece yarısını aşan aralık.
            minute >= self.start_minute || minute < self.end_minute
        }
    }
}

/// O an geçerli hız sınırını belirler.
///
/// Kural listesi sırayla taranıyor ve **ilk eşleşen** kazanıyor: kullanıcı
/// listeyi sıralayarak öncelik belirleyebiliyor. Hiçbir kural tutmazsa genel
/// sınır (`global_limit`) geçerli.
pub fn resolve_limit(rules: &[BandwidthRule], global_limit: u64, minute_of_day: u32) -> u64 {
    rules
        .iter()
        .find(|rule| rule.contains(minute_of_day))
        .map(|rule| rule.limit_bytes)
        .unwrap_or(global_limit)
}

/// Yerel saate göre gün içi dakika. Kurallar kullanıcının duvar saatine göre
/// yazıldığı için UTC değil yerel saat kullanılıyor.
pub fn current_minute_of_day() -> u32 {
    use chrono::Timelike;
    let now = chrono::Local::now();
    now.hour() * 60 + now.minute()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cikarma() {
        assert_eq!(host_of("https://ornek.com/a/b.zip"), "ornek.com");
        assert_eq!(host_of("http://ORNEK.com:8080/a"), "ornek.com");
        assert_eq!(host_of("https://cdn.ornek.com"), "cdn.ornek.com");
        assert_eq!(host_of("https://kullanici:parola@ornek.com/a"), "ornek.com");
        assert_eq!(host_of("https://[::1]:8080/a"), "[::1]");
    }

    #[test]
    fn kural_araligi_gun_icinde() {
        let kural = BandwidthRule {
            start_minute: 9 * 60,
            end_minute: 17 * 60,
            limit_bytes: 2_000_000,
            enabled: true,
        };

        assert!(!kural.contains(8 * 60 + 59));
        assert!(kural.contains(9 * 60));
        assert!(kural.contains(12 * 60));
        assert!(!kural.contains(17 * 60), "bitiş dakikası aralığa dahil değil");
    }

    #[test]
    fn kural_gece_yarisini_asabiliyor() {
        // 23:00 - 06:00 arası sınırsız.
        let kural = BandwidthRule {
            start_minute: 23 * 60,
            end_minute: 6 * 60,
            limit_bytes: 0,
            enabled: true,
        };

        assert!(kural.contains(23 * 60));
        assert!(kural.contains(0), "gece yarısı aralığın içinde");
        assert!(kural.contains(3 * 60));
        assert!(!kural.contains(6 * 60));
        assert!(!kural.contains(12 * 60));
    }

    #[test]
    fn kapali_kural_eslesmiyor() {
        let kural = BandwidthRule {
            start_minute: 0,
            end_minute: 1440,
            limit_bytes: 1000,
            enabled: false,
        };
        assert!(!kural.contains(600));
    }

    #[test]
    fn ilk_eslesen_kural_kazaniyor() {
        let kurallar = vec![
            BandwidthRule { start_minute: 120, end_minute: 480, limit_bytes: 0, enabled: true },
            BandwidthRule { start_minute: 0, end_minute: 1440, limit_bytes: 1_000_000, enabled: true },
        ];

        // 03:00 → ilk kural (gece, sınırsız)
        assert_eq!(resolve_limit(&kurallar, 5_000_000, 180), 0);
        // 12:00 → ikinci kural
        assert_eq!(resolve_limit(&kurallar, 5_000_000, 720), 1_000_000);
    }

    #[test]
    fn kural_yoksa_genel_sinir_gecerli() {
        assert_eq!(resolve_limit(&[], 2_000_000, 720), 2_000_000);
        assert_eq!(resolve_limit(&[], 0, 720), 0);
    }

    #[tokio::test]
    async fn sinirsiz_limiter_beklemiyor() {
        let limiter = RateLimiter::unlimited();
        let basla = Instant::now();
        for _ in 0..100 {
            limiter.consume(1_000_000).await;
        }
        assert!(basla.elapsed() < Duration::from_millis(100), "sınırsız modda bekleme olmamalı");
    }

    #[tokio::test]
    async fn sinirli_limiter_fazla_tuketimde_bekletiyor() {
        // 10 KB/s: kova 10_000 token ile dolu başlıyor.
        // Test gerçek saatle çalışıyor; süreler kısa tutuldu.
        let limiter = RateLimiter::new(10_000);

        let basla = Instant::now();
        limiter.consume(10_000).await; // Hazır token — beklemesiz.
        assert!(basla.elapsed() < Duration::from_millis(100), "dolu kova beklememeliydi");

        let basla = Instant::now();
        limiter.consume(5_000).await; // Kova boş → 5000/10000 = ~0.5 sn.
        let gecen = basla.elapsed();
        assert!(gecen >= Duration::from_millis(350), "beklenenden hızlı geçti: {gecen:?}");
        assert!(gecen < Duration::from_secs(3), "beklenenden yavaş geçti: {gecen:?}");
    }

    #[tokio::test]
    async fn sinir_calisma_aninda_degistirilebiliyor() {
        let limiter = RateLimiter::new(1000);
        assert!(!limiter.is_unlimited());
        assert_eq!(limiter.rate(), 1000);

        limiter.set_rate(0);
        assert!(limiter.is_unlimited());

        // Sınır kalktıktan sonra büyük tüketim de beklemesiz olmalı.
        let basla = Instant::now();
        limiter.consume(10_000_000).await;
        assert!(basla.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn host_kotasi_eszamanli_baglantiyi_sinirliyor() {
        let limiter = HostLimiter::new(2);

        let a = limiter.acquire("ornek.com").await;
        let b = limiter.acquire("ornek.com").await;

        // Üçüncü izin beklemeli.
        let ucuncu = tokio::time::timeout(
            Duration::from_millis(50),
            limiter.acquire("ornek.com"),
        )
        .await;
        assert!(ucuncu.is_err(), "kota dolmuşken izin verilmemeli");

        // Farklı host kendi kotasına sahip.
        let _diger = tokio::time::timeout(
            Duration::from_millis(50),
            limiter.acquire("baska.com"),
        )
        .await
        .expect("farklı host beklememeliydi");

        drop(a);
        let _serbest = tokio::time::timeout(
            Duration::from_millis(50),
            limiter.acquire("ornek.com"),
        )
        .await
        .expect("izin bırakılınca sıradaki devralmalı");

        drop(b);
    }

    #[test]
    fn pay_ayni_hosttaki_indirmeler_arasinda_bolusuyor() {
        let limiter = HostLimiter::new(8);

        // Tek indirme kotanın tamamını kullanabilir.
        let a = limiter.register("ornek.com");
        assert_eq!(limiter.fair_share("ornek.com"), 8);

        let b = limiter.register("ornek.com");
        assert_eq!(limiter.fair_share("ornek.com"), 4);

        let c = limiter.register("ornek.com");
        // 8 / 3 = 2; toplam 6 bağlantı, kota aşılmıyor ve kimse aç kalmıyor.
        assert_eq!(limiter.fair_share("ornek.com"), 2);

        // Başka host etkilenmiyor.
        assert_eq!(limiter.fair_share("baska.com"), 8);

        // İndirme bitince pay büyüyor — adaptif bölme bunu değerlendiriyor.
        drop(c);
        assert_eq!(limiter.fair_share("ornek.com"), 4);
        drop(b);
        assert_eq!(limiter.fair_share("ornek.com"), 8);
        drop(a);
        assert_eq!(limiter.fair_share("ornek.com"), 8);
    }

    #[test]
    fn pay_hicbir_zaman_sifir_olmuyor() {
        let limiter = HostLimiter::new(2);
        let _kayitlar: Vec<_> = (0..5).map(|_| limiter.register("ornek.com")).collect();

        // 2 / 5 = 0 olurdu; sıfır pay indirmenin hiç başlayamaması demek.
        assert_eq!(limiter.fair_share("ornek.com"), 1);
    }

    #[test]
    fn kayit_dusunce_host_tablodan_siliniyor() {
        let limiter = HostLimiter::new(4);
        {
            let _k = limiter.register("ornek.com");
            assert_eq!(limiter.active.lock().unwrap().len(), 1);
        }
        assert!(limiter.active.lock().unwrap().is_empty(), "kayıt sızmamalı");
    }
}
