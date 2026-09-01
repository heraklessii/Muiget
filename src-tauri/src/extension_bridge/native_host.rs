//! Chrome Native Messaging köprüsü (karar #6).
//!
//! Chrome, uzantının konuştuğu "native host"u ayrı bir süreç olarak başlatıp
//! **stdin/stdout** üzerinden mesajlaşıyor. Biçim: 4 byte uzunluk öneki
//! (yerel byte sırası) + UTF-8 JSON gövde.
//!
//! Neden yerel bir HTTP sunucusu değil: açık bir port, tarayıcıdaki *herhangi
//! bir* sekmenin indirme kuyruğuna erişebilmesi demek olurdu. Native messaging'de
//! kanalı Chrome açıyor, yalnızca manifestte adı geçen uzantılar konuşabiliyor
//! ve süreç uzantı kapanınca ölüyor.
//!
//! ## Süreç akışı
//!
//! ```text
//! Chrome ──(stdio)──> muiget --native-host ──(argv)──> çalışan Muiget penceresi
//! ```
//!
//! Köprü süreci indirmeyi kendisi yapmıyor; isteği `muiget --add <yük>` olarak
//! ana uygulamaya iletiyor. Tek örnek (single instance) eklentisi bu argümanı
//! zaten açık olan pencereye taşıyor. Böylece köprü sürecinin ömrü kısa ve
//! durumsuz kalıyor.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Manifestte ve uzantıda aynı olmak zorunda olan host adı.
pub const HOST_NAME: &str = "com.muiget.host";

/// Chrome'un uzantıdan host'a izin verdiği en büyük mesaj (1 MB).
///
/// Sınır ayrıca bir güvenlik önlemi: uzunluk önekine körü körüne güvenip
/// `Vec::with_capacity` çağırmak, bozuk ya da kötü niyetli bir önekle
/// gigabaytlarca bellek ayırtabilirdi.
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Ana uygulamayı indirme eklemek için çağırırken kullanılan argüman.
pub const ADD_FLAG: &str = "--add";

/// Köprü kipini elle açan argüman — duman testi ve elle yazılmış manifestler
/// için. Chrome bu bayrağı **geçirmiyor**; bkz. [`ORIGIN_PREFIX`].
pub const HOST_FLAG: &str = "--native-host";

/// Chrome'un köprü sürecine ilk argüman olarak verdiği çağıran kimliği.
///
/// Native messaging manifestinde özel argüman alanı yok: Chrome komutu
/// `<exe> chrome-extension://<id>/ --parent-window=<handle>` biçiminde
/// çalıştırıyor. Bu yüzden köprü kipi yalnızca [`HOST_FLAG`] ile anlaşılamaz.
/// Edge de Chromium tabanlı olduğu için aynı öneki veriyor.
pub const ORIGIN_PREFIX: &str = "chrome-extension://";

/// Köprüyü konuşan tarayıcı ailesi (karar #31).
///
/// İkisi de aynı stdio protokolünü konuşuyor; ayrıldıkları yer **manifest**:
/// izin listesinin alan adı, dosyanın durduğu yer ve Windows'ta registry kökü.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    /// Chrome ve Edge — aynı manifest biçimi, aynı `chrome-extension://` kaynağı.
    Chromium,
    Firefox,
}

impl Browser {
    /// Manifest dosya adı. Windows'ta iki manifest de aynı klasöre yazılıyor
    /// (yolu registry veriyor), bu yüzden adlarının ayrışması şart: içerikleri
    /// farklı ve biri diğerini ezerse ezilen tarayıcı köprüyü hiç göremez.
    fn manifest_file_name(self) -> String {
        match self {
            Browser::Chromium => format!("{HOST_NAME}.json"),
            Browser::Firefox => format!("{HOST_NAME}.firefox.json"),
        }
    }
}

