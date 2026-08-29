//! Pano izleme — kopyalanan bağlantıyı yakalayıp indirme önerme.
//!
//! IDM'in en çok kullanılan davranışlarından biri (karar #24). Bağlantıya sağ
//! tıklamadan, uzantı kurmadan: adresi kopyala, uygulama sorsun.
//!
//! **Varsayılan kapalı.** Panoyu sürekli okumak, kullanıcının kopyaladığı her
//! şeyi (parola yöneticisinden gelen bir parola dâhil) uygulamanın görmesi
//! demek. Bu projede böyle bir şeyin sessizce açık gelmesi doğru olmazdı;
//! ayarlardaki anahtar ne yaptığını açıkça yazıyor.
//!
//! Okunan metin hiçbir yere yazılmıyor: yalnızca bu modüldeki filtreden
//! geçiyor, indirilebilir bir bağlantı değilse anında düşüyor. Eşleşen adres
//! bile indirmeyi kendiliğinden başlatmıyor — arayüzde bir öneri çıkıyor.

use crate::download::category;

/// Panoda beklenebilecek makul en uzun adres. Daha uzun metinler (yapıştırılan
/// bir yazı, bir base64 blob) zaten bağlantı değil.
const MAKUL_UZUNLUK: usize = 2048;

/// Pano içeriği indirilebilir bir bağlantı mı?
///
/// Ölçüt bilinçli olarak dar: yalnızca **tek satırlık**, `http(s)` şemalı ve
/// yolunun sonunda **tanınan bir dosya uzantısı** olan adresler. Her URL'yi
/// yakalamak, kullanıcı bir haber sayfasının adresini kopyaladığında da
/// sormak demekti; ikinci kez rahatsız eden bir özellik kapatılır.
pub fn indirilebilir_baglanti(pano: &str) -> Option<String> {
    let metin = pano.trim();

    if metin.len() > MAKUL_UZUNLUK || metin.is_empty() {
        return None;
    }
    // Boşluk ya da satır sonu içeren metin tek bir adres değil.
    if metin.chars().any(char::is_whitespace) {
        return None;
    }
    if !(metin.starts_with("http://") || metin.starts_with("https://")) {
        return None;
    }

    let ad = crate::download::http::file_name_from_url(metin)?;
    category::folder_for(&ad)?;

    Some(metin.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dosya_uzantili_adres_yakalaniyor() {
        assert_eq!(
            indirilebilir_baglanti("https://ornek.com/film.mkv"),
            Some("https://ornek.com/film.mkv".to_string())
        );
        assert_eq!(
            indirilebilir_baglanti("  http://ornek.com/a/b/kurulum.exe?token=1  "),
            Some("http://ornek.com/a/b/kurulum.exe?token=1".to_string()),
            "sorgu dizesi adresin parçası, kırpılmamalı"
        );
    }

    #[test]
    fn sayfa_adresi_yakalanmiyor() {
        assert_eq!(indirilebilir_baglanti("https://haber.com/gundem/yazi"), None);
        assert_eq!(indirilebilir_baglanti("https://ornek.com/"), None);
        assert_eq!(indirilebilir_baglanti("https://ornek.com"), None);
    }

    #[test]
    fn duz_metin_ve_cok_satirli_icerik_yakalanmiyor() {
        assert_eq!(indirilebilir_baglanti("merhaba dünya"), None);
        assert_eq!(indirilebilir_baglanti("https://a.com/x.zip https://b.com/y.zip"), None);
        assert_eq!(indirilebilir_baglanti("https://a.com/x.zip\nhttps://b.com/y.zip"), None);
        assert_eq!(indirilebilir_baglanti(""), None);
        assert_eq!(indirilebilir_baglanti("   "), None);
    }

    #[test]
    fn baska_semalar_yakalanmiyor() {
        assert_eq!(indirilebilir_baglanti("ftp://ornek.com/dosya.zip"), None);
        assert_eq!(indirilebilir_baglanti("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(indirilebilir_baglanti(r"C:\indirmeler\film.mkv"), None);
    }

    #[test]
    fn asiri_uzun_icerik_yakalanmiyor() {
        let uzun = format!("https://ornek.com/{}.zip", "a".repeat(MAKUL_UZUNLUK));
        assert_eq!(indirilebilir_baglanti(&uzun), None);
    }

    #[test]
    fn tanimayan_uzanti_yakalanmiyor() {
        // Kategori tablosunda olmayan uzantı: bilinen bir dosya türü değil.
        assert_eq!(indirilebilir_baglanti("https://ornek.com/veri.xyz"), None);
    }
}
