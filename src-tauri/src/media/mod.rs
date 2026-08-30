//! Akış videosu indirme — HLS (`.m3u8`) ve DASH (`.mpd`).
//!
//! Normal bir HTTP indirmesinde tek bir dosya var ve iş byte aralıklarını
//! paylaştırmak. Akışta böyle bir dosya yok: manifest, yüzlerce küçük parçanın
//! adresini listeliyor ve oynatıcı onları sırayla birleştiriyor. Bu yüzden
//! `download` motorunun segment mantığı burada işe yaramıyor — orada
//! "tek dosyanın N aralığı", burada "N dosyanın tamamı" var.
//!
//! Katmanlar:
//!
//! | Modül        | Sorumluluk                                                |
//! |--------------|-----------------------------------------------------------|
//! | [`url`]      | Manifestteki göreli adresleri çözme                        |
//! | [`xml`]      | Küçük XML okuyucu (yalnızca DASH için)                     |
//! | [`m3u8`]     | HLS master + medya playlist ayrıştırma                     |
//! | [`mpd`]      | DASH manifest ayrıştırma                                   |
//! | [`crypt`]    | HLS `AES-128` segment çözme, DRM tespiti                   |
//! | [`pipeline`] | Parçaları paralel indirip **sırayla** tek dosyaya yazma     |
//! | [`mux`]      | ffmpeg ile kap dönüştürme / ses-video birleştirme          |
//!
//! ## Kapsam sınırı
//!
//! `CLAUDE.md`'deki sınır burada da geçerli: bu modül DRM'i **kırmıyor**.
//! HLS'in `AES-128` segment şifrelemesi destekleniyor çünkü anahtar
//! manifestin gösterdiği adresten herkese açık şekilde veriliyor — tarayıcıdaki
//! oynatıcı da tam olarak bunu yapıyor. Buna karşılık `SAMPLE-AES`
//! (FairPlay), Widevine ve PlayReady korumalı içerik açıkça reddediliyor;
//! bunları çözmek anahtar teslim sistemini atlatmak demek olurdu.
//!
//! ## ffmpeg
//!
//! Parçaları indirmek, çözmek ve uç uca eklemek tamamen bu kodun işi; ffmpeg
//! **isteğe bağlı** ve yalnızca iki şey için gerekiyor: MPEG-TS'i `.mp4`e
//! çevirmek ve ayrı inen ses ile videoyu tek dosyada birleştirmek. ffmpeg yoksa
//! birleştirme gerektirmeyen akışlar yine iniyor (bkz. `docs/decisions.md` #25).

pub mod crypt;
pub mod m3u8;
pub mod mpd;
pub mod mux;
pub mod pipeline;
pub mod url;
pub mod xml;

use serde::{Deserialize, Serialize};

use crate::download::{DownloadError, Result};

/// Akış protokolü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Hls,
    Dash,
}

impl Protocol {
    pub fn label(&self) -> &'static str {
        match self {
            Protocol::Hls => "HLS",
            Protocol::Dash => "DASH",
        }
    }
}

/// Parçaların kabı. Çıktı dosyasının uzantısını ve ffmpeg'e ihtiyaç olup
/// olmadığını bu belirliyor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Container {
    /// MPEG-TS (`.ts`). Uç uca eklemek geçerli bir dosya veriyor.
    ///
    /// Varsayılan: HLS'in kap belirtmeyen playlistleri MPEG-TS demek.
    #[default]
    Ts,
    /// Parçalı MP4 (fMP4/CMAF). Init parçası + medya parçaları = geçerli MP4.
    Fmp4,
    /// Ham AAC / MP3 ses akışı.
    RawAudio,
}

impl Container {
    /// ffmpeg yokken kullanılacak uzantı.
    pub fn extension(&self) -> &'static str {
        match self {
            Container::Ts => "ts",
            Container::Fmp4 => "mp4",
            Container::RawAudio => "aac",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    /// Ses ve görüntü aynı parçaların içinde — birleştirme gerekmiyor.
    ///
    /// Varsayılan: ayrı ses parçası olduğu **kanıtlanana** kadar birleşik
    /// sayılıyor. Tersi, gereksiz yere ffmpeg şartı koymak olurdu.
    #[default]
    Muxed,
    Video,
    Audio,
}

/// Bir parçanın byte aralığı (`#EXT-X-BYTERANGE`, DASH `mediaRange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: u64,
    pub length: u64,
}