/// Firefox uzantısının kimliği (`browser_specific_settings.gecko.id`).
///
/// Chrome kimliği uzantının açık anahtarından türüyor, yani kullanıcıdan
/// alınmak zorunda. Firefox'ta kimliği **biz** yazıyoruz: sabit, bilinen ve
/// paketten pakete değişmiyor — o yüzden kullanıcıya sormak gereksiz.
///
/// Bedeli açıkça yazalım: kimlik uzantının kendi beyanı olduğu için imzasız
/// (geçici olarak yüklenmiş) başka bir uzantı da aynı kimliği yazıp köprüye
/// ulaşabilir. Chrome'un kimliği sahtelenemez, Firefox'unki sahtelenebilir.
/// Kapı yine de dar: yalnızca `http(s)` adresleri geçiyor ve köprü indirme
/// eklemekten başka bir şey yapmıyor.
pub const FIREFOX_EXTENSION_ID: &str = "muiget@muiget.app";

/// Sürecin köprü kipinde mi başlatıldığını söyler.
///
/// [`ADD_FLAG`] varsa asla köprü kipi değil: o çağrı çalışan pencereyi
/// hedefliyor. Base64 yükü `:` içeremediği için ikisi pratikte karışamaz,
/// yine de sıra açıkça yazıldı — yükün kodlaması ileride değişirse sessiz
/// bir hataya dönüşmesin.
///
/// Firefox, Chrome'dan farklı olarak kaynağı **geçirmiyor**: köprüyü
/// `<exe> <manifest yolu> <eklenti kimliği>` ile başlatıyor (kimlik Firefox
/// 55'ten beri). Bu yüzden hem manifest dosyasının adı hem kendi eklenti
/// kimliğimiz birer köprü işareti sayılıyor; yoksa Firefox her mesaj
/// denemesinde uygulamanın penceresini açardı ve tek bir indirme bile gelmezdi.
///
/// İki işaretin de aranması bilinçli: Firefox'un manifesti nereden okuduğu
/// kuruluma göre değişebiliyor, kimlik ise sabit.
pub fn is_host_invocation(args: &[String]) -> bool {
    if args.iter().any(|a| a == ADD_FLAG) {
        return false;
    }

    args.iter().any(|a| {
        a == HOST_FLAG
            || a.starts_with(ORIGIN_PREFIX)
            || a == FIREFOX_EXTENSION_ID
            || is_manifest_argument(a)
    })
}

/// Argüman bizim native messaging manifestimizi mi gösteriyor?
///
/// Yalnızca dosya adına bakılıyor: Firefox manifesti kullanıcının seçtiği
/// klasörden okuyabiliyor ve yolu sabitlemek elle kurulumu bozardı.
fn is_manifest_argument(arg: &str) -> bool {
    let ad = arg.rsplit(['/', '\\']).next().unwrap_or(arg);
    ad == Browser::Chromium.manifest_file_name() || ad == Browser::Firefox.manifest_file_name()
}

/// Uzantıdan gelen mesajlar.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExtensionMessage {
    /// "Şu bağlantıyı Muiget ile indir."
    Download(DownloadRequest),
    /// Uzantının köprünün ayakta olduğunu anlaması için.
    Ping,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    /// Tarayıcının çözdüğü dosya adı — genelde sunucunun verdiğinden doğru.
    #[serde(default)]
    pub file_name: Option<String>,
    /// Sayfanın adresi. Hotlink korumalı sitelerde şart.
    #[serde(default)]
    pub referrer: Option<String>,
    /// Oturum çerezleri. Giriş gerektiren indirmeler için.
    #[serde(default)]
    pub cookies: Option<String>,
    /// Tarayıcının kendi UA'sı; bazı sunucular UA'ya göre farklı yanıt veriyor.
    #[serde(default)]
    pub user_agent: Option<String>,
}

impl DownloadRequest {
    /// İsteği motorun anladığı HTTP başlıklarına çevirir.
    /// Boş/whitespace değerler atlanıyor: boş bir `Referer` göndermek
    /// göndermemekten daha kötü, bazı sunucular bunu reddediyor.
    pub fn to_headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let mut ekle = |ad: &str, deger: &Option<String>| {
            if let Some(v) = deger {
                if !v.trim().is_empty() {
                    headers.push((ad.to_string(), v.trim().to_string()));
                }
            }
        };

