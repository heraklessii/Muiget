//! ffmpeg köprüsü — kap dönüştürme ve ses/video birleştirme.
//!
//! ## ffmpeg neden gömülü değil, neden zorunlu da değil
//!
//! Gömmek: ffmpeg'in kendisi ~70 MB ve LGPL/GPL lisans matrisi Apache-2.0 bir
//! projeye eklenince dağıtımı karmaşıklaştırıyor. Kurulum paketini on katına
//! çıkarmak, kullanıcıların çoğunun hiç ihtiyaç duymadığı bir yetenek için
//! ağır bir bedel.
//!
//! Zorunlu kılmak: parçaları indirmek, çözmek ve uç uca eklemek bu kodun işi ve
//! sonuç zaten oynatılabilir bir dosya — MPEG-TS parçaları `.ts`, fMP4 parçaları
//! `.mp4` veriyor. ffmpeg yalnızca iki şey için gerekli:
//!
//! * `.ts` → `.mp4` dönüşümü (kalite kaybı yok, yalnızca kap değişiyor)
//! * **ayrı inen ses ile videoyu birleştirmek** — DASH'te ve HLS'in ayrı ses
//!   grubu olan yayınlarında kaçınılmaz
//!
//! İkincisi gerekiyorsa ve ffmpeg yoksa indirme **hiç başlamıyor**: sessiz bir
//! video dosyası teslim etmek, hata vermekten kötü (bkz. `docs/decisions.md` #25).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::download::{DownloadError, Result};

/// Bulunan ffmpeg.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegInfo {
    pub path: String,
    /// `ffmpeg -version` çıktısının ilk satırı.
    pub version: String,
}

/// Aday ffmpeg yollarını sırayla dener ve ilk çalışanı döner.
pub async fn detect(configured: &str) -> Option<FfmpegInfo> {
    for aday in adaylar(configured) {
        if let Some(surum) = surumu_sor(&aday).await {
            return Some(FfmpegInfo { path: aday.to_string_lossy().into_owned(), version: surum });
        }
    }
    None
}

/// Denenecek yollar.
///
/// Ayarlarda bir yol yazılıysa **yalnızca o** deneniyor. Bir makinede birden
/// çok ffmpeg olabiliyor; kullanıcı hangisini istediğini yazdıysa sessizce
/// PATH'tekine düşmek sürpriz olurdu. Yazdığı yol çalışmıyorsa bunu görmesi
/// gerekiyor — başka bir sürümün arkasında saklanması değil.
///
/// Yol yazılı değilse önce uygulamanın yanına bakılıyor (taşınabilir
/// kurulumlarda ffmpeg'i exe'nin yanına atmak en kolay yol), sonra PATH'e.
fn adaylar(configured: &str) -> Vec<PathBuf> {
    let yapilandirilmis = configured.trim();
    if !yapilandirilmis.is_empty() {
        return vec![PathBuf::from(yapilandirilmis)];
    }

    let mut liste: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dizin) = exe.parent() {
            liste.push(dizin.join(ffmpeg_adi()));
        }
    }
    // PATH — çözümü işletim sistemine bırakıyoruz.
    liste.push(PathBuf::from("ffmpeg"));
    liste
}

fn ffmpeg_adi() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

async fn surumu_sor(path: &Path) -> Option<String> {
    let cikti = komut(path).arg("-version").output().await.ok()?;
    if !cikti.status.success() {
        return None;
    }
    let metin = String::from_utf8_lossy(&cikti.stdout);
    Some(metin.lines().next().unwrap_or("ffmpeg").trim().to_string())
}