impl ByteRange {
    pub fn header(&self) -> String {
        format!("bytes={}-{}", self.offset, self.offset + self.length.max(1) - 1)
    }
}

/// `AES-128` ile şifrelenmiş bir parçanın çözme bilgisi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentKey {
    /// Anahtarın indirileceği adres (16 byte ham anahtar döner).
    pub uri: String,
    /// Açıkça verilmişse IV. Verilmemişse medya sırası numarasından türetiliyor
    /// (RFC 8216 §5.2).
    pub iv: Option<[u8; 16]>,
}

/// İndirilecek tek bir parça.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSegment {
    pub url: String,
    /// Tek dosyanın içindeki aralık. `None` = nesnenin tamamı.
    pub range: Option<ByteRange>,
    /// Saniye. Boyut tahmini ve süre gösterimi için.
    pub duration: f64,
    /// Medya sırası numarası — IV türetiminde gerekiyor.
    pub sequence: u64,
    pub key: Option<SegmentKey>,
}

impl MediaSegment {
    pub fn plain(url: String) -> Self {
        MediaSegment { url, range: None, duration: 0.0, sequence: 0, key: None }
    }
}

/// Bir kalite/dil seçeneği ve onun parçaları.
#[derive(Debug, Clone, Default)]
pub struct MediaTrack {
    /// Arayüzün geri gönderdiği seçim anahtarı. HLS'te playlist adresi,
    /// DASH'te `Representation@id` üzerinden üretiliyor.
    pub id: String,
    pub kind: TrackKind,
    /// bit/saniye. Boyut tahmininin tek dayanağı.
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codecs: Option<String>,
    pub language: Option<String>,
    pub name: Option<String>,
    /// HLS `AUDIO`/`GROUP-ID` bağı. Video parçasında hangi ses grubunu
    /// istediği, ses parçasında kendi grubunun adı yazıyor; eşleşme seçim
    /// anında yapılıyor. Bir master playlistte birden çok ses grubu olabiliyor
    /// (ör. stereo ve 5.1) ve yanlış gruptan ses almak sessiz dosya demek.
    pub group: Option<String>,
    pub container: Container,
    /// HLS: bu parçanın medya playlist adresi. Parçalar ancak o indirilince
    /// biliniyor. DASH'te her şey tek belgede olduğu için `None`.
    pub playlist_url: Option<String>,
    /// fMP4'te ilk yazılacak başlangıç parçası (`#EXT-X-MAP` / DASH
    /// `Initialization`). Onsuz dosya oynatılamaz.
    pub init: Option<MediaSegment>,
    pub segments: Vec<MediaSegment>,
    /// Toplam süre (saniye).
    pub duration: f64,
}

impl MediaTrack {
    /// Parça listesi yüklendi mi? HLS master playlistinden gelen parçalarda
    /// yalnızca adres var.
    pub fn is_resolved(&self) -> bool {
        !self.segments.is_empty() || self.playlist_url.is_none()
    }

    /// Arayüzde görünecek etiket: `1920x1080 · 5.0 Mbps`.
    pub fn label(&self) -> String {
        let mut parcalar: Vec<String> = Vec::new();
        match (self.width, self.height) {
            (Some(w), Some(h)) => parcalar.push(format!("{w}x{h}")),
            (_, Some(h)) => parcalar.push(format!("{h}p")),
            _ => {}
        }
        if let Some(ad) = &self.name {
            parcalar.push(ad.clone());
        }
        if let Some(dil) = &self.language {
            if self.name.is_none() {
                parcalar.push(dil.clone());
            }
        }
        if self.bandwidth > 0 {
            parcalar.push(format!("{:.1} Mbps", self.bandwidth as f64 / 1_000_000.0));
        }
        if parcalar.is_empty() {
            parcalar.push("bilinmeyen kalite".to_string());
        }
        parcalar.join(" · ")
    }

    /// Bant genişliği ve süreden byte tahmini.
    ///
    /// Gerçek boyut ancak parçalar inerken öğreniliyor; bu tahmin ilerleme
    /// çubuğunun ilk saniyelerde bir şey gösterebilmesi için var ve indirme
    /// ilerledikçe gerçek ölçümle değiştiriliyor (bkz. [`pipeline`]).
    pub fn estimated_size(&self) -> u64 {
        if self.bandwidth == 0 || self.duration <= 0.0 {
            return 0;
        }
        (self.duration * self.bandwidth as f64 / 8.0) as u64
    }
}