        ekle("Referer", &self.referrer);
        ekle("Cookie", &self.cookies);
        ekle("User-Agent", &self.user_agent);
        headers
    }

    /// Yalnızca `http`/`https` kabul ediliyor.
    ///
    /// Bu bir **güvenlik sınırı**: mesaj tarayıcıdan geliyor ve `file://`,
    /// `data:` ya da `javascript:` şemalarını indirme motoruna geçirmek
    /// istemiyoruz. Motor da ayrıca kontrol ediyor; iki kat kontrol bilinçli.
    pub fn is_supported(&self) -> bool {
        let url = self.url.trim();
        url.starts_with("http://") || url.starts_with("https://")
    }
}

/// Host'tan uzantıya giden yanıtlar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostResponse {
    Accepted { url: String },
    Rejected { reason: String },
    Pong { version: String },
}

/// Bir mesaj okur. Temiz dosya sonunda (Chrome kanalı kapattı) `Ok(None)`.
///
/// Önek elle okunuyor, `read_exact` ile değil: `read_exact` "hiç byte gelmedi"
/// (kanal düzgün kapandı) ile "önek yarım kaldı" (kanal mesajın ortasında
/// koptu) durumlarının ikisine de aynı `UnexpectedEof`u veriyor. Birincisi
/// normal sonlanma, ikincisi protokol hatası — köprünün bunları ayırt etmesi
/// gerekiyor.
pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<ExtensionMessage>> {
    let mut onek = [0u8; 4];
    let mut okunan = 0;

    while okunan < onek.len() {
        match reader.read(&mut onek[okunan..]) {
            Ok(0) if okunan == 0 => return Ok(None), // Temiz kapanış.
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("uzunluk öneki yarım kaldı ({okunan}/4 byte)"),
                ))
            }
            Ok(n) => okunan += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }

    // Chrome uzunluğu **yerel** byte sırasında yazıyor (belgelenmiş davranış).
    let uzunluk = u32::from_ne_bytes(onek) as usize;

    if uzunluk == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "boş mesaj"));
    }
    if uzunluk > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("mesaj çok büyük: {uzunluk} byte (üst sınır {MAX_MESSAGE_SIZE})"),
        ));
    }

    let mut govde = vec![0u8; uzunluk];
    reader.read_exact(&mut govde)?;

    serde_json::from_slice(&govde)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("geçersiz JSON: {e}")))
}

/// Bir yanıt yazar.
pub fn write_message<W: Write>(writer: &mut W, message: &HostResponse) -> io::Result<()> {
    let govde = serde_json::to_vec(message)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    writer.write_all(&(govde.len() as u32).to_ne_bytes())?;
    writer.write_all(&govde)?;
    // Chrome yanıtı bekliyor; tamponda kalırsa uzantı asılı kalır.
    writer.flush()
}

/// İndirme isteğini `muiget --add <base64>` argümanına çevirir.
///
/// Base64 tercih edildi çünkü URL'ler boşluk, tırnak ve kabuk için anlamlı
/// karakterler içerebiliyor; ham JSON'u komut satırına koymak platformlar
/// arasında farklı biçimlerde bozulurdu.
pub fn encode_payload(request: &DownloadRequest) -> String {
    let json = serde_json::to_vec(request).unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(json)
}

pub fn decode_payload(payload: &str) -> Option<DownloadRequest> {
    let ham = base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?;
    serde_json::from_slice(&ham).ok()
}

/// Tarayıcının okuyacağı native messaging host manifesti.
///
/// İzin listesi **daraltıcı**: yalnızca burada kimliği yazan uzantı bu host'u
/// başlatabiliyor. Boş bırakmak her uzantıya kapı açmak olurdu, o yüzden liste
/// boşsa da geçerli bir (kimseyi kabul etmeyen) manifest üretiliyor.
///
/// Alan adı tarayıcıya göre değişiyor: Chromium `allowed_origins` içinde tam
/// bir kaynak adresi, Firefox `allowed_extensions` içinde çıplak kimlik
/// bekliyor. Yanlış alanı yazmak sessizce "hiçbir uzantı yetkili değil"
/// anlamına gelirdi.
pub fn manifest_json(browser: Browser, executable: &Path, allowed_extension_ids: &[String]) -> String {
    let mut manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Muiget indirme yöneticisi köprüsü",
        "path": executable.to_string_lossy(),
        "type": "stdio",
    });

    match browser {
        Browser::Chromium => {
            let origins: Vec<String> = allowed_extension_ids
                .iter()
                .map(|id| format!("{ORIGIN_PREFIX}{id}/"))
                .collect();
            manifest["allowed_origins"] = serde_json::json!(origins);
        }
        Browser::Firefox => {
            manifest["allowed_extensions"] = serde_json::json!(allowed_extension_ids);
        }
    }

    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// Manifest dosyasının platforma ve tarayıcıya göre duracağı yer.