/// ffmpeg'den ne istendiği.
///
/// Ses çıkarma ayrı bir kip çünkü argümanlar sadece "bir bayrak daha" değil:
/// video izi düşürülüyor (`-vn`), akış eşlemesi anlamını yitiriyor ve MP3'te
/// `-c copy` felsefesinden **bilerek** sapılıyor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuxMode {
    /// Video (varsa ses ile birlikte) yeni kaba taşınıyor. Yeniden kodlama yok.
    #[default]
    Video,
    /// Yalnızca ses, **olduğu gibi**. Yeniden kodlama yok, saniyeler sürüyor.
    ///
    /// Varsayılan ses indirme yolu bu: YouTube/HLS/DASH sesi zaten AAC ya da
    /// Opus, yani çoktan kayıplı sıkıştırılmış. MP3'e çevirmek ikinci bir
    /// kayıplı geçiş demek — kalite düşer, üstüne dakikalar sürer. Kabı
    /// değiştirip içeriğe dokunmamak hem anında hem kayıpsız.
    AudioCopy,
    /// Yalnızca ses, MP3'e kodlanmış.
    ///
    /// Kalite kaybı kaçınılmaz (bkz. [`MuxMode::AudioCopy`]); yine de var,
    /// çünkü MP3 hâlâ her cihazda çalan tek biçim ve kullanıcı bunu bilerek
    /// isteyebiliyor.
    AudioMp3 {
        kbps: u32,
    },
}

/// Birleştirme isteği.
#[derive(Debug, Clone)]
pub struct MuxRequest {
    /// Sırayla: video (ya da tek birleşik dosya), sonra varsa ses.
    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
    pub mode: MuxMode,
}

impl MuxRequest {
    /// Video birleştirme/kap dönüşümü — eski davranış.
    pub fn video(inputs: Vec<PathBuf>, output: PathBuf) -> Self {
        Self { inputs, output, mode: MuxMode::Video }
    }
}

/// Ses izini hangi uzantıyla yazmak gerektiği.
///
/// Kabı yanlış seçmek `-c copy`yi çalışmaz hale getiriyor: AAC'yi `.opus`
/// diye yazmaya kalkarsan ffmpeg akışı ogg'a koyamayacağı için hata veriyor.
/// Bu yüzden manifestteki `codecs` alanına bakılıyor; bilinmiyorsa `m4a`,
/// çünkü akış videosunda ezici çoğunluk AAC.
pub fn audio_extension(codecs: Option<&str>) -> &'static str {
    let Some(c) = codecs else { return "m4a" };
    let c = c.to_ascii_lowercase();

    // RFC 6381 kodları: mp4a.40.x = AAC, mp4a.69 / mp4a.6b = MP3.
    if c.contains("mp4a.69") || c.contains("mp4a.6b") || c.contains("mp3") {
        return "mp3";
    }
    if c.contains("opus") {
        return "opus";
    }
    if c.contains("vorbis") {
        return "ogg";
    }
    if c.contains("flac") {
        return "flac";
    }
    if c.contains("ec-3") {
        return "eac3";
    }
    if c.contains("ac-3") {
        return "ac3";
    }
    "m4a"
}

/// `+faststart` yalnızca MP4 ailesinde anlamlı; `.mp3`/`.opus` için ffmpeg
/// "geçersiz seçenek" deyip çıkıyor.
fn faststart_uygun(output: &Path) -> bool {
    matches!(
        output.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("mp4" | "m4a" | "m4v" | "mov")
    )
}

