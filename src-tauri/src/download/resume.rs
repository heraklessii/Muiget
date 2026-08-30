//! `.muiget` resume meta dosyası (karar #4).
//!
//! Her indirmenin yanında `<dosya>.muiget` adında bir JSON duruyor: segment
//! aralıkları, her segmentten inen byte sayısı ve sunucunun doğrulayıcıları
//! (`ETag` / `Last-Modified`). Uygulama çökse bile kaldığı yerden devam etmek
//! için gereken her şey burada.
//!
//! JSON tercih edildi çünkü insan-okunur: kullanıcı ya da geliştirici dosyayı
//! açıp neyin ne kadar indiğini görebiliyor, ekstra veritabanı bağımlılığı
//! gerekmiyor.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::http::ServerCapabilities;
use super::segmenter::Segment;
use super::writer;
use super::{DownloadError, DownloadOptions, Result};

pub const META_EXTENSION: &str = "muiget";

/// Meta dosyası biçim sürümü. Alanların anlamı değişirse artırılır; eski
/// sürümdeki dosyalar okunmaz ve indirme baştan başlar (yanlış yorumlanmış bir
/// meta, sessizce bozuk dosya üretmekten iyidir).
pub const META_VERSION: u32 = 1;

/// `dosya.zip` → `dosya.zip.muiget`
pub fn meta_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(META_EXTENSION);
    target.with_file_name(name)
}

/// Diskteki resume durumunun sunucudaki dosyayla uyuşup uyuşmadığı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Doğrulayıcı eşleşti — devam etmek güvenli.
    Fresh,
    /// Sunucu `ETag` de `Last-Modified` de vermiyor. Boyut tuttuğu için devam
    /// ediliyor ama dosya sessizce değişmiş olabilir; arayüzde uyarılıyor.
    Unverifiable,
    /// Uyuşmuyor — baştan başlanmalı. İçindeki metin kullanıcıya gösteriliyor.
    Stale(String),
}