///
/// Windows'ta konum serbest — yol registry'ye yazılıyor
/// (`HKCU\Software\Google\Chrome\NativeMessagingHosts\<ad>`). macOS ve Linux'ta
/// tarayıcılar sabit dizinlere bakıyor ve bu dizinler Chrome ile Firefox'ta
/// farklı.
pub fn manifest_path(browser: Browser, config_dir: &Path) -> PathBuf {
    let ad = browser.manifest_file_name();

    if cfg!(target_os = "windows") {
        return config_dir.join(ad);
    }

    let klasor = match (browser, cfg!(target_os = "macos")) {
        (Browser::Chromium, true) => "Library/Application Support/Google/Chrome/NativeMessagingHosts",
        (Browser::Chromium, false) => ".config/google-chrome/NativeMessagingHosts",
        (Browser::Firefox, true) => "Library/Application Support/Mozilla/NativeMessagingHosts",
        (Browser::Firefox, false) => ".mozilla/native-messaging-hosts",
    };

    // Firefox manifesti adıyla değil **host adıyla** aranıyor: sabit dizinlerde
    // dosyanın adı `com.muiget.host.json` olmak zorunda. Ad ayrımı yalnızca
    // Windows'ta, iki manifestin aynı klasörü paylaşması yüzünden gerekliydi.
    dirs_home().join(klasor).join(format!("{HOST_NAME}.json"))
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Köprünün ana döngüsü — Chrome tarafından `--native-host` ile başlatılır.
///
/// Döngü stdin kapanınca bitiyor; Chrome uzantıyı kapattığında kanal kapanıyor
/// ve süreç kendiliğinden sonlanıyor.
pub fn run_host() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut okuyucu = stdin.lock();
    let mut yazici = stdout.lock();

    while let Some(mesaj) = read_message(&mut okuyucu)? {
        let yanit = handle_message(mesaj);
        write_message(&mut yazici, &yanit)?;
    }

    Ok(())
}

/// Tek bir mesajı işler. Yan etkisi süreç başlatmak olduğu için ayrı tutuldu;
/// karar mantığı [`classify`] içinde ve saf.
fn handle_message(message: ExtensionMessage) -> HostResponse {
    match classify(message) {
        Ok(istek) => {
            match std::env::current_exe() {
                Ok(exe) => {
                    // Borular çocuğa miras bırakılmıyor.
                    //
                    // Miras kalsaydı uygulama penceresi, Chrome'un okuduğu
                    // stdout borusunu açık tutardı: köprü çıktıktan sonra bile
                    // boru kapanmaz, EOF gelmezdi. Chrome yanıtı beklemeden
                    // dağıttığı için pratikte çalışıyordu ama bu, gözlenmiş bir
                    // davranışa yaslanmak olurdu — kesmek bir satır.
                    let sonuc = std::process::Command::new(exe)
                        .arg(ADD_FLAG)
                        .arg(encode_payload(&istek))
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();

                    match sonuc {
                        Ok(_) => HostResponse::Accepted { url: istek.url },
                        Err(e) => HostResponse::Rejected {
                            reason: format!("Muiget başlatılamadı: {e}"),
                        },
                    }
                }
                Err(e) => HostResponse::Rejected {
                    reason: format!("uygulama yolu bulunamadı: {e}"),
                },
            }
        }
        Err(yanit) => yanit,
    }
}

