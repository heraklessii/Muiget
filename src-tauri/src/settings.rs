//! Kalıcı uygulama ayarları.
//!
//! Ayarlar uygulamanın yapılandırma dizininde `settings.json` olarak duruyor.
//! Motor ayarları ([`ManagerConfig`]) da buraya gömülü — indirme motorunun
//! kendisi dosya sistemini tanımıyor, ayarları yalnızca veri olarak alıyor.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::manager::ManagerConfig;
use crate::download::{DownloadError, Result};

pub const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// İndirilen dosyaların varsayılan hedefi.
    pub download_dir: PathBuf,
    /// `dark` | `light`. Arayüz `<html data-theme>` üzerinden uyguluyor.
    pub theme: String,
    /// Uygulama kapatılınca sistem tepsisinde çalışmaya devam etsin mi.
    #[serde(default = "varsayilan_dogru")]
    pub minimize_to_tray: bool,
    /// İndirme bitince bildirim.
    #[serde(default = "varsayilan_dogru")]
    pub notify_on_complete: bool,
    /// Açılışta diskten geri yüklenen yarım indirmeler kendiliğinden devam
    /// etsin mi.
    ///
    /// Varsayılan kapalı: uygulamayı açar açmaz bağlantının dolması, kullanıcı
    /// istemeden olacak bir şey. Liste yine de dolu geliyor, sürdürmek tek
    /// tıklık iş.
    #[serde(default)]
    pub resume_on_start: bool,
    /// Panoya kopyalanan bağlantıyı yakalayıp indirme önersin mi (karar #24).
    ///
    /// Varsayılan kapalı: panoyu sürekli okumak, kullanıcının kopyaladığı her
    /// şeyi görmek demek. Böyle bir yeteneğin sessizce açık gelmesi bu projede
    /// yanlış olurdu.
    #[serde(default)]
    pub clipboard_watch: bool,
    /// Açılışta GitHub'daki son sürüme baksın mı (karar #23).
    ///
    /// Uygulamanın kendiliğinden yaptığı **tek** dış istek bu. Kullanıcı verisi
    /// taşımıyor; yine de kapatılabiliyor.
    #[serde(default = "varsayilan_dogru")]
    pub check_updates: bool,
    /// Motor ayarları — segment sayısı, hız sınırı, zaman kuralları.
    #[serde(default)]
    pub engine: ManagerConfig,
    /// Native messaging köprüsünü kullanmasına izin verilen Chrome uzantısı
    /// kimlikleri. Boşken hiçbir uzantı köprüyü başlatamaz.
    #[serde(default)]
    pub extension_ids: Vec<String>,
}

fn varsayilan_dogru() -> bool {
    true
}

impl AppSettings {
    /// `download_dir` dışarıdan veriliyor: platformun "İndirilenler" klasörünü
    /// bulmak Tauri'nin işi, bu modülün değil.
    pub fn with_download_dir(download_dir: PathBuf) -> Self {
        AppSettings {
            download_dir,
            theme: "dark".to_string(),
            minimize_to_tray: true,
            notify_on_complete: true,
            resume_on_start: false,
            clipboard_watch: false,
            check_updates: true,
            engine: ManagerConfig::default(),
            extension_ids: Vec::new(),
        }
    }

    /// Ayarları okur. Dosya yoksa **ya da bozuksa** varsayılana düşer.
    ///
    /// Bozuk dosyada hata döndürmek uygulamayı açılamaz hâle getirirdi; elle
    /// düzenlenmiş tek bir virgül hatası yüzünden kullanıcının indirme
    /// yöneticisini kaybetmesi kabul edilemez. Bunun yerine varsayılana dönülüp
    /// log'a yazılıyor.
    pub async fn load(config_dir: &Path, fallback_download_dir: PathBuf) -> Self {
        let path = config_dir.join(SETTINGS_FILE);
        match tokio::fs::read(&path).await {
            Ok(bytes) => match serde_json::from_slice::<AppSettings>(&bytes) {
                Ok(mut ayarlar) => {
                    ayarlar.normalize();
                    ayarlar
                }
                Err(e) => {
                    log::warn!("{} okunamadı ({e}); varsayılanlara dönülüyor", path.display());
                    AppSettings::with_download_dir(fallback_download_dir)
                }
            },
            Err(_) => AppSettings::with_download_dir(fallback_download_dir),
        }
    }