/// Ayrıştırılmış manifest.
#[derive(Debug, Clone)]
pub struct MediaManifest {
    pub protocol: Protocol,
    /// Manifestin kendi adresi — göreli parça adresleri buna göre çözülüyor.
    pub url: String,
    /// Canlı yayın mı (`#EXT-X-ENDLIST` yok / MPD `type="dynamic"`).
    pub live: bool,
    pub duration: Option<f64>,
    /// Video (ya da ses+video birleşik) seçenekleri, kaliteye göre azalan sırada.
    pub video: Vec<MediaTrack>,
    /// Ayrı ses parçaları. Boşsa ses zaten video parçasının içinde.
    pub audio: Vec<MediaTrack>,
}

impl MediaManifest {
    pub fn track(&self, id: &str) -> Option<&MediaTrack> {
        self.video
            .iter()
            .chain(self.audio.iter())
            .find(|t| t.id == id)
    }
}

/// Kullanıcının kalite tercihi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Best,
    Worst,
    /// En fazla bu yükseklik (`720` → 720p ve altı).
    UpTo(u32),
}

impl Quality {
    /// Ayarlardaki dizgeyi çözer: `best` | `worst` | `1080` | `720p`.
    /// Tanınmayan değer `Best` — bozuk bir ayar yüzünden en kötü kaliteyi
    /// indirmek kullanıcının hiç istemeyeceği bir sürpriz olurdu.
    pub fn parse(s: &str) -> Quality {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "worst" | "en_dusuk" => Quality::Worst,
            "best" | "en_yuksek" | "" => Quality::Best,
            other => match other.trim_end_matches('p').parse::<u32>() {
                Ok(h) if h > 0 => Quality::UpTo(h),
                _ => Quality::Best,
            },
        }
    }
}

/// Kalite tercihine göre video parçası seçer.
///
/// Liste azalan sırada geldiği için `Best` başı, `Worst` sonu alıyor.
/// `UpTo(h)` sınırın altındaki ilk (yani en iyi) parçayı seçiyor; hiçbiri
/// sığmıyorsa **en düşüğü** dönüyor — kullanıcı "en fazla 480p" dediyse
/// 1080p indirmek onun istediğinin tam tersi olurdu.
pub fn select_video(tracks: &[MediaTrack], quality: Quality) -> Option<&MediaTrack> {
    if tracks.is_empty() {
        return None;
    }
    match quality {
        Quality::Best => tracks.first(),
        Quality::Worst => tracks.last(),
        Quality::UpTo(sinir) => tracks
            .iter()
            .find(|t| t.height.map(|h| h <= sinir).unwrap_or(false))
            .or_else(|| tracks.last()),
    }
}

/// Seçilen videoya eşlik edecek ses parçasını seçer.
///
/// Önce grup: bir master playlistte birden çok ses grubu olabiliyor ve video
/// parçasının işaret etmediği gruptan ses almak, kodek uyuşmazlığı yüzünden
/// sessiz ya da bozuk dosya demek. Grup içinde dil tercihine bakılıyor;
/// tutmuyorsa listenin ilki (manifest kendi varsayılanını başa koyuyor).
pub fn select_audio<'a>(
    tracks: &'a [MediaTrack],
    group: Option<&str>,
    language: Option<&str>,
) -> Option<&'a MediaTrack> {
    if tracks.is_empty() {
        return None;
    }

    let aday: Vec<&MediaTrack> = match group {
        Some(g) => {
            let eslesen: Vec<&MediaTrack> =
                tracks.iter().filter(|t| t.group.as_deref() == Some(g)).collect();
            // Grup bulunamazsa tüm listeye düşülüyor: bağı kurulamamış bir
            // manifest yüzünden sesi tamamen atmak daha kötü olurdu.
            if eslesen.is_empty() {
                tracks.iter().collect()
            } else {
                eslesen
            }
        }
        None => tracks.iter().collect(),
    };

    if let Some(dil) = language {
        if let Some(t) = aday.iter().find(|t| {
            t.language
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(dil))
        }) {
            return Some(t);
        }
    }
    aday.first().copied()
}

