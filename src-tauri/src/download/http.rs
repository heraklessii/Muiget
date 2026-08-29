//! Sunucu yeteneklerinin tespiti ve HTTP istemcisinin kurulumu.
//!
//! Bir indirmeye başlamadan önce üç şeyi bilmek gerekiyor: dosya ne kadar
//! büyük, sunucu `Range` destekliyor mu ve dosyanın adı ne. Bu modül bunları
//! öğrenir; segmentleme kararını [`super::segmenter`] verir.

use std::time::Duration;

use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH,
    CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE, USER_AGENT,
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use super::{DownloadError, Result};

/// Kullanıcı arayüzünde de görünen tanıtıcı. Sunucular bazen bilinmeyen
/// istemcileri reddediyor; bu yüzden gerçek bir UA gönderiliyor.
pub const DEFAULT_USER_AGENT: &str = concat!("Muiget/", env!("CARGO_PKG_VERSION"), " (+https://github.com/ilker/muiget)");

/// Dosya adı hiç çıkarılamadığında kullanılan son çare.
pub const FALLBACK_FILE_NAME: &str = "indirme";

/// Sunucunun bu URL için ne yapabildiği.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    /// Yönlendirmeler izlendikten sonraki nihai adres. Segment istekleri buraya
    /// gidiyor: her segmentin yeniden yönlendirme zincirini yürümesi gereksiz.
    pub final_url: String,
    pub supports_ranges: bool,
    pub content_length: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub file_name: String,
    pub content_type: Option<String>,
}

impl ServerCapabilities {
    /// Segmentli indirme yalnızca boyut **ve** Range desteği birlikte varken
    /// mümkün. Boyut bilinmiyorsa nereden nereye istek atılacağı da bilinmiyor.
    pub fn can_segment(&self) -> bool {
        self.supports_ranges && self.content_length.is_some_and(|len| len > 0)
    }
}

/// Ortak HTTP istemcisi.
///
/// `Accept-Encoding: identity` bilinçli: sunucu gövdeyi sıkıştırırsa okunan
/// byte sayısı `Content-Length` ve `Range` aralığıyla tutmaz, segment
/// muhasebesi bozulur ve dosya sessizce yanlış yazılır.
pub fn build_client(user_agent: &str, connect_timeout: Duration) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(user_agent)
            .map_err(|_| DownloadError::Other("geçersiz User-Agent".into()))?,
    );

    Client::builder()
        .default_headers(headers)
        .connect_timeout(connect_timeout)
        // Toplam timeout YOK: büyük dosyalarda indirme saatler sürebilir.
        // Takılan bağlantıları worker'daki okuma zaman aşımı yakalıyor.
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(DownloadError::Network)
}

/// Sunucu yeteneklerini öğrenir.
///
/// İki aşamalı, çünkü tek aşama güvenilir değil:
/// 1. `HEAD` — ucuz, çoğu sunucuda yeterli.
/// 2. `HEAD` başarısızsa ya da `Accept-Ranges` göstermiyorsa: `GET` +
///    `Range: bytes=0-0`. `206 Partial Content` dönerse Range gerçekten
///    destekleniyor demektir ve `Content-Range` toplam boyutu da verir.
///
/// İkinci aşama şart: bazı sunucular (özellikle CDN arkasındakiler) `HEAD`'e
/// `Accept-Ranges` koymuyor ama `GET`'te Range'i sorunsuz uyguluyor. Sadece
/// `HEAD`'e bakan bir istemci bu dosyaları gereksiz yere tek parça indirir.
pub async fn probe(client: &Client, url: &str) -> Result<ServerCapabilities> {
    probe_with(client, url, &[]).await
}