    /// Atomik yazma — `resume.rs`'teki gerekçenin aynısı: yarım JSON, ayarların
    /// tamamının kaybı demek.
    pub async fn save(&self, config_dir: &Path) -> Result<()> {
        tokio::fs::create_dir_all(config_dir).await?;

        let path = config_dir.join(SETTINGS_FILE);
        let tmp = path.with_extension("json.tmp");

        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| DownloadError::Other(format!("ayarlar serileştirilemedi: {e}")))?;
        tokio::fs::write(&tmp, &json).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    /// Elle düzenlenmiş ya da eski sürümden gelen dosyadaki saçma değerleri
    /// makul aralığa çeker. Motor bu değerlere güvenerek çalışıyor.
    pub fn normalize(&mut self) {
        use crate::download::segmenter::{MAX_SEGMENTS, MIN_SEGMENT_SIZE};

        self.engine.segments = self.engine.segments.clamp(1, MAX_SEGMENTS);
        self.engine.max_connections_per_host = self.engine.max_connections_per_host.clamp(1, 64);
        // 0 = sınırsız, bilinçli olarak korunuyor; üst sınır makine değil
        // sunucu tarafında anlam kazanıyor.
        self.engine.max_concurrent_downloads = self.engine.max_concurrent_downloads.min(64);
        self.engine.max_retries = self.engine.max_retries.min(20);
        self.engine.min_segment_size = self.engine.min_segment_size.max(64 * 1024);
        self.engine.min_steal_size = self.engine.min_steal_size.max(MIN_SEGMENT_SIZE / 4);
        self.engine.connect_timeout_secs = self.engine.connect_timeout_secs.clamp(3, 120);
        self.engine.read_timeout_secs = self.engine.read_timeout_secs.clamp(5, 600);

        if self.engine.user_agent.trim().is_empty() {
            self.engine.user_agent = crate::download::http::DEFAULT_USER_AGENT.to_string();
        }
        self.engine.proxy = normalize_proxy(&self.engine.proxy);

        // Akış ayarları (karar #25). Kalite dizgesi kırpılmıyor: tanınmayan
        // değer zaten `Quality::parse` içinde "en yüksek"e düşüyor ve orada
        // gerekçesi yazılı.
        self.engine.ffmpeg_path = self.engine.ffmpeg_path.trim().to_string();
        self.engine.media_language = self.engine.media_language.trim().to_ascii_lowercase();
        self.engine.media_quality = self.engine.media_quality.trim().to_ascii_lowercase();
        // Üst sınır 16: bir CDN'e daha fazla eşzamanlı parça isteği atmak
        // indirmeyi hızlandırmıyor, 429 riskini artırıyor.
        self.engine.media_concurrency = self.engine.media_concurrency.clamp(1, 16);
        if self.theme != "light" {
            self.theme = "dark".to_string();
        }

        // Gün sınırlarını aşan kural sessizce yanlış davranır; kırp.
        for kural in &mut self.engine.bandwidth_rules {
            kural.start_minute = kural.start_minute.min(1439);
            kural.end_minute = kural.end_minute.min(1440);
        }

        // Geçersiz kimlikler atılıyor: bu değer native messaging manifestine
        // yazılıyor ve manifest bozulursa köprü hiç çalışmaz. Sessizce kırpmak,
        // yanlış bir kimliği "kurulmuş" gibi göstermekten iyi.
        self.extension_ids.retain(|id| gecerli_uzanti_kimligi(id));
        self.extension_ids.dedup();
    }
}

/// Proxy adresini kullanılabilir hâle getirir (karar #19).
///
/// Şemasız yazılan `10.0.0.1:8080` gibi adresler `http://` sayılıyor — insanlar
/// vekil adresini böyle not ediyor ve şema istemek gereksiz bir tökezleme
/// noktası. Desteklenmeyen şemalar **boşaltılıyor**: geçersiz bir vekille
/// istemci hiç kurulamaz ve uygulama indirme yapamaz hâle gelirdi; sessizce
/// doğrudan bağlanmak, çalışmayan bir yapılandırmadan iyi.
fn normalize_proxy(raw: &str) -> String {
    let temiz = raw.trim();
    if temiz.is_empty() {
        return String::new();
    }

    let Some((sema, kalan)) = temiz.split_once("://") else {
        return format!("http://{temiz}");
    };

    const DESTEKLENEN: [&str; 5] = ["http", "https", "socks5", "socks5h", "socks4"];
    if kalan.is_empty() || !DESTEKLENEN.contains(&sema.to_ascii_lowercase().as_str()) {
        log::warn!("proxy adresi kullanılamadı, doğrudan bağlanılacak: {temiz}");
        return String::new();
    }

    temiz.to_string()
}