impl Freshness {
    /// Devam edilebilir mi? `Unverifiable` da devam ediyor: çoğu basit dosya
    /// sunucusu doğrulayıcı göndermiyor, bu yüzden reddetmek resume'u pratikte
    /// kullanılamaz hâle getirirdi.
    pub fn can_resume(&self) -> bool {
        matches!(self, Freshness::Fresh | Freshness::Unverifiable)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeMeta {
    pub version: u32,
    pub id: String,
    /// Kullanıcının verdiği özgün adres. Yönlendirme zinciri değişirse bile
    /// indirmeyi yeniden başlatmak için doğru başlangıç noktası bu.
    pub url: String,
    /// Yönlendirmeler çözüldükten sonraki adres — segment istekleri buraya gider.
    pub final_url: String,
    pub file_name: String,
    pub total_size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub segments: Vec<Segment>,
    /// Sunucunun `Range` desteği. Yalnızca arayüzde gösteriliyor ama oturumlar
    /// arası liste geri yüklenirken sunucuyu yeniden yoklamadan bilmek gerekiyor.
    #[serde(default)]
    pub supports_ranges: bool,
    /// İndirmeye özgü başlıklar ve ad ezmesi (uzantıdan gelen `Referer` gibi).
    ///
    /// Metaya yazılmasının sebebi: uygulama kapanıp açıldığında indirme
    /// listeden değil diskteki metadan geri yükleniyor. Başlıklar orada
    /// olmasaydı devam eden indirme `Referer` olmadan gidip 403 alırdı.
    #[serde(default)]
    pub options: DownloadOptions,
    /// Akış (HLS/DASH) indirmesiyse devam noktası. Sıradan HTTP indirmelerinde
    /// `None`; eski meta dosyalarında alan hiç yok.
    #[serde(default)]
    pub media: Option<MediaResume>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Akış indirmesinin devam noktası.
///
/// Sıradan indirmede devam noktası byte aralıkları (bkz. [`Segment`]); akışta
/// öyle bir şey yok, çünkü çıktı dosyası yüzlerce ayrı parçanın **sırayla**
/// eklenmesiyle büyüyor. Bu yüzden devam noktası iki sayıdan ibaret: kaç parça
/// tamamlandı ve dosya kaç byte. Dosya devam ederken bu boya kırpılıyor, yani
/// meta yazılmadan önce çökülse bile yarım kalmış son parça temizleniyor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResume {
    /// Manifestin adresi. Değişmişse devam edilmiyor.
    pub manifest_url: String,
    /// `hls` | `dash` — yalnızca gösterim ve günlük için.
    pub protocol: String,
    pub video_track: String,
    pub audio_track: Option<String>,
    pub video_total: usize,
    pub audio_total: usize,
    pub video_done: usize,
    pub audio_done: usize,
    pub video_bytes: u64,
    pub audio_bytes: u64,
    /// ffmpeg ile birleştirme gerekiyor mu (ayrı ses var).
    pub merge: bool,
    /// Arayüzde gösterilen kalite etiketi (`1920x1080 · 5.0 Mbps`).
    #[serde(default)]
    pub label: Option<String>,
}

impl MediaResume {
    /// İnmiş parça sayısı (ses dâhil).
    pub fn done(&self) -> usize {
        self.video_done + self.audio_done
    }

    /// Toplam parça sayısı (ses dâhil).
    pub fn total(&self) -> usize {
        self.video_total + self.audio_total
    }

    pub fn bytes(&self) -> u64 {
        self.video_bytes + self.audio_bytes
    }

    /// Kaydedilen devam noktası yeniden çözülen planla uyuşuyor mu?
    ///
    /// Manifest yeniden indiriliyor ve parça listesi değişmiş olabilir (CDN
    /// kalite ekleyip çıkarabiliyor, canlıdan VOD'a geçen yayınlarda parça
    /// sayısı değişiyor). Uyuşmuyorsa baştan başlamak şart: eski parçaların
    /// üzerine yenilerini eklemek sessizce bozuk bir video verirdi.
    pub fn matches(
        &self,
        manifest_url: &str,
        video_track: &str,
        audio_track: Option<&str>,
        video_total: usize,
        audio_total: usize,
    ) -> bool {
        self.manifest_url == manifest_url
            && self.video_track == video_track
            && self.audio_track.as_deref() == audio_track
            && self.video_total == video_total
            && self.audio_total == audio_total
            && self.video_done <= video_total
            && self.audio_done <= audio_total
    }
}

impl ResumeMeta {
    pub fn new(
        id: String,
        url: String,
        caps: &ServerCapabilities,
        segments: Vec<Segment>,
    ) -> Self {
        let now = unix_now();
        ResumeMeta {
            version: META_VERSION,
            id,
            url,
            final_url: caps.final_url.clone(),
            file_name: caps.file_name.clone(),
            total_size: caps.content_length.unwrap_or(0),
            etag: caps.etag.clone(),
            last_modified: caps.last_modified.clone(),
            segments,
            supports_ranges: caps.supports_ranges,
            options: DownloadOptions::default(),
            media: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Akış (HLS/DASH) indirmesi için meta.
    ///
    /// [`new`](Self::new) sunucu yeteneklerinden türetiliyor; akışta öyle bir
    /// şey yok — manifest bir dosya değil, dosya listesi. `total_size`,
    /// `etag` ve segment aralıkları bu yüzden boş: devam noktasının tamamı
    /// [`MediaResume`] içinde.
    pub fn for_media(id: String, url: String, file_name: String, media: MediaResume) -> Self {
        let now = unix_now();
        ResumeMeta {
            version: META_VERSION,
            final_url: url.clone(),
            id,
            url,
            file_name,
            total_size: 0,
            etag: None,
            last_modified: None,
            segments: Vec::new(),
            supports_ranges: false,
            options: DownloadOptions::default(),
            media: Some(media),
            created_at: now,
            updated_at: now,
        }
    }

    /// İndirmeye özgü seçenekleri metaya ekler.
    ///
    /// Ayrı bir kurucu yerine zincirlenebilir bir metot: seçenekler yalnızca
    /// uzantıdan gelen indirmelerde dolu, çağrıların çoğunda gereksiz bir
    /// parametre olurdu.
    pub fn with_options(mut self, options: DownloadOptions) -> Self {
        self.options = options;
        self
    }

    /// Tüm segmentlerden inen toplam byte.
    pub fn downloaded(&self) -> u64 {
        self.segments.iter().map(|s| s.downloaded).sum()
    }

    pub fn is_complete(&self) -> bool {
        !self.segments.is_empty() && self.segments.iter().all(Segment::is_complete)
    }

    /// Diskteki durum sunucudaki dosyayla uyuşuyor mu?
    ///
    /// Sıra önemli: önce boyut (en ucuz ve en kesin sinyal), sonra `ETag`,
    /// sonra `Last-Modified`.
    pub fn freshness(&self, caps: &ServerCapabilities) -> Freshness {
        if self.version != META_VERSION {
            return Freshness::Stale(format!(
                "meta dosyası sürüm {} (beklenen {META_VERSION})",
                self.version
            ));
        }

        match caps.content_length {
            Some(len) if len != self.total_size => {
                return Freshness::Stale(format!(
                    "dosya boyutu değişmiş: {} → {}",
                    self.total_size, len
                ));
            }
            None => {
                return Freshness::Stale("sunucu dosya boyutunu bildirmiyor".into());
            }
            _ => {}
        }

        if let (Some(kayitli), Some(guncel)) = (&self.etag, &caps.etag) {
            // Zayıf ETag (`W/"abc"`) byte-byte aynılığı garanti etmiyor ama
            // dosyanın değişmediğini söylüyor; resume için bu yeterli.
            return if normalize_etag(kayitli) == normalize_etag(guncel) {
                Freshness::Fresh
            } else {
                Freshness::Stale("sunucudaki dosya değişmiş (ETag uyuşmuyor)".into())
            };
        }

        if let (Some(kayitli), Some(guncel)) = (&self.last_modified, &caps.last_modified) {
            return if kayitli == guncel {
                Freshness::Fresh
            } else {
                Freshness::Stale("sunucudaki dosya değişmiş (Last-Modified uyuşmuyor)".into())
            };
        }

        Freshness::Unverifiable
    }

    /// Meta dosyasını **atomik** yazar.
    ///
    /// Önce `.tmp`ye yazılıp sonra `rename` ediliyor: yazma ortasında elektrik
    /// giderse yarım bir JSON kalmıyor, ya eski ya yeni sürüm oluyor. Yarım JSON
    /// tüm resume bilgisini çöpe atardı.
    pub async fn save(&mut self, target: &Path) -> Result<()> {
        self.updated_at = unix_now();

        let path = meta_path(target);
        let tmp = path.with_extension(format!("{META_EXTENSION}.tmp"));

        let json = serde_json::to_vec_pretty(self)?;
        tokio::fs::write(&tmp, &json).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    /// Meta dosyasını okur. Dosya yoksa `Ok(None)` — bu bir hata değil, sadece
    /// "yeni indirme" demek.
    pub async fn load(target: &Path) -> Result<Option<Self>> {
        let path = meta_path(target);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let meta: ResumeMeta = serde_json::from_slice(&bytes)
                    .map_err(|e| DownloadError::Meta(format!("{}: {e}", path.display())))?;
                Ok(Some(meta))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(DownloadError::Io(e)),
        }
    }

    /// İndirme bitince meta dosyasını siler. Silinemezse hata **yutuluyor**:
    /// dosya başarıyla indi, artık kalan bir çöp dosya yüzünden kullanıcıya
    /// hata göstermenin anlamı yok.
    pub async fn cleanup(target: &Path) {
        let _ = tokio::fs::remove_file(meta_path(target)).await;
    }
}

/// `dosya.zip.muiget` → `dosya.zip`. [`meta_path`] fonksiyonunun tersi.
pub fn target_from_meta_path(meta: &Path) -> Option<PathBuf> {
    let ad = meta.file_name()?.to_str()?;
    let govde = ad.strip_suffix(&format!(".{META_EXTENSION}"))?;
    if govde.is_empty() {
        return None; // Adı yalnızca ".muiget" olan dosya bize ait değil.
    }
    Some(meta.with_file_name(govde))
}

/// Bir klasördeki yarım kalmış indirmeleri bulur.
///
/// Uygulama kapanınca indirme listesi bellekte kalmıyor; diskte kalan tek iz
/// `.muiget` meta dosyaları. Bu fonksiyon onları toplayıp (hedef dosya, meta)
/// çiftleri olarak döndürüyor — listeyi oturumlar arası taşımanın yolu bu.
///
/// Klasörün kendisine ve **yalnızca kategori alt klasörlerine** bakılıyor.
/// Serbest özyineleme yok: kullanıcının İndirilenler klasörü altında binlerce
/// dosya olabilir ve açılışı yavaşlatmak, kapsamı genişletmekten pahalıya
/// gelirdi. Kategori klasörleri istisna çünkü dosyaları oraya bu uygulama
/// koyuyor (bkz. [`super::category`]); taranmasalardı kategori açıkken yarım
/// indirmeler açılışta listeye hiç dönmezdi.
///
/// Sağlıksız kayıtlar sessizce eleniyor:
/// * bozuk ya da okunamayan JSON — log'a yazılıp geçiliyor
/// * eski sürüm meta — [`ResumeMeta::freshness`] zaten reddederdi
/// * `.mgpart` dosyası kaybolmuş meta — öksüz sayılıp siliniyor. Listede
///   "yarısı inmiş" gösterip sonra sıfırdan başlamak kullanıcıyı yanıltırdı.
pub async fn scan_directory(dir: &Path) -> Vec<(PathBuf, ResumeMeta)> {
    let mut bulunan = scan_one(dir).await;

    for kategori in super::category::folder_names() {
        let alt = dir.join(kategori);
        // Klasör yoksa `scan_one` zaten boş dönüyor; ayrıca var mı diye
        // bakmak fazladan bir sistem çağrısı olurdu.
        bulunan.extend(scan_one(&alt).await);
    }

    // En eski indirme en başta: listenin sırası oturumlar arasında korunuyor.
    bulunan.sort_by_key(|(_, meta)| meta.created_at);
    bulunan
}

/// Tek bir klasörü tarar — alt klasörlere inmez.
async fn scan_one(dir: &Path) -> Vec<(PathBuf, ResumeMeta)> {
    let mut girdiler = match tokio::fs::read_dir(dir).await {
        Ok(g) => g,
        Err(e) => {
            // Klasör silinmiş ya da erişilemiyor olabilir; açılışı durduracak
            // bir sebep değil.
            // Kategori klasörleri çoğu kurulumda hiç yok; her açılışta
            // "taranamadı" diye uyarmak log'u anlamsız yere doldururdu.
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("{} taranamadı: {e}", dir.display());
            }
            return Vec::new();
        }
    };

    let mut bulunan = Vec::new();
    while let Ok(Some(girdi)) = girdiler.next_entry().await {
        let meta_yolu = girdi.path();
        if meta_yolu.extension().and_then(|u| u.to_str()) != Some(META_EXTENSION) {
            continue;
        }
        let Some(target) = target_from_meta_path(&meta_yolu) else {
            continue;
        };

        match ResumeMeta::load(&target).await {
            Ok(Some(meta)) => {
                if meta.version != META_VERSION {
                    log::warn!("{} sürüm {} — atlanıyor", meta_yolu.display(), meta.version);
                    continue;
                }
                if !writer::part_path(&target).exists() {
                    log::info!("öksüz meta siliniyor: {}", meta_yolu.display());
                    let _ = tokio::fs::remove_file(&meta_yolu).await;
                    continue;
                }
                bulunan.push((target, meta));
            }
            // `load` dosyayı bulamadıysa arada silinmiş demektir; sorun değil.
            Ok(None) => {}
            Err(e) => log::warn!("{} okunamadı: {e}", meta_yolu.display()),
        }
    }

    bulunan
}

/// `W/"abc"` ve `"abc"` aynı kaynağı gösteriyor; karşılaştırmadan önce zayıflık
/// öneki ve tırnaklar atılıyor.
fn normalize_etag(etag: &str) -> &str {
    etag.trim().trim_start_matches("W/").trim_matches('"')
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ornek_caps() -> ServerCapabilities {
        ServerCapabilities {
            final_url: "https://ornek.com/a.zip".into(),
            supports_ranges: true,
            content_length: Some(1000),
            etag: Some("\"abc123\"".into()),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
            file_name: "a.zip".into(),
            content_type: Some("application/zip".into()),
        }
    }

    fn ornek_meta() -> ResumeMeta {
        let segments = super::super::segmenter::plan_segments(1000, 4, 1);
        ResumeMeta::new("id-1".into(), "https://ornek.com/a.zip".into(), &ornek_caps(), segments)
    }

    #[test]
    fn meta_yolu_uzanti_ekliyor() {
        let p = meta_path(Path::new("/indirmeler/film.mkv"));
        assert_eq!(p.file_name().unwrap(), "film.mkv.muiget");
    }

    #[test]
    fn inen_byte_toplami_segmentlerden_geliyor() {
        let mut meta = ornek_meta();
        assert_eq!(meta.downloaded(), 0);
        assert!(!meta.is_complete());

        meta.segments[0].downloaded = 100;
        meta.segments[1].downloaded = 50;
        assert_eq!(meta.downloaded(), 150);

        for s in &mut meta.segments {
            s.downloaded = s.total();
        }
        assert_eq!(meta.downloaded(), 1000);
        assert!(meta.is_complete());
    }

    #[test]
    fn etag_ayni_ise_taze() {
        let meta = ornek_meta();
        assert_eq!(meta.freshness(&ornek_caps()), Freshness::Fresh);
    }

    #[test]
    fn zayif_etag_ile_gucclu_etag_esdeger_sayiliyor() {
        let mut meta = ornek_meta();
        meta.etag = Some("W/\"abc123\"".into());
        assert_eq!(meta.freshness(&ornek_caps()), Freshness::Fresh);
    }

    #[test]
    fn etag_degistiyse_bayat() {
        let meta = ornek_meta();
        let mut caps = ornek_caps();
        caps.etag = Some("\"farkli\"".into());

        match meta.freshness(&caps) {
            Freshness::Stale(mesaj) => assert!(mesaj.contains("ETag")),
            other => panic!("bayat beklenirken {other:?} döndü"),
        }
    }

    #[test]
    fn boyut_degistiyse_etag_bakilmadan_bayat() {
        let meta = ornek_meta();
        let mut caps = ornek_caps();
        caps.content_length = Some(2000); // ETag hâlâ aynı ama boyut değişti

        match meta.freshness(&caps) {
            Freshness::Stale(mesaj) => assert!(mesaj.contains("boyut")),
            other => panic!("bayat beklenirken {other:?} döndü"),
        }
    }

    #[test]
    fn etag_yoksa_last_modified_kullaniliyor() {
        let mut meta = ornek_meta();
        meta.etag = None;
        let mut caps = ornek_caps();
        caps.etag = None;

        assert_eq!(meta.freshness(&caps), Freshness::Fresh);

        caps.last_modified = Some("Thu, 22 Oct 2026 07:28:00 GMT".into());
        assert!(matches!(meta.freshness(&caps), Freshness::Stale(_)));
    }

    #[test]
    fn hicbir_dogrulayici_yoksa_dogrulanamaz_ama_devam_edilebilir() {
        let mut meta = ornek_meta();
        meta.etag = None;
        meta.last_modified = None;
        let mut caps = ornek_caps();
        caps.etag = None;
        caps.last_modified = None;

        let taze = meta.freshness(&caps);
        assert_eq!(taze, Freshness::Unverifiable);
        assert!(taze.can_resume());
    }

    #[test]
    fn boyut_bilinmiyorsa_devam_edilemez() {
        let meta = ornek_meta();
        let mut caps = ornek_caps();
        caps.content_length = None;

        assert!(!meta.freshness(&caps).can_resume());
    }

    #[test]
    fn eski_surum_metasi_reddediliyor() {
        let mut meta = ornek_meta();
        meta.version = 0;
        match meta.freshness(&ornek_caps()) {
            Freshness::Stale(mesaj) => assert!(mesaj.contains("sürüm")),
            other => panic!("bayat beklenirken {other:?} döndü"),
        }
    }

    #[tokio::test]
    async fn kaydet_yukle_donusu_veriyi_koruyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");

        let mut meta = ornek_meta();
        meta.segments[1].downloaded = 42;
        meta.save(&target).await.unwrap();

        let okunan = ResumeMeta::load(&target).await.unwrap().expect("meta bulunmalı");
        assert_eq!(okunan.id, meta.id);
        assert_eq!(okunan.segments, meta.segments);
        assert_eq!(okunan.downloaded(), 42);
        assert_eq!(okunan.etag, meta.etag);
    }

    #[tokio::test]
    async fn olmayan_meta_hata_degil() {
        let dir = tempfile::tempdir().unwrap();
        let sonuc = ResumeMeta::load(&dir.path().join("yok.zip")).await.unwrap();
        assert!(sonuc.is_none());
    }

    #[tokio::test]
    async fn bozuk_meta_anlasilir_hata_veriyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");
        tokio::fs::write(meta_path(&target), b"{ bu json degil").await.unwrap();

        let hata = ResumeMeta::load(&target).await.unwrap_err();
        assert!(matches!(hata, DownloadError::Meta(_)), "beklenmeyen hata: {hata:?}");
    }

    #[tokio::test]
    async fn kaydetme_gecici_dosya_birakmiyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");

        ornek_meta().save(&target).await.unwrap();

        let mut girdiler = tokio::fs::read_dir(dir.path()).await.unwrap();
        while let Some(girdi) = girdiler.next_entry().await.unwrap() {
            let ad = girdi.file_name().to_string_lossy().into_owned();
            assert!(!ad.ends_with(".tmp"), "geçici dosya kalmış: {ad}");
        }
    }