/// [`probe`]'un ek başlıklı hâli.
///
/// Yoklama, indirmenin kendisiyle **aynı** başlıkları göndermek zorunda: bir
/// site `Referer` olmadan 403 dönüyorsa yoklama da 403 alır ve indirme daha
/// başlamadan yanlış sonuçla biterdi.
pub async fn probe_with(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<ServerCapabilities> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DownloadError::InvalidUrl(url.to_string()));
    }

    let mut head_istegi = client.head(url);
    for (ad, deger) in headers {
        head_istegi = head_istegi.header(ad, deger);
    }
    let head = head_istegi.send().await;

    if let Ok(response) = head {
        if response.status().is_success() {
            let caps = from_response(&response, url);
            if caps.supports_ranges {
                return Ok(caps);
            }
            // Boyutu öğrendik ama Range belirsiz — ikinci aşamaya geç ve
            // öğrendiklerimizi yedek olarak sakla.
            return Ok(range_probe(client, url, headers).await.unwrap_or(caps));
        }
    }

    // HEAD ya reddedildi (405 yaygın) ya da ağ hatası verdi.
    range_probe(client, url, headers).await
}

/// `GET` + `Range: bytes=0-0` ile Range desteğini kesin olarak sınar.
async fn range_probe(
    client: &Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<ServerCapabilities> {
    let mut istek = client.get(url).header(RANGE, "bytes=0-0");
    for (ad, deger) in headers {
        istek = istek.header(ad, deger);
    }
    let response = istek.send().await?;

    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::HttpStatus { status: status.as_u16() });
    }

    let mut caps = from_response(&response, url);

    if status == StatusCode::PARTIAL_CONTENT {
        caps.supports_ranges = true;
        // 206'da Content-Length = 1 (tek byte istedik). Gerçek boyut
        // Content-Range'in payda tarafında.
        caps.content_length = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total)
            .or(caps.content_length);
    } else {
        // 200 döndü: sunucu Range'i yok saydı, tek parça inecek.
        caps.supports_ranges = false;
    }

    Ok(caps)
}

/// Yanıt başlıklarından yetenekleri çıkarır.
fn from_response(response: &reqwest::Response, requested_url: &str) -> ServerCapabilities {
    let headers = response.headers();
    let final_url = response.url().to_string();

    let header_str = |name: reqwest::header::HeaderName| -> Option<String> {
        headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
    };

    let supports_ranges = headers
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    let content_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());

    let file_name = header_str(CONTENT_DISPOSITION)
        .as_deref()
        .and_then(file_name_from_disposition)
        .or_else(|| file_name_from_url(&final_url))
        .or_else(|| file_name_from_url(requested_url))
        .map(|name| sanitize_file_name(&name))
        .unwrap_or_else(|| FALLBACK_FILE_NAME.to_string());

    ServerCapabilities {
        final_url,
        supports_ranges,
        content_length,
        etag: header_str(ETAG),
        last_modified: header_str(LAST_MODIFIED),
        file_name,
        content_type: header_str(CONTENT_TYPE),
    }
}

/// `bytes 0-0/12345` → `Some(12345)`. Boyut bilinmiyorsa (`*`) `None`.
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    let total = value.rsplit('/').next()?.trim();
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
}

/// `Content-Disposition` başlığından dosya adı çıkarır.
///
/// RFC 5987'nin `filename*=UTF-8''...` biçimi önceliklidir: Türkçe karakterli
/// adlar tarayıcıya bu şekilde geliyor ve düz `filename=` alanı genelde
/// bozulmuş ASCII karşılığını taşıyor.
pub fn file_name_from_disposition(value: &str) -> Option<String> {
    // Önce RFC 5987 genişletilmiş biçim.
    if let Some(rest) = find_param(value, "filename*") {
        // <charset>'<language>'<yüzde-kodlu ad>
        let encoded = rest.splitn(3, '\'').nth(2).unwrap_or(rest);
        let decoded = percent_decode(encoded);
        if !decoded.trim().is_empty() {
            return Some(decoded);
        }
    }

    let raw = find_param(value, "filename")?;
    let unquoted = raw.trim().trim_matches('"').trim();
    if unquoted.is_empty() {
        None
    } else {
        Some(unquoted.to_string())
    }
}