/// ffmpeg argümanlarını üretir.
///
/// `-c copy`: yeniden kodlama **yok**. İçerik olduğu gibi yeni kaba taşınıyor;
/// hem saniyeler sürüyor hem de kalite kaybı olmuyor. Bir indirme yöneticisinin
/// kullanıcının videosunu yeniden kodlaması zaten kabul edilemezdi.
pub fn build_args(req: &MuxRequest) -> Vec<String> {
    let mut args: Vec<String> = vec![
        // Kullanıcı arayüzü yok: ffmpeg'in bir şey sorup asılı kalması
        // indirmeyi sonsuza kadar bekletirdi.
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
    ];

    for girdi in &req.inputs {
        args.push("-i".into());
        args.push(girdi.to_string_lossy().into_owned());
    }

    match req.mode {
        MuxMode::Video => {
            if req.inputs.len() > 1 {
                // İlk girdiden yalnızca video, ikinciden yalnızca ses.
                // `-map 0 -map 1` demek, ayrı inen video dosyasındaki boş ses
                // izini de taşımak olurdu.
                args.push("-map".into());
                args.push("0:v:0".into());
                args.push("-map".into());
                args.push("1:a:0".into());
            }
            args.push("-c".into());
            args.push("copy".into());
        }

        // `-vn`: girdi tek başına ses dosyası olabileceği gibi tam bir video da
        // olabiliyor (kullanıcı inmiş bir videodan sesi çıkarmak istediğinde).
        // İkisinde de doğru olan video izini düşürmek.
        MuxMode::AudioCopy => {
            args.push("-vn".into());
            args.push("-c:a".into());
            args.push("copy".into());
        }

        MuxMode::AudioMp3 { kbps } => {
            args.push("-vn".into());
            args.push("-c:a".into());
            args.push("libmp3lame".into());
            args.push("-b:a".into());
            args.push(format!("{kbps}k"));
        }
    }

    // İndeks dosyanın başına: yarım inen dosya bile oynatılabiliyor ve
    // tarayıcıda akıtılabiliyor.
    if faststart_uygun(&req.output) {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    args.push(req.output.to_string_lossy().into_owned());
    args
}

/// ffmpeg'i çalıştırır. Hata hâlinde stderr'ın son satırları mesaja giriyor.
pub async fn run(ffmpeg: &Path, req: &MuxRequest) -> Result<()> {
    let cikti = komut(ffmpeg)
        .args(build_args(req))
        .output()
        .await
        .map_err(|e| DownloadError::Other(format!("ffmpeg çalıştırılamadı ({}): {e}", ffmpeg.display())))?;

    if cikti.status.success() {
        return Ok(());
    }

    let hata = String::from_utf8_lossy(&cikti.stderr);
    let ozet: Vec<&str> = hata.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
    Err(DownloadError::Other(format!(
        "ffmpeg birleştirme başarısız: {}",
        ozet.into_iter().rev().collect::<Vec<_>>().join(" | ")
    )))
}

/// Komutu kurar. Windows'ta konsol penceresi açılmasını engelliyor —
/// arka planda çalışan bir birleştirme için ekranda siyah pencere yanıp sönmesi
/// kullanıcıya çökme hissi verirdi.
fn komut(path: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(path);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    #[cfg(windows)]
    {
        // `tokio::process::Command` bu yöntemi Windows'ta kendisi sunuyor;
        // `std`nin `CommandExt` trait'ini almaya gerek yok.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tek_girdi_yalnizca_kap_degistiriyor() {
        let args = build_args(&MuxRequest::video(vec![PathBuf::from("a.ts")], PathBuf::from("a.mp4")));
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"copy".to_string()));
        // Tek girdide akış eşlemesi yok: ffmpeg varsayılanı zaten doğru.
        assert!(!args.contains(&"-map".to_string()));
        assert_eq!(args.last().unwrap(), "a.mp4");
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 1);
    }

    #[test]
    fn iki_girdi_ses_ve_videoyu_esliyor() {
        let args = build_args(&MuxRequest::video(vec![PathBuf::from("v.m4s"), PathBuf::from("a.m4s")], PathBuf::from("film.mp4")));
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        let esleme: Vec<&String> = args
            .iter()
            .skip_while(|a| *a != "-map")
            .collect();
        assert!(esleme.contains(&&"0:v:0".to_string()));
        assert!(esleme.contains(&&"1:a:0".to_string()));
    }

    #[test]
    fn ses_kopyalama_videoyu_dusuruyor_ve_kodlamiyor() {
        let args = build_args(&MuxRequest {
            inputs: vec![PathBuf::from("ses.m4s")],
            output: PathBuf::from("sarki.m4a"),
            mode: MuxMode::AudioCopy,
        });
        assert!(args.contains(&"-vn".to_string()));
        assert!(args.contains(&"copy".to_string()));
        // Kodlayıcı adı geçmemeli: kayıpsız çıkarmanın tek anlamı bu.
        assert!(!args.iter().any(|a| a.contains("lame")));
        assert_eq!(args.last().unwrap(), "sarki.m4a");
    }

    #[test]
    fn mp3_kipi_kodlayici_ve_bit_hizi_veriyor() {
        let args = build_args(&MuxRequest {
            inputs: vec![PathBuf::from("ses.m4s")],
            output: PathBuf::from("sarki.mp3"),
            mode: MuxMode::AudioMp3 { kbps: 192 },
        });
        assert!(args.contains(&"-vn".to_string()));
        assert!(args.contains(&"libmp3lame".to_string()));
        assert!(args.contains(&"192k".to_string()));
        // MP3 yeniden kodlanıyor; `copy` sızarsa bit hızı sessizce yok sayılırdı.
        assert!(!args.contains(&"copy".to_string()));
    }

    #[test]
    fn faststart_yalnizca_mp4_ailesinde() {
        // MP3/Opus çıktısında `-movflags` ffmpeg'i hatayla durduruyor.
        for (cikti, beklenen) in [("a.mp4", true), ("a.m4a", true), ("a.mp3", false), ("a.opus", false)] {
            let args = build_args(&MuxRequest {
                inputs: vec![PathBuf::from("g.m4s")],
                output: PathBuf::from(cikti),
                mode: MuxMode::AudioCopy,
            });
            assert_eq!(args.contains(&"-movflags".to_string()), beklenen, "{cikti}");
        }
    }

    #[test]
    fn ses_uzantisi_codecs_alanindan_geliyor() {
        assert_eq!(audio_extension(Some("mp4a.40.2")), "m4a");
        assert_eq!(audio_extension(Some("opus")), "opus");
        assert_eq!(audio_extension(Some("mp4a.69")), "mp3");
        assert_eq!(audio_extension(Some("ec-3")), "eac3");
        assert_eq!(audio_extension(Some("FLAC")), "flac");
        // Bilinmeyen ya da eksik: akış videosunda ezici çoğunluk AAC.
        assert_eq!(audio_extension(Some("bilinmeyen")), "m4a");
        assert_eq!(audio_extension(None), "m4a");
    }

    #[test]
    fn ac3_ile_eac3_karismiyor() {
        // "ec-3" içinde "c-3" var; sıralama yanlış olsaydı ikisi de ac3 çıkardı.
        assert_eq!(audio_extension(Some("ac-3")), "ac3");
        assert_eq!(audio_extension(Some("ec-3")), "eac3");
    }

    #[test]
    fn etkilesim_kapali() {
        let args = build_args(&MuxRequest::video(vec![PathBuf::from("a.ts")], PathBuf::from("a.mp4")));
        // `-nostdin` olmazsa ffmpeg üzerine yazma onayı bekleyip asılı kalabilir.
        assert!(args.contains(&"-nostdin".to_string()));
        assert!(args.contains(&"-y".to_string()));
    }

    #[test]
    fn yapilandirilmis_yol_tek_aday() {
        // Kullanıcı bir yol yazdıysa PATH'tekine sessizce düşmek yok: yazdığı
        // ffmpeg çalışmıyorsa bunu görmesi gerekiyor.
        let liste = adaylar("C:/araclar/ffmpeg.exe");
        assert_eq!(liste, vec![PathBuf::from("C:/araclar/ffmpeg.exe")]);
    }

    #[test]
    fn bos_ayar_yol_listesini_bozmuyor() {
        let liste = adaylar("   ");
        assert!(!liste.iter().any(|p| p.as_os_str().is_empty()));
        assert_eq!(liste.last().unwrap(), &PathBuf::from("ffmpeg"));
    }

    #[tokio::test]
    async fn olmayan_ffmpeg_bulunamiyor() {
        // Var olmayan mutlak bir yol veriliyor; PATH'te ffmpeg olabileceği için
        // yalnızca bu adayın elenmesi sınanıyor.
        assert!(surumu_sor(Path::new("C:/kesinlikle/yok/ffmpeg.exe")).await.is_none());
    }
}
