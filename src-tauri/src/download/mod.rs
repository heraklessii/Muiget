//! İndirme motoru.
//!
//! Katmanlar (aşağıdan yukarı):
//!
//! | Modül        | Sorumluluk                                                  |
//! |--------------|-------------------------------------------------------------|
//! | [`http`]     | Sunucu yeteneklerini öğrenme (Range desteği, boyut, ad)     |
//! | [`segmenter`]| Dosyayı byte aralıklarına bölme planı                        |
//! | [`writer`]   | Sparse dosya + kendi offsetine yazan segment yazıcısı        |
//! | [`resume`]   | `.muiget` meta dosyası, kaldığı yerden devam                 |
//! | [`worker`]   | Tek bir segmenti indiren async task (retry + backoff)        |
//! | [`speed`]    | Segment ve toplam hız ölçümü (EWMA)                          |
//! | [`throttle`] | Bant genişliği sınırlama (token bucket) + host bağlantı kotası |
//! | [`manager`]  | Hepsini orkestre eden yönetici, adaptif segment bölme         |
//!
//! Mimari kararların gerekçesi için `docs/decisions.md` #3, #4, #5.

pub mod http;
pub mod manager;
pub mod resume;
pub mod segmenter;
pub mod speed;
pub mod throttle;
pub mod worker;
pub mod writer;

use serde::{Deserialize, Serialize, Serializer};

pub type Result<T> = std::result::Result<T, DownloadError>;

/// Bir indirmeye özgü seçenekler.
///
/// Chrome uzantısından gelen indirmeler için şart: çoğu site `Referer`
/// olmadan dosyayı vermiyor, bazıları oturum çerezi bekliyor. Tarayıcı bu
/// başlıkları zaten biliyor; köprü onları buraya taşıyor.
///
/// Motorun kökünde duruyor çünkü hem [`manager`] hem [`resume`] kullanıyor:
/// seçenekler resume metasına da yazılıyor. Yoksa uygulama yeniden açıldığında
/// `Referer` kaybolur, devam eden indirme 403 alırdı.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOptions {
    /// Her segment isteğine eklenecek başlıklar.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Sunucudan gelen adı ezmek için. Uzantı, tarayıcının çözdüğü adı
    /// gönderiyor; o ad genelde daha doğru oluyor.
    #[serde(default)]
    pub file_name: Option<String>,
}

impl DownloadOptions {
    /// Taşıdığı bir bilgi var mı?
    ///
    /// Metaya yazarken gerekiyor: elle eklenen boş bir seçenek kümesinin,
    /// tarayıcıdan gelen başlıkların üzerine yazması istenmiyor.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.file_name.is_none()
    }
}

/// İndirme sırasında oluşabilecek hatalar.
///
/// `Serialize` elle yazıldı: Tauri komutlarının `Err` dalı frontend'e JSON
/// olarak geçiyor ve orada tek ihtiyacımız okunabilir bir mesaj. Yapısal
/// serileştirme (variant + alanlar) arayüzde hiçbir işe yaramıyordu.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("ağ hatası: {0}")]
    Network(#[from] reqwest::Error),

    #[error("dosya hatası: {0}")]
    Io(#[from] std::io::Error),

    #[error("sunucu {status} döndürdü")]
    HttpStatus { status: u16 },

    #[error("geçersiz URL: {0}")]
    InvalidUrl(String),

    #[error("resume meta dosyası okunamadı: {0}")]
    Meta(String),

    /// Sunucu `Range` isteğini yok sayıp dosyanın tamamını göndermeye başladı.
    /// Segment #0 dışında bu ölümcül: yazmaya devam etmek dosyayı bozar.
    #[error("sunucu Range isteğini yok saydı (segment {segment})")]
    RangeIgnored { segment: usize },

    #[error("indirme iptal edildi")]
    Cancelled,

    #[error("indirme duraklatıldı")]
    Paused,

    #[error("{0} numaralı indirme bulunamadı")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl Serialize for DownloadError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<serde_json::Error> for DownloadError {
    fn from(e: serde_json::Error) -> Self {
        DownloadError::Meta(e.to_string())
    }
}

/// Byte sayısını insan okunur hâle çevirir. Hem log'da hem testlerde lazım.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_birimleri_dogru_seciyor() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn hata_mesaji_string_olarak_serilestiriliyor() {
        let err = DownloadError::HttpStatus { status: 404 };
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"sunucu 404 döndürdü\"");
    }
}