/// `key=value` parametresini ayıklar. `filename*` ararken `filename` ile
/// karışmaması için anahtarın hemen ardından `=` gelmesi şart.
///
/// `=` içermeyen parçalar (başlığın `attachment` gibi ilk sözcüğü) atlanıyor —
/// erken dönmek başlığın geri kalanını hiç okumamak demek olurdu.
fn find_param<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    for part in header.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case(key) {
            return Some(value);
        }
    }
    None
}

/// Yüzde kodlamasını çözer (`%C3%BC` → `ü`). Geçersiz dizileri olduğu gibi
/// bırakır — bozuk bir başlık yüzünden indirmeyi iptal etmeye değmez.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// URL'nin son yol parçasından dosya adı üretir. Sorgu dizesi ve fragment atılır.
pub fn file_name_from_url(url: &str) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let path = without_scheme.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?.trim();

    if last.is_empty() {
        return None;
    }

    let decoded = percent_decode(last);
    if decoded.trim().is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Dosya adını diske yazılabilir hâle getirir.
///
/// Bu fonksiyon bir **güvenlik sınırı**: ad sunucudan geliyor ve kötü niyetli
/// bir `Content-Disposition` başlığı `..\..\Windows\System32\...` içerebilir.
/// Yol ayırıcıları, sürücü harfleri ve kontrol karakterleri temizlenir; Windows
/// için ayrılmış adlar (`CON`, `NUL`, `COM1`...) ek kelimeyle etkisizleştirilir.
pub fn sanitize_file_name(raw: &str) -> String {
    // Yol ayırıcılarından sonraki son parça — dizin bileşenleri tamamen atılır.
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);

    let mut cleaned: String = base
        .chars()
        .map(|c| match c {
            // Windows'ta yasak karakterler + kontrol karakterleri.
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();

    // Windows sondaki nokta ve boşluğu sessizce siliyor: "a. " → "a".
    cleaned = cleaned.trim().trim_end_matches(['.', ' ']).trim().to_string();

    if cleaned.is_empty() {
        return FALLBACK_FILE_NAME.to_string();
    }

    // Ayrılmış aygıt adları — uzantılı hâlleri de ("NUL.txt") geçersiz.
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = cleaned.split('.').next().unwrap_or(&cleaned);
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        cleaned = format!("_{cleaned}");
    }

    // Çoğu dosya sisteminde ad sınırı 255 byte. Uzantıyı koruyarak kırp.
    const MAX_LEN: usize = 200;
    if cleaned.len() > MAX_LEN {
        let extension = cleaned
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| ext.len() <= 16 && !ext.is_empty())
            .unwrap_or("");

        let keep = MAX_LEN.saturating_sub(extension.len() + 1);
        // Karakter sınırında kes: byte ortasından bölmek geçersiz UTF-8 üretir.
        let mut stem: String = cleaned.chars().take(keep).collect();
        while stem.len() > keep {
            stem.pop();
        }
        cleaned = if extension.is_empty() { stem } else { format!("{stem}.{extension}") };
    }

    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_toplam_boyutu_veriyor() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes 200-1000/67589"), Some(67589));
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
        assert_eq!(parse_content_range_total("saçmalık"), None);
    }

    #[test]
    fn disposition_duz_filename_okunuyor() {
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"rapor.pdf\""),
            Some("rapor.pdf".to_string())
        );
        assert_eq!(
            file_name_from_disposition("attachment; filename=rapor.pdf"),
            Some("rapor.pdf".to_string())
        );
    }

    #[test]
    fn disposition_rfc5987_biciminde_turkce_ad_cozuluyor() {
        let header = "attachment; filename=\"bozuk.pdf\"; filename*=UTF-8''%C3%B6rnek%20dosya.pdf";
        assert_eq!(file_name_from_disposition(header), Some("örnek dosya.pdf".to_string()));
    }

    #[test]
    fn disposition_bos_ise_none() {
        assert_eq!(file_name_from_disposition("attachment"), None);
        assert_eq!(file_name_from_disposition("attachment; filename=\"\""), None);
    }

    #[test]
    fn url_son_parcasindan_ad_cikariyor() {
        assert_eq!(
            file_name_from_url("https://ornek.com/dosyalar/kurulum.exe"),
            Some("kurulum.exe".to_string())
        );
        assert_eq!(
            file_name_from_url("https://ornek.com/a/b.zip?token=abc#kisim"),
            Some("b.zip".to_string())
        );
        assert_eq!(
            file_name_from_url("https://ornek.com/%C3%BCr%C3%BCn.iso"),
            Some("ürün.iso".to_string())
        );
        assert_eq!(file_name_from_url("https://ornek.com/"), None);
    }

    #[test]
    fn dosya_adi_yol_kacisini_engelliyor() {
        // Content-Disposition'dan gelen dizin gezinme denemesi.
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("..\\..\\Windows\\System32\\cmd.exe"), "cmd.exe");
        assert_eq!(sanitize_file_name("C:\\Users\\ilker\\gizli.txt"), "gizli.txt");
        assert_eq!(sanitize_file_name("/mutlak/yol/dosya.bin"), "dosya.bin");
    }

    #[test]
    fn dosya_adi_yasak_karakterleri_temizliyor() {
        assert_eq!(sanitize_file_name("a<b>c:d\"e|f?g*h.txt"), "a_b_c_d_e_f_g_h.txt");
        assert_eq!(sanitize_file_name("kontrol\u{0007}karakter.bin"), "kontrol_karakter.bin");
    }

    #[test]
    fn windows_ayrilmis_adlari_etkisizlestiriliyor() {
        assert_eq!(sanitize_file_name("NUL"), "_NUL");
        assert_eq!(sanitize_file_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_file_name("COM1.zip"), "_COM1.zip");
        // Ayrılmış ad İÇEREN ama ona eşit olmayan adlar dokunulmadan geçmeli.
        assert_eq!(sanitize_file_name("console.log"), "console.log");
    }

    #[test]
    fn sondaki_nokta_ve_bosluk_kirpiliyor() {
        assert_eq!(sanitize_file_name("dosya.txt.  "), "dosya.txt");
        assert_eq!(sanitize_file_name("  bosluklu.zip  "), "bosluklu.zip");
    }

    #[test]
    fn bos_ad_yedege_dusuyor() {
        assert_eq!(sanitize_file_name(""), FALLBACK_FILE_NAME);
        assert_eq!(sanitize_file_name("..."), FALLBACK_FILE_NAME);
        assert_eq!(sanitize_file_name("///"), FALLBACK_FILE_NAME);
    }

    #[test]
    fn uzun_ad_uzanti_korunarak_kirpiliyor() {
        let uzun = format!("{}.tar.gz", "a".repeat(400));
        let sonuc = sanitize_file_name(&uzun);
        assert!(sonuc.len() <= 200, "kırpılmış ad hâlâ {} byte", sonuc.len());
        assert!(sonuc.ends_with(".gz"), "uzantı korunmalı: {sonuc}");
    }

    #[test]
    fn cok_baytli_ad_kirpilirken_utf8_bozulmuyor() {
        let uzun = "ş".repeat(300);
        let sonuc = sanitize_file_name(&uzun);
        assert!(sonuc.len() <= 200);
        // String zaten geçerli UTF-8 ise bu satır panik atmaz; asıl kontrol
        // kırpmanın karakter sınırında yapıldığı.
        assert!(sonuc.chars().all(|c| c == 'ş'));
    }

    #[test]
    fn can_segment_iki_kosula_birden_bakiyor() {
        let temel = ServerCapabilities {
            final_url: "https://ornek.com/a.zip".into(),
            supports_ranges: true,
            content_length: Some(1000),
            etag: None,
            last_modified: None,
            file_name: "a.zip".into(),
            content_type: None,
        };
        assert!(temel.can_segment());

        let mut range_yok = temel.clone();
        range_yok.supports_ranges = false;
        assert!(!range_yok.can_segment());

        let mut boyut_yok = temel.clone();
        boyut_yok.content_length = None;
        assert!(!boyut_yok.can_segment());

        let mut sifir = temel;
        sifir.content_length = Some(0);
        assert!(!sifir.can_segment());
    }
}