/// Adres ve içerik türünden akış protokolünü tahmin eder.
///
/// İkisine de bakılıyor çünkü ikisi de tek başına yetmiyor: CDN'ler `.m3u8`
/// uzantısını sorgu parametresinin arkasına saklıyor, bazıları da manifesti
/// `text/plain` olarak veriyor.
pub fn detect(url: &str, content_type: Option<&str>) -> Option<Protocol> {
    if let Some(tur) = content_type {
        let tur = tur.split(';').next().unwrap_or(tur).trim().to_ascii_lowercase();
        match tur.as_str() {
            "application/vnd.apple.mpegurl" | "application/x-mpegurl" | "audio/mpegurl"
            | "audio/x-mpegurl" | "application/mpegurl" => return Some(Protocol::Hls),
            "application/dash+xml" => return Some(Protocol::Dash),
            _ => {}
        }
    }

    let yol = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if yol.ends_with(".m3u8") || yol.ends_with(".m3u") {
        return Some(Protocol::Hls);
    }
    if yol.ends_with(".mpd") {
        return Some(Protocol::Dash);
    }
    None
}

/// Manifest metnini ayrıştırır.
pub fn parse(protocol: Protocol, text: &str, url: &str) -> Result<MediaManifest> {
    match protocol {
        Protocol::Hls => m3u8::parse(text, url),
        Protocol::Dash => mpd::parse(text, url),
    }
}

/// Manifest adreslerinde sık geçen, dosya adı olarak bir şey anlatmayan
/// gövdeler.
const ANLAMSIZ: [&str; 8] =
    ["index", "master", "playlist", "manifest", "video", "stream", "chunklist", "media"];

/// Bu gövde bir video için isim sayılır mı?
///
/// Tarayıcı uzantısı manifestin dosya adını (`master.m3u8`) gönderiyor ve o ad
/// kullanıcının diskinde `master.mp4` olurdu. Böyle durumlarda adresten
/// türetilen ad (`suggested_stem`) daha iyi bir tahmin.
pub fn is_generic_stem(stem: &str) -> bool {
    let temiz = stem.trim().to_ascii_lowercase();
    temiz.is_empty() || ANLAMSIZ.contains(&temiz.as_str())
}

/// Manifest adresinden makul bir dosya adı üretir (uzantısız).
///
/// `.../720p/index.m3u8` gibi adreslerde son parça anlamsız oluyor; o zaman bir
/// üst dizinin adı deneniyor. İkisi de işe yaramazsa `video`.
pub fn suggested_stem(url: &str) -> String {
    let yol = url.split(['?', '#']).next().unwrap_or(url);
    let parcalar: Vec<&str> = yol
        .trim_end_matches('/')
        .split('/')
        .filter(|p| !p.is_empty() && !p.contains(':'))
        .collect();

    for parca in parcalar.iter().rev().take(3) {
        let govde = parca.rsplit_once('.').map(|(a, _)| a).unwrap_or(parca);
        let temiz = crate::download::http::sanitize_file_name(govde);
        if is_generic_stem(&temiz) {
            continue;
        }
        return temiz;
    }

    "video".to_string()
}

/// Manifest indirmesinin üst sınırı.
///
/// Bir m3u8 ya da MPD en fazla birkaç yüz KB. Sınır, `.m3u8` sanılan bir
/// adresin aslında dev bir dosya olduğu durumda belleği doldurmasını önlüyor.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

/// Manifest (ya da playlist) metnini indirir.
pub async fn fetch_text(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<String> {
    let mut istek = client.get(url);
    for (ad, deger) in headers {
        istek = istek.header(ad, deger);
    }

    let yanit = istek.send().await?;
    let durum = yanit.status();
    if !durum.is_success() {
        return Err(DownloadError::HttpStatus { status: durum.as_u16() });
    }
    if yanit.content_length().is_some_and(|n| n > MAX_MANIFEST_BYTES) {
        return Err(DownloadError::Manifest(
            "manifest beklenenden çok büyük; bu adres bir playlist değil".into(),
        ));
    }

    let govde = yanit.bytes().await?;
    if govde.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(DownloadError::Manifest(
            "manifest beklenenden çok büyük; bu adres bir playlist değil".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&govde).into_owned())
}

/// Kullanıcının (ya da varsayılanların) parça seçimi.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSelection {
    /// Seçilen video parçasının kimliği. Boşsa kalite tercihi uygulanıyor.
    #[serde(default)]
    pub video: Option<String>,
    /// Seçilen ses parçasının kimliği. Boşsa dile göre otomatik.
    #[serde(default)]
    pub audio: Option<String>,
    /// Sesi hiç indirme. ffmpeg yokken kullanıcının elinde kalan tek yol:
    /// birleştirilemeyen bir yayından hiç değilse görüntüyü almak.
    #[serde(default)]
    pub video_only: bool,
}