/// Chrome uzantı kimliği: tam 32 karakter, yalnızca `a`–`p` arası küçük harf.
/// (Chrome, uzantının açık anahtarının SHA-256 özetini bu alfabeye çeviriyor.)
pub fn gecerli_uzanti_kimligi(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::throttle::BandwidthRule;

    fn ornek() -> AppSettings {
        AppSettings::with_download_dir(PathBuf::from("/indirmeler"))
    }

    #[test]
    fn varsayilan_ayarlar_makul() {
        let a = ornek();
        assert_eq!(a.theme, "dark");
        assert_eq!(a.engine.segments, 8);
        assert!(a.engine.adaptive);
        assert_eq!(a.engine.global_speed_limit, 0, "varsayılan sınırsız olmalı");
    }

    #[test]
    fn normalize_sacma_degerleri_duzeltiyor() {
        let mut a = ornek();
        a.engine.segments = 999;
        a.engine.max_connections_per_host = 0;
        a.engine.max_retries = 100;
        a.engine.min_segment_size = 1;
        a.engine.connect_timeout_secs = 0;
        a.engine.read_timeout_secs = 100_000;
        a.engine.user_agent = "   ".into();
        a.theme = "mor".into();

        a.normalize();

        assert_eq!(a.engine.segments, 32);
        assert_eq!(a.engine.max_connections_per_host, 1);
        assert_eq!(a.engine.max_retries, 20);
        assert_eq!(a.engine.min_segment_size, 64 * 1024);
        assert_eq!(a.engine.connect_timeout_secs, 3);
        assert_eq!(a.engine.read_timeout_secs, 600);
        assert!(!a.engine.user_agent.trim().is_empty());
        assert_eq!(a.theme, "dark", "bilinmeyen tema koyuya düşmeli");
    }

    #[test]
    fn normalize_gun_disi_kural_dakikalarini_kirpiyor() {
        let mut a = ornek();
        a.engine.bandwidth_rules = vec![BandwidthRule {
            start_minute: 5000,
            end_minute: 9999,
            limit_bytes: 1000,
            enabled: true,
        }];

        a.normalize();

        assert_eq!(a.engine.bandwidth_rules[0].start_minute, 1439);
        assert_eq!(a.engine.bandwidth_rules[0].end_minute, 1440);
    }

    #[test]
    fn proxy_semasiz_adrese_http_ekliyor() {
        assert_eq!(normalize_proxy("10.0.0.1:8080"), "http://10.0.0.1:8080");
        assert_eq!(normalize_proxy("  vekil.local:3128  "), "http://vekil.local:3128");
    }

    #[test]
    fn proxy_desteklenen_semalari_koruyor() {
        for adres in [
            "http://vekil:8080",
            "https://vekil:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
            "http://ali:gizli@vekil:8080",
        ] {
            assert_eq!(normalize_proxy(adres), adres);
        }
    }

    #[test]
    fn proxy_desteklenmeyen_sema_bosaltiliyor() {
        assert_eq!(normalize_proxy("ftp://vekil:21"), "");
        assert_eq!(normalize_proxy("socks5://"), "");
        assert_eq!(normalize_proxy(""), "");
        assert_eq!(normalize_proxy("   "), "");
    }

    #[test]
    fn normalize_proxy_ayarini_da_temizliyor() {
        let mut a = ornek();
        a.engine.proxy = "  vekil.local:3128 ".into();
        a.normalize();
        assert_eq!(a.engine.proxy, "http://vekil.local:3128");

        a.engine.proxy = "ftp://olmaz".into();
        a.normalize();
        assert_eq!(a.engine.proxy, "", "geçersiz vekil doğrudan bağlantıya düşmeli");
    }

    #[test]
    fn uzanti_kimligi_dogrulaniyor() {
        assert!(gecerli_uzanti_kimligi("abcdefghijklmnopabcdefghijklmnop"));
        assert!(gecerli_uzanti_kimligi(&"a".repeat(32)));

        // Yanlış uzunluk
        assert!(!gecerli_uzanti_kimligi(&"a".repeat(31)));
        assert!(!gecerli_uzanti_kimligi(&"a".repeat(33)));
        assert!(!gecerli_uzanti_kimligi(""));
        // Alfabe dışı karakter (`q` ve sonrası, rakam, büyük harf)
        assert!(!gecerli_uzanti_kimligi(&"q".repeat(32)));
        assert!(!gecerli_uzanti_kimligi(&"A".repeat(32)));
        assert!(!gecerli_uzanti_kimligi(&"1".repeat(32)));
        // Manifest'e enjekte edilmeye çalışılan değer
        assert!(!gecerli_uzanti_kimligi("aaaa/\", \"allowed_origins\": [\"*\"]"));
    }

    #[test]
    fn normalize_gecersiz_uzanti_kimliklerini_atiyor() {
        let mut a = ornek();
        let gecerli = "abcdefghijklmnopabcdefghijklmnop".to_string();
        a.extension_ids = vec![gecerli.clone(), "kisa".into(), "Z".repeat(32)];

        a.normalize();

        assert_eq!(a.extension_ids, vec![gecerli]);
    }

    #[tokio::test]
    async fn kaydet_yukle_donusu() {
        let dir = tempfile::tempdir().unwrap();

        let mut a = ornek();
        a.theme = "light".into();
        a.engine.segments = 16;
        a.engine.global_speed_limit = 3_000_000;
        a.save(dir.path()).await.unwrap();

        let okunan = AppSettings::load(dir.path(), PathBuf::from("/yedek")).await;
        assert_eq!(okunan.theme, "light");
        assert_eq!(okunan.engine.segments, 16);
        assert_eq!(okunan.engine.global_speed_limit, 3_000_000);
        assert_eq!(okunan.download_dir, PathBuf::from("/indirmeler"));
    }

    #[tokio::test]
    async fn dosya_yoksa_varsayilan_donuyor() {
        let dir = tempfile::tempdir().unwrap();
        let a = AppSettings::load(dir.path(), PathBuf::from("/yedek")).await;
        assert_eq!(a.download_dir, PathBuf::from("/yedek"));
    }

    #[tokio::test]
    async fn bozuk_dosya_uygulamayi_kilitlemez() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(SETTINGS_FILE), b"{ bozuk json,,,").await.unwrap();

        let a = AppSettings::load(dir.path(), PathBuf::from("/yedek")).await;
        assert_eq!(a.download_dir, PathBuf::from("/yedek"), "bozuk dosyada varsayılana düşmeli");
        assert_eq!(a.engine.segments, 8);
    }

    #[tokio::test]
    async fn eksik_alanlar_varsayilanla_dolduruluyor() {
        // Eski sürümden kalan, yalnızca iki alanı olan bir dosya.
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(
            dir.path().join(SETTINGS_FILE),
            br#"{"downloadDir":"/eski","theme":"light"}"#,
        )
        .await
        .unwrap();

        let a = AppSettings::load(dir.path(), PathBuf::from("/yedek")).await;
        assert_eq!(a.download_dir, PathBuf::from("/eski"));
        assert_eq!(a.theme, "light");
        assert_eq!(a.engine.segments, 8, "eksik motor ayarı varsayılandan gelmeli");
        assert!(a.minimize_to_tray);
        assert!(a.check_updates, "eksik alan varsayılan olarak açık gelmeli");
        assert!(!a.clipboard_watch, "pano izleme eksik alanda kapalı gelmeli");
        assert_eq!(a.engine.proxy, "", "eksik alanda doğrudan bağlantı");

        // Akış ayarları (karar #25) sonradan eklendi; eski dosyalarda yoklar.
        // `media_concurrency` özellikle önemli: serde varsayılanı olmasaydı 0
        // okunur ve hiçbir video parçası inmezdi.
        assert_eq!(a.engine.media_quality, "best");
        assert_eq!(a.engine.media_concurrency, 6);
        assert_eq!(a.engine.ffmpeg_path, "", "eksik alanda ffmpeg otomatik aranmalı");
        assert_eq!(a.engine.media_language, "");
    }

    #[tokio::test]
    async fn akis_ayarlari_normalize_ediliyor() {
        let mut a = ornek();
        a.engine.media_concurrency = 0;
        a.engine.ffmpeg_path = "  C:/araclar/ffmpeg.exe  ".into();
        a.engine.media_language = " TR ".into();
        a.engine.media_quality = " 720P ".into();
        a.normalize();

        // 0 eşzamanlı parça indirmeyi sonsuza kadar bekletirdi.
        assert_eq!(a.engine.media_concurrency, 1);
        assert_eq!(a.engine.ffmpeg_path, "C:/araclar/ffmpeg.exe");
        assert_eq!(a.engine.media_language, "tr");
        assert_eq!(a.engine.media_quality, "720p");

        a.engine.media_concurrency = 99;
        a.normalize();
        assert_eq!(a.engine.media_concurrency, 16);
    }

    #[tokio::test]
    async fn kaydetme_gecici_dosya_birakmiyor() {
        let dir = tempfile::tempdir().unwrap();
        ornek().save(dir.path()).await.unwrap();

        let mut girdiler = tokio::fs::read_dir(dir.path()).await.unwrap();
        while let Some(g) = girdiler.next_entry().await.unwrap() {
            assert!(!g.file_name().to_string_lossy().ends_with(".tmp"));
        }
    }
}