/// Mesajın kabul edilip edilmeyeceğine karar verir. Saf fonksiyon — test edilebilir.
pub fn classify(message: ExtensionMessage) -> std::result::Result<DownloadRequest, HostResponse> {
    match message {
        ExtensionMessage::Ping => Err(HostResponse::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
        ExtensionMessage::Download(istek) => {
            if !istek.is_supported() {
                return Err(HostResponse::Rejected {
                    reason: format!("desteklenmeyen adres: {}", istek.url),
                });
            }
            Ok(istek)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Chrome'un yazacağı biçimde bir mesaj çerçeveler.
    fn cerceve(json: &str) -> Vec<u8> {
        let mut tampon = (json.len() as u32).to_ne_bytes().to_vec();
        tampon.extend_from_slice(json.as_bytes());
        tampon
    }

    #[test]
    fn indirme_mesaji_ayristiriliyor() {
        let json = r#"{"type":"download","url":"https://ornek.com/a.zip","fileName":"a.zip"}"#;
        let mut girdi = Cursor::new(cerceve(json));

        let mesaj = read_message(&mut girdi).unwrap().unwrap();
        match mesaj {
            ExtensionMessage::Download(istek) => {
                assert_eq!(istek.url, "https://ornek.com/a.zip");
                assert_eq!(istek.file_name.as_deref(), Some("a.zip"));
                assert!(istek.referrer.is_none());
            }
            other => panic!("indirme bekleniyordu: {other:?}"),
        }
    }

    #[test]
    fn ping_mesaji_ayristiriliyor() {
        let mut girdi = Cursor::new(cerceve(r#"{"type":"ping"}"#));
        assert_eq!(read_message(&mut girdi).unwrap(), Some(ExtensionMessage::Ping));
    }

    #[test]
    fn arka_arkaya_iki_mesaj_okunuyor() {
        let mut tampon = cerceve(r#"{"type":"ping"}"#);
        tampon.extend(cerceve(r#"{"type":"download","url":"https://a.com/b"}"#));
        let mut girdi = Cursor::new(tampon);

        assert_eq!(read_message(&mut girdi).unwrap(), Some(ExtensionMessage::Ping));
        assert!(matches!(
            read_message(&mut girdi).unwrap(),
            Some(ExtensionMessage::Download(_))
        ));
        // Üçüncü okuma temiz EOF.
        assert_eq!(read_message(&mut girdi).unwrap(), None);
    }

    #[test]
    fn temiz_dosya_sonu_hata_degil() {
        let mut bos = Cursor::new(Vec::new());
        assert_eq!(read_message(&mut bos).unwrap(), None);
    }

    #[test]
    fn yarim_uzunluk_oneki_hata_veriyor() {
        // Chrome kanalı ortada kesti: 4 byte'lık önek tamamlanmamış.
        let mut girdi = Cursor::new(vec![1u8, 0, 0]);
        assert!(read_message(&mut girdi).is_err());
    }

    #[test]
    fn govdesi_eksik_mesaj_hata_veriyor() {
        let mut tampon = (100u32).to_ne_bytes().to_vec();
        tampon.extend_from_slice(b"{\"type\":\"ping\"}"); // 100 byte sözü verildi, 15 geldi
        let mut girdi = Cursor::new(tampon);
        assert!(read_message(&mut girdi).is_err());
    }

    #[test]
    fn asiri_buyuk_mesaj_reddediliyor() {
        // Bellek ayırmadan ÖNCE reddedilmeli: sadece önek gönderiliyor.
        let tampon = ((MAX_MESSAGE_SIZE + 1) as u32).to_ne_bytes().to_vec();
        let mut girdi = Cursor::new(tampon);

        let hata = read_message(&mut girdi).unwrap_err();
        assert_eq!(hata.kind(), io::ErrorKind::InvalidData);
        assert!(hata.to_string().contains("çok büyük"));
    }

    #[test]
    fn sifir_uzunluklu_mesaj_reddediliyor() {
        let mut girdi = Cursor::new(0u32.to_ne_bytes().to_vec());
        assert!(read_message(&mut girdi).is_err());
    }

    #[test]
    fn bozuk_json_reddediliyor() {
        let mut girdi = Cursor::new(cerceve("{ bu json degil"));
        let hata = read_message(&mut girdi).unwrap_err();
        assert!(hata.to_string().contains("geçersiz JSON"));
    }

    #[test]
    fn yazma_cercevesi_okunabiliyor() {
        let mut cikti = Vec::new();
        let yanit = HostResponse::Accepted { url: "https://a.com/b.zip".into() };
        write_message(&mut cikti, &yanit).unwrap();

        // Önek gövde uzunluğunu vermeli.
        let uzunluk = u32::from_ne_bytes(cikti[..4].try_into().unwrap()) as usize;
        assert_eq!(uzunluk, cikti.len() - 4);

        let cozulen: HostResponse = serde_json::from_slice(&cikti[4..]).unwrap();
        assert_eq!(cozulen, yanit);
    }

    #[test]
    fn basliklar_isteklerden_uretiliyor() {
        let istek = DownloadRequest {
            url: "https://ornek.com/a.zip".into(),
            file_name: None,
            referrer: Some("https://ornek.com/sayfa".into()),
            cookies: Some("oturum=abc".into()),
            user_agent: Some("Mozilla/5.0".into()),
        };

        let headers = istek.to_headers();
        assert_eq!(
            headers,
            vec![
                ("Referer".to_string(), "https://ornek.com/sayfa".to_string()),
                ("Cookie".to_string(), "oturum=abc".to_string()),
                ("User-Agent".to_string(), "Mozilla/5.0".to_string()),
            ]
        );
    }

    #[test]
    fn bos_baslik_degerleri_atlaniyor() {
        let istek = DownloadRequest {
            url: "https://ornek.com/a.zip".into(),
            referrer: Some("   ".into()),
            cookies: Some(String::new()),
            ..Default::default()
        };
        assert!(istek.to_headers().is_empty());
    }

    #[test]
    fn yalnizca_http_semalari_kabul_ediliyor() {
        let kabul = ["https://a.com/b", "http://a.com/b"];
        for url in kabul {
            let istek = DownloadRequest { url: url.into(), ..Default::default() };
            assert!(istek.is_supported(), "{url} kabul edilmeliydi");
        }

        let red = [
            "file:///C:/Windows/System32/config/SAM",
            "javascript:alert(1)",
            "data:text/html,<script>",
            "ftp://a.com/b",
            "",
        ];
        for url in red {
            let istek = DownloadRequest { url: url.into(), ..Default::default() };
            assert!(!istek.is_supported(), "{url} reddedilmeliydi");
        }
    }

    #[test]
    fn desteklenmeyen_adres_reddediliyor() {
        let mesaj = ExtensionMessage::Download(DownloadRequest {
            url: "file:///etc/passwd".into(),
            ..Default::default()
        });

        match classify(mesaj) {
            Err(HostResponse::Rejected { reason }) => assert!(reason.contains("desteklenmeyen")),
            other => panic!("red bekleniyordu: {other:?}"),
        }
    }

    #[test]
    fn ping_pong_ile_yanitlaniyor() {
        match classify(ExtensionMessage::Ping) {
            Err(HostResponse::Pong { version }) => assert!(!version.is_empty()),
            other => panic!("pong bekleniyordu: {other:?}"),
        }
    }

    #[test]
    fn yuk_kodlama_donusu() {
        let istek = DownloadRequest {
            url: "https://ornek.com/dosya adı içeren.zip?a=1&b=2".into(),
            file_name: Some("dosya adı içeren.zip".into()),
            referrer: Some("https://ornek.com/sayfa".into()),
            cookies: None,
            user_agent: None,
        };

        let kodlu = encode_payload(&istek);
        // Kabuk için tehlikeli karakter kalmamalı.
        assert!(!kodlu.contains(' '));
        assert!(!kodlu.contains('&'));
        assert!(!kodlu.contains('"'));

        assert_eq!(decode_payload(&kodlu), Some(istek));
    }

    #[test]
    fn bozuk_yuk_none_donuyor() {
        assert_eq!(decode_payload("bu base64 degil!!!"), None);
        assert_eq!(decode_payload(""), None);
    }

    #[test]
    fn manifest_izin_verilen_uzantilari_listeliyor() {
        let manifest = manifest_json(
            Browser::Chromium,
            Path::new("C:\\Program Files\\Muiget\\muiget.exe"),
            &["abcdefghijklmnopabcdefghijklmnop".to_string()],
        );

        assert!(manifest.contains(HOST_NAME));
        assert!(manifest.contains("\"type\": \"stdio\""));
        assert!(manifest.contains("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"));
    }

    #[test]
    fn izin_listesi_bos_manifest_kimseyi_kabul_etmiyor() {
        let manifest = manifest_json(Browser::Chromium, Path::new("/usr/bin/muiget"), &[]);
        let cozulen: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(cozulen["allowed_origins"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn firefox_manifesti_ciplak_kimlik_yaziyor() {
        let manifest = manifest_json(
            Browser::Firefox,
            Path::new("/usr/bin/muiget"),
            &[FIREFOX_EXTENSION_ID.to_string()],
        );
        let cozulen: serde_json::Value = serde_json::from_str(&manifest).unwrap();

        // Firefox `allowed_extensions` okuyor; `allowed_origins` yazmak sessizce
        // "kimse yetkili değil" demek olurdu.
        assert!(cozulen.get("allowed_origins").is_none());
        assert_eq!(
            cozulen["allowed_extensions"].as_array().unwrap(),
            &vec![serde_json::json!(FIREFOX_EXTENSION_ID)]
        );
        // Kaynak öneki Firefox kimliğine yapıştırılmamalı.
        assert!(!manifest.contains(ORIGIN_PREFIX));
    }

    #[test]
    fn iki_manifest_ayni_klasorde_cakismiyor() {
        // Windows'ta ikisi de yapılandırma klasörüne yazılıyor; adları aynı
        // olsaydı ikinci yazım birincisini ezer ve o tarayıcı köprüyü göremezdi.
        let klasor = Path::new("C:\\Users\\x\\AppData\\Roaming\\Muiget");
        assert_ne!(
            manifest_path(Browser::Chromium, klasor),
            manifest_path(Browser::Firefox, klasor)
        );
    }

    #[test]
    fn firefox_manifest_yolu_kopru_kipini_aciyor() {
        // Firefox'un gerçekte verdiği argümanlar: manifest yolu + eklenti kimliği.
        let args = vec![
            "muiget.exe".to_string(),
            "C:\\Users\\x\\AppData\\Roaming\\Muiget\\com.muiget.host.firefox.json".to_string(),
            FIREFOX_EXTENSION_ID.to_string(),
        ];
        assert!(is_host_invocation(&args));

        // Manifest başka bir yoldan okunmuş olsa bile kimlik yetiyor.
        let args = vec!["muiget".to_string(), FIREFOX_EXTENSION_ID.to_string()];
        assert!(is_host_invocation(&args));

        // Linux/macOS'ta dosyanın adı host adının kendisi.
        let args = vec![
            "muiget".to_string(),
            "/home/x/.mozilla/native-messaging-hosts/com.muiget.host.json".to_string(),
        ];
        assert!(is_host_invocation(&args));
    }

    #[test]
    fn baska_json_argumani_kopru_kipi_degil() {
        let args = vec!["muiget.exe".to_string(), "C:\\bir\\ayarlar.json".to_string()];
        assert!(!is_host_invocation(&args));
    }

    #[test]
    fn chrome_kaynagi_kopru_kipini_aciyor() {
        // Chrome'un gerçekte verdiği argümanlar: kimlik + pencere tutamacı.
        let args = vec![
            "muiget.exe".to_string(),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/".to_string(),
            "--parent-window=12345".to_string(),
        ];

        assert!(is_host_invocation(&args));
    }

    #[test]
    fn elle_verilen_bayrak_kopru_kipini_aciyor() {
        let args = vec!["muiget.exe".to_string(), HOST_FLAG.to_string()];
        assert!(is_host_invocation(&args));
    }

    #[test]
    fn argumansiz_calistirma_pencere_aciyor() {
        assert!(!is_host_invocation(&["muiget.exe".to_string()]));
    }

    #[test]
    fn add_cagrisi_kopru_kipine_girmiyor() {
        let args = vec![
            "muiget.exe".to_string(),
            ADD_FLAG.to_string(),
            encode_payload(&DownloadRequest {
                url: "https://ornek.com/a.zip".into(),
                ..Default::default()
            }),
        ];

        assert!(!is_host_invocation(&args));
    }
}