/// İndirmeye hazır, parçaları çözülmüş plan.
#[derive(Debug, Clone)]
pub struct MediaPlan {
    pub protocol: Protocol,
    pub manifest_url: String,
    pub video: MediaTrack,
    /// Ayrı inen ses. `None` ise ses zaten videonun içinde (ya da istenmedi).
    pub audio: Option<MediaTrack>,
    pub container: Container,
    /// Uzantısıyla birlikte hedef dosya adı.
    pub file_name: String,
    pub estimated_size: u64,
    /// ffmpeg olmadan tamamlanamaz mı? (Ayrı ses varsa evet.)
    pub needs_ffmpeg: bool,
}

impl MediaPlan {
    /// İndirilecek toplam parça sayısı (ses dâhil).
    pub fn segment_count(&self) -> usize {
        self.video.segments.len() + self.audio.as_ref().map_or(0, |a| a.segments.len())
    }
}

/// Çıktı dosyasının uzantısı.
///
/// ffmpeg varken MPEG-TS de `.mp4`e çevriliyor: `.ts` dosyaları Windows'ta
/// çift tıklanınca çoğu zaman açılmıyor ve telefona atılamıyor. Dönüşüm
/// yeniden kodlama değil, yalnızca kap değişimi (bkz. [`mux`]).
pub fn output_extension(container: Container, merge: bool, ffmpeg: bool) -> &'static str {
    if merge {
        return "mp4";
    }
    match container {
        Container::Fmp4 => "mp4",
        Container::Ts if ffmpeg => "mp4",
        diger => diger.extension(),
    }
}

/// HLS'te bir parçanın segment listesi ayrı bir playlistte duruyor; burada
/// indirilip dolduruluyor. DASH parçaları zaten dolu geliyor.
pub async fn resolve_track(
    client: &reqwest::Client,
    track: &MediaTrack,
    headers: &[(String, String)],
) -> Result<MediaTrack> {
    if !track.segments.is_empty() {
        return Ok(track.clone());
    }
    let Some(adres) = track.playlist_url.clone() else {
        return Err(DownloadError::Manifest(format!(
            "{} parçasının ne segmenti ne de playlist adresi var",
            track.id
        )));
    };

    let metin = fetch_text(client, &adres, headers).await?;
    let playlist = m3u8::parse_media(&metin, &adres)?;
    if playlist.live {
        return Err(DownloadError::Manifest(
            "bu bir canlı yayın; kaydetme desteklenmiyor".into(),
        ));
    }

    let mut cozulmus = track.clone();
    cozulmus.container = playlist.container;
    cozulmus.init = playlist.init;
    cozulmus.duration = playlist.duration;
    cozulmus.segments = playlist.segments;
    Ok(cozulmus)
}