    #[test]
    fn meta_yolu_ile_hedef_yolu_birbirinin_tersi() {
        let hedef = Path::new("/indirmeler/film.mkv");
        let meta = meta_path(hedef);
        assert_eq!(target_from_meta_path(&meta).unwrap(), hedef);

        // Bize ait olmayan dosyalar reddedilmeli.
        assert!(target_from_meta_path(Path::new("/indirmeler/film.mkv")).is_none());
        assert!(target_from_meta_path(Path::new("/indirmeler/.muiget")).is_none());
    }

    #[tokio::test]
    async fn secenekler_meta_ile_birlikte_saklaniyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");

        let secenekler = DownloadOptions {
            headers: vec![("Referer".into(), "https://ornek.com/sayfa".into())],
            file_name: Some("gercek-ad.zip".into()),
        };
        ornek_meta().with_options(secenekler.clone()).save(&target).await.unwrap();

        let okunan = ResumeMeta::load(&target).await.unwrap().unwrap();
        assert_eq!(okunan.options, secenekler, "Referer kaybolursa devam eden indirme 403 alır");
    }

    #[tokio::test]
    async fn secenek_alani_olmayan_eski_meta_hala_okunuyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");

        // `options` ve `supportsRanges` alanları eklenmeden önce yazılmış meta.
        let eski = r#"{
            "version": 1,
            "id": "eski-1",
            "url": "https://ornek.com/a.zip",
            "finalUrl": "https://ornek.com/a.zip",
            "fileName": "a.zip",
            "totalSize": 1000,
            "etag": null,
            "lastModified": null,
            "segments": [{ "index": 0, "start": 0, "end": 999, "downloaded": 400 }],
            "createdAt": 1,
            "updatedAt": 2
        }"#;
        tokio::fs::write(meta_path(&target), eski).await.unwrap();

        let okunan = ResumeMeta::load(&target).await.unwrap().unwrap();
        assert_eq!(okunan.downloaded(), 400);
        assert!(okunan.options.is_empty());
        assert!(!okunan.supports_ranges);
    }

    /// `scan_directory` yalnızca `.mgpart`ı duran metaları döndürüyor; testlerin
    /// çoğu bu ikiliyi kurmak zorunda.
    async fn yarim_indirme_kur(dir: &Path, ad: &str, created_at: u64) -> PathBuf {
        let target = dir.join(ad);
        tokio::fs::write(writer::part_path(&target), b"yarim").await.unwrap();

        let mut meta = ornek_meta();
        meta.id = format!("id-{ad}");
        meta.file_name = ad.to_string();
        meta.save(&target).await.unwrap();

        // `save` her yazışta `updated_at`i tazeliyor; `created_at` elle
        // ayarlanıp sıralamanın sınanabilmesi için ikinci kez yazılıyor.
        meta.created_at = created_at;
        let json = serde_json::to_vec_pretty(&meta).unwrap();
        tokio::fs::write(meta_path(&target), json).await.unwrap();

        target
    }

    #[tokio::test]
    async fn tarama_yarim_indirmeleri_eskiden_yeniye_buluyor() {
        let dir = tempfile::tempdir().unwrap();
        yarim_indirme_kur(dir.path(), "ikinci.bin", 200).await;
        yarim_indirme_kur(dir.path(), "birinci.bin", 100).await;

        let bulunan = scan_directory(dir.path()).await;
        let adlar: Vec<_> = bulunan.iter().map(|(_, m)| m.file_name.clone()).collect();
        assert_eq!(adlar, vec!["birinci.bin", "ikinci.bin"], "sıra oluşturma zamanına göre olmalı");
    }

    #[tokio::test]
    async fn tarama_oksuz_metayi_atiyor_ve_siliyor() {
        let dir = tempfile::tempdir().unwrap();
        let saglam = yarim_indirme_kur(dir.path(), "saglam.bin", 100).await;

        // Yarım dosyası olmayan meta: kullanıcı `.mgpart`ı elle silmiş ya da
        // indirme bitmiş de temizlik yarım kalmış olabilir.
        let oksuz = dir.path().join("oksuz.bin");
        ornek_meta().save(&oksuz).await.unwrap();

        let bulunan = scan_directory(dir.path()).await;
        assert_eq!(bulunan.len(), 1, "yalnızca .mgpart'ı duran indirme dönmeli");
        assert_eq!(bulunan[0].0, saglam);
        assert!(!meta_path(&oksuz).exists(), "öksüz meta silinmeliydi");
    }

    #[tokio::test]
    async fn tarama_ilgisiz_dosyalari_gormezden_geliyor() {
        let dir = tempfile::tempdir().unwrap();
        yarim_indirme_kur(dir.path(), "gercek.bin", 100).await;

        tokio::fs::write(dir.path().join("film.mkv"), b"x").await.unwrap();
        tokio::fs::write(dir.path().join("baska.bin.mgpart"), b"x").await.unwrap();
        // Yarıda kalmış atomik yazmadan artan geçici dosya.
        tokio::fs::write(dir.path().join("yarim.bin.muiget.tmp"), b"{").await.unwrap();

        let bulunan = scan_directory(dir.path()).await;
        assert_eq!(bulunan.len(), 1);
        assert_eq!(bulunan[0].1.file_name, "gercek.bin");
    }

    #[tokio::test]
    async fn tarama_bozuk_metayi_atlayip_devam_ediyor() {
        let dir = tempfile::tempdir().unwrap();
        yarim_indirme_kur(dir.path(), "saglam.bin", 100).await;

        let bozuk = dir.path().join("bozuk.bin");
        tokio::fs::write(writer::part_path(&bozuk), b"x").await.unwrap();
        tokio::fs::write(meta_path(&bozuk), b"{ bu json degil").await.unwrap();

        // Tek bozuk dosya yüzünden tüm listeyi kaybetmemeliyiz.
        let bulunan = scan_directory(dir.path()).await;
        assert_eq!(bulunan.len(), 1);
        assert_eq!(bulunan[0].1.file_name, "saglam.bin");
    }

    #[tokio::test]
    async fn olmayan_klasorun_taranmasi_patlamiyor() {
        let bulunan = scan_directory(Path::new("/kesinlikle/olmayan/klasor")).await;
        assert!(bulunan.is_empty());
    }

    #[tokio::test]
    async fn temizleme_meta_dosyasini_siliyor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.zip");
        ornek_meta().save(&target).await.unwrap();
        assert!(meta_path(&target).exists());

        ResumeMeta::cleanup(&target).await;
        assert!(!meta_path(&target).exists());

        // İkinci çağrı da patlamamalı — dosya zaten yok.
        ResumeMeta::cleanup(&target).await;
    }

    #[tokio::test]
    async fn tarama_kategori_klasorlerine_de_bakiyor() {
        let dir = tempfile::tempdir().unwrap();

        // Kategori kapalıyken inen bir dosya kökte.
        yarim_indirme_kur(dir.path(), "kokte.bin", 100).await;

        // Kategori açıkken inenler alt klasörde. Taranmasalardı bu indirmeler
        // uygulama yeniden açılınca listeye hiç dönmezdi.
        let video = dir.path().join("Video");
        tokio::fs::create_dir_all(&video).await.unwrap();
        yarim_indirme_kur(&video, "film.mkv", 200).await;

        let muzik = dir.path().join("Müzik");
        tokio::fs::create_dir_all(&muzik).await.unwrap();
        yarim_indirme_kur(&muzik, "parca.mp3", 300).await;

        let bulunan = scan_directory(dir.path()).await;
        let adlar: Vec<_> = bulunan.iter().map(|(_, m)| m.file_name.clone()).collect();
        assert_eq!(adlar, vec!["kokte.bin", "film.mkv", "parca.mp3"]);

        // Yollar gerçekten alt klasörü gösteriyor.
        let film = bulunan.iter().find(|(_, m)| m.file_name == "film.mkv").unwrap();
        assert!(film.0.starts_with(&video), "hedef yol kategori klasöründe olmalı");
    }

    #[tokio::test]
    async fn tarama_kategori_disi_alt_klasore_inmiyor() {
        let dir = tempfile::tempdir().unwrap();
        let baska = dir.path().join("RastgeleKlasor");
        tokio::fs::create_dir_all(&baska).await.unwrap();
        yarim_indirme_kur(&baska, "gizli.bin", 100).await;

        // Serbest özyineleme bilinçli olarak yok (karar #15).
        assert!(scan_directory(dir.path()).await.is_empty());
    }
}