/// Manifestten indirilebilir bir plan çıkarır.
///
/// `ffmpeg`: ffmpeg bulundu mu. Yalnızca çıktı uzantısını etkiliyor; birleştirme
/// gerekip gerekmediği kararı ondan bağımsız.
pub async fn build_plan(
    client: &reqwest::Client,
    manifest: &MediaManifest,
    selection: &MediaSelection,
    quality: Quality,
    language: Option<&str>,
    headers: &[(String, String)],
    ffmpeg: bool,
) -> Result<MediaPlan> {
    if manifest.live {
        return Err(DownloadError::Manifest(
            "bu bir canlı yayın; kaydetme desteklenmiyor".into(),
        ));
    }

    // Seçilen kimlik listede yoksa sessizce başka bir kaliteye düşmek yerine
    // tercihe geri dönülüyor: kullanıcı 1080p seçtiyse ve manifest değiştiyse
    // 360p indirmek fark edilmeyen bir kayıp olurdu — ama hiç indirmemek de
    // orantısız. Tercih (varsayılan: en iyi) makul orta yol.
    let video_secimi = selection
        .video
        .as_deref()
        .and_then(|id| manifest.video.iter().find(|t| t.id == id))
        .or_else(|| select_video(&manifest.video, quality))
        .ok_or_else(|| DownloadError::Manifest("manifestte video parçası yok".into()))?;

    let video = resolve_track(client, video_secimi, headers).await?;

    let ses_gerekli = video.kind == TrackKind::Video && !manifest.audio.is_empty();
    let audio = if selection.video_only || !ses_gerekli {
        None
    } else {
        let secim = selection
            .audio
            .as_deref()
            .and_then(|id| manifest.audio.iter().find(|t| t.id == id))
            .or_else(|| select_audio(&manifest.audio, video.group.as_deref(), language));
        match secim {
            Some(t) => Some(resolve_track(client, t, headers).await?),
            None => None,
        }
    };

    if video.segments.is_empty() {
        return Err(DownloadError::Manifest("seçilen parçada hiç segment yok".into()));
    }

    let needs_ffmpeg = audio.is_some();
    let container = video.container;
    let uzanti = output_extension(container, needs_ffmpeg, ffmpeg);

    let tahmin = video.estimated_size() + audio.as_ref().map_or(0, |a| a.estimated_size());

    Ok(MediaPlan {
        protocol: manifest.protocol,
        manifest_url: manifest.url.clone(),
        file_name: format!("{}.{uzanti}", suggested_stem(&manifest.url)),
        video,
        audio,
        container,
        estimated_size: tahmin,
        needs_ffmpeg,
    })
}

/// Arayüzde bir kalite/ses seçeneği.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub id: String,
    pub kind: TrackKind,
    /// Hazır etiket — arayüzün yeniden biçimlendirmesi gerekmiyor.
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bandwidth: u64,
    pub codecs: Option<String>,
    pub language: Option<String>,
    pub name: Option<String>,
}

impl From<&MediaTrack> for TrackInfo {
    fn from(t: &MediaTrack) -> Self {
        TrackInfo {
            id: t.id.clone(),
            kind: t.kind,
            label: t.label(),
            width: t.width,
            height: t.height,
            bandwidth: t.bandwidth,
            codecs: t.codecs.clone(),
            language: t.language.clone(),
            name: t.name.clone(),
        }
    }
}

/// Yeni indirme penceresinin akış için gösterdiği her şey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub protocol: Protocol,
    pub live: bool,
    pub duration_seconds: Option<f64>,
    pub video: Vec<TrackInfo>,
    pub audio: Vec<TrackInfo>,
    /// Ayarlardaki tercihle seçilecek parçalar.
    ///
    /// Arayüzün açılışta hangi seçeneği işaretleyeceğini bilmesi gerekiyor;
    /// "listenin ilki" demek yanlış olurdu, çünkü kalite tercihi 720p ise
    /// motor 1080p'yi indirmeyecek. Önizleme ile indirmenin ayrışmaması için
    /// karar tek yerde, burada veriliyor.
    pub default_video: Option<String>,
    pub default_audio: Option<String>,
    /// Varsayılan seçimde ses ayrı iniyor mu — yani ffmpeg şart mı?
    pub requires_ffmpeg: bool,
    /// Bulunan ffmpeg. `None` ise arayüz uyarı gösteriyor.
    pub ffmpeg: Option<mux::FfmpegInfo>,
    pub suggested_file_name: String,
    /// Bant genişliği × süre. Gerçek boyut değil; arayüz "yaklaşık" diyor.
    pub estimated_size: u64,
}

/// Manifesti kullanıcıya anlatılabilir hâle getirir.
///
/// Varsayılan video parçasının medya playlisti de indiriliyor. Fazladan bir
/// istek ama karşılığı büyük: HLS master playlisti ne süreyi ne de yayının
/// canlı olup olmadığını söylüyor. İkisi de kullanıcının indirmeye basmadan
/// önce görmesi gereken şeyler.
pub async fn describe(
    client: &reqwest::Client,
    manifest: &MediaManifest,
    quality: Quality,
    language: Option<&str>,
    headers: &[(String, String)],
    ffmpeg: Option<mux::FfmpegInfo>,
) -> Result<MediaInfo> {
    let varsayilan = select_video(&manifest.video, quality);
    let ffmpeg_var = ffmpeg.is_some();

    let (sure, canli, kap) = match varsayilan {
        Some(t) if t.segments.is_empty() && t.playlist_url.is_some() => {
            // Canlı yayın burada anlaşılıyor; hata değil, bilgi olarak dönüyor
            // ki arayüz "canlı yayın indirilemiyor" diyebilsin.
            match resolve_track(client, t, headers).await {
                Ok(cozulmus) => (Some(cozulmus.duration), manifest.live, cozulmus.container),
                Err(DownloadError::Manifest(m)) if m.contains("canlı") => {
                    (None, true, t.container)
                }
                Err(e) => return Err(e),
            }
        }
        Some(t) => (
            Some(t.duration).filter(|d| *d > 0.0).or(manifest.duration),
            manifest.live,
            t.container,
        ),
        None => (manifest.duration, manifest.live, Container::default()),
    };

    let ses_ayri =
        varsayilan.is_some_and(|t| t.kind == TrackKind::Video) && !manifest.audio.is_empty();

    let varsayilan_ses = if ses_ayri {
        select_audio(&manifest.audio, varsayilan.and_then(|t| t.group.as_deref()), language)
    } else {
        None
    };
    let ses_bandi = varsayilan_ses.map(|a| a.bandwidth).unwrap_or(0);

    let tahmin = match (sure, varsayilan) {
        (Some(s), Some(t)) if s > 0.0 => {
            ((t.bandwidth + ses_bandi) as f64 * s / 8.0) as u64
        }
        _ => 0,
    };

    Ok(MediaInfo {
        protocol: manifest.protocol,
        live: canli,
        duration_seconds: sure.filter(|d| *d > 0.0),
        video: manifest.video.iter().map(TrackInfo::from).collect(),
        audio: manifest.audio.iter().map(TrackInfo::from).collect(),
        default_video: varsayilan.map(|t| t.id.clone()),
        default_audio: varsayilan_ses.map(|t| t.id.clone()),
        requires_ffmpeg: ses_ayri,
        ffmpeg,
        // Önizlemedeki ad, indirmenin gerçekten üreteceği adla aynı olmak
        // zorunda: uzantı hem kaba hem ffmpeg'in varlığına bağlı.
        suggested_file_name: format!(
            "{}.{}",
            suggested_stem(&manifest.url),
            output_extension(kap, ses_ayri, ffmpeg_var)
        ),
        estimated_size: tahmin,
    })
}

/// Manifest metninin gerçekten beklenen biçimde olup olmadığını sınar.
///
/// Bazı sunucular `.m3u8` adresine HTML hata sayfası döndürüyor. Bunu erken
/// yakalamak, kullanıcıya "0 parça bulundu" demekten iyi.
pub fn ensure_shape(protocol: Protocol, text: &str) -> Result<()> {
    let bas = text.trim_start();
    let uygun = match protocol {
        Protocol::Hls => bas.starts_with("#EXTM3U"),
        Protocol::Dash => bas.starts_with('<'),
    };
    if uygun {
        return Ok(());
    }
    Err(DownloadError::Manifest(format!(
        "adres {} manifesti gibi görünmüyor",
        protocol.label()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parca(id: &str, h: u32, bw: u64) -> MediaTrack {
        MediaTrack {
            id: id.to_string(),
            kind: TrackKind::Video,
            bandwidth: bw,
            width: Some(h * 16 / 9),
            height: Some(h),
            ..MediaTrack::default()
        }
    }

    #[test]
    fn protokol_icerik_turunden_taniniyor() {
        assert_eq!(detect("https://a/x", Some("application/vnd.apple.mpegurl")), Some(Protocol::Hls));
        assert_eq!(detect("https://a/x", Some("application/dash+xml; charset=utf-8")), Some(Protocol::Dash));
        assert_eq!(detect("https://a/x", Some("video/mp4")), None);
    }

    #[test]
    fn protokol_uzantidan_taniniyor() {
        assert_eq!(detect("https://a/b/list.m3u8?token=1", None), Some(Protocol::Hls));
        assert_eq!(detect("https://a/b/dash.mpd", None), Some(Protocol::Dash));
        assert_eq!(detect("https://a/b/film.mp4", None), None);
    }

    #[test]
    fn icerik_turu_uzantiya_baskin() {
        // `.mp4` uzantılı ama DASH manifesti veren sunucular var.
        assert_eq!(
            detect("https://a/b/film.mp4", Some("application/dash+xml")),
            Some(Protocol::Dash)
        );
    }

    #[test]
    fn kalite_secimi() {
        let liste = vec![parca("a", 1080, 5_000_000), parca("b", 720, 2_500_000), parca("c", 360, 800_000)];
        assert_eq!(select_video(&liste, Quality::Best).unwrap().id, "a");
        assert_eq!(select_video(&liste, Quality::Worst).unwrap().id, "c");
        assert_eq!(select_video(&liste, Quality::UpTo(720)).unwrap().id, "b");
        assert_eq!(select_video(&liste, Quality::UpTo(900)).unwrap().id, "b");
    }

    #[test]
    fn sinirin_altinda_parca_yoksa_en_dusuk_seciliyor() {
        let liste = vec![parca("a", 1080, 5_000_000), parca("b", 720, 2_500_000)];
        // "En fazla 480p" istendi ama yok: 1080p indirmek istenenin tersi olurdu.
        assert_eq!(select_video(&liste, Quality::UpTo(480)).unwrap().id, "b");
    }

    #[test]
    fn kalite_ayari_cozuluyor() {
        assert_eq!(Quality::parse("best"), Quality::Best);
        assert_eq!(Quality::parse("worst"), Quality::Worst);
        assert_eq!(Quality::parse("720p"), Quality::UpTo(720));
        assert_eq!(Quality::parse("1080"), Quality::UpTo(1080));
        // Bozuk ayar en yüksek kaliteye düşüyor, en düşüğe değil.
        assert_eq!(Quality::parse("saçmalık"), Quality::Best);
    }

    fn ses(id: &str, grup: &str, dil: &str) -> MediaTrack {
        MediaTrack {
            id: id.to_string(),
            kind: TrackKind::Audio,
            group: Some(grup.to_string()),
            language: Some(dil.to_string()),
            ..MediaTrack::default()
        }
    }

    #[test]
    fn ses_dile_gore_seciliyor() {
        let liste = vec![ses("en", "aac", "en"), ses("tr", "aac", "tr")];
        assert_eq!(select_audio(&liste, None, Some("tr")).unwrap().id, "tr");
        assert_eq!(select_audio(&liste, None, Some("de")).unwrap().id, "en");
        assert_eq!(select_audio(&liste, None, None).unwrap().id, "en");
    }

    #[test]
    fn ses_once_gruba_gore_daraltiliyor() {
        let liste = vec![
            ses("stereo-en", "aac", "en"),
            ses("surround-en", "ec3", "en"),
            ses("surround-tr", "ec3", "tr"),
        ];
        // Video "ec3" grubunu istiyor: dil eşleşse bile stereo grubu seçilmemeli.
        assert_eq!(select_audio(&liste, Some("ec3"), Some("tr")).unwrap().id, "surround-tr");
        assert_eq!(select_audio(&liste, Some("ec3"), None).unwrap().id, "surround-en");
        // Grup hiç yoksa liste bütününe düşülüyor.
        assert_eq!(select_audio(&liste, Some("yok"), Some("en")).unwrap().id, "stereo-en");
    }

    #[test]
    fn dosya_adi_anlamsiz_parcalari_atliyor() {
        assert_eq!(suggested_stem("https://cdn/x/Kara_Film/index.m3u8"), "Kara_Film");
        assert_eq!(suggested_stem("https://cdn/x/bolum-12.m3u8"), "bolum-12");
        assert_eq!(suggested_stem("https://cdn/master.m3u8"), "cdn");
        assert_eq!(suggested_stem("https://a/index/master/playlist.m3u8"), "video");
    }

    #[test]
    fn manifest_bicimi_dogrulaniyor() {
        assert!(ensure_shape(Protocol::Hls, "#EXTM3U\n#EXT-X-VERSION:3").is_ok());
        assert!(ensure_shape(Protocol::Hls, "<html>404</html>").is_err());
        assert!(ensure_shape(Protocol::Dash, "<?xml version=\"1.0\"?><MPD/>").is_ok());
        assert!(ensure_shape(Protocol::Dash, "#EXTM3U").is_err());
    }

    #[test]
    fn boyut_tahmini_bant_genisliginden() {
        let mut t = parca("a", 720, 8_000_000);
        t.duration = 10.0;
        // 8 Mbit/s × 10 s = 80 Mbit = 10 MB
        assert_eq!(t.estimated_size(), 10_000_000);
    }

    #[test]
    fn byte_araligi_basligi() {
        let r = ByteRange { offset: 100, length: 50 };
        assert_eq!(r.header(), "bytes=100-149");
    }
}
