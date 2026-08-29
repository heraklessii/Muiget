//! Dosya türüne göre kategori klasörleri.
//!
//! IDM'in en çok kullanılan davranışlarından biri: inen dosyayı uzantısına
//! göre `Video`, `Müzik`, `Belgeler`, `Arşivler`, `Programlar` alt klasörlerine
//! ayırmak. Kullanıcı indirme klasörünü elle düzenlemek zorunda kalmıyor.
//!
//! Eşleme **sabit ve gömülü**: kullanıcıya kural düzenleyicisi vermek, bu
//! aşamada çözdüğünden çok soru doğuruyordu (çakışan kurallar, sıralama,
//! büyük/küçük harf). Özellik ayarlardan tümüyle kapatılabiliyor ve varsayılan
//! kapalı — indirmenin nereye düştüğünü sessizce değiştirmek, kullanıcının
//! dosyasını kaybetmesi gibi hissettirirdi.

/// Kategori adı → o kategoriye giren uzantılar.
///
/// Sıra önemli: bir uzantı yalnızca ilk eşleştiği kategoriye giriyor. Şu an
/// çakışan uzantı yok ama listeye ekleme yapılırken bu kural geçerli.
const KATEGORILER: &[(&str, &[&str])] = &[
    (
        "Video",
        &[
            "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "3gp",
            "ogv",
        ],
    ),
    ("Müzik", &["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus", "aiff", "alac"]),
    (
        "Belgeler",
        &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "odt", "ods", "odp",
            "epub", "mobi", "azw3", "csv",
        ],
    ),
    ("Arşivler", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso", "cab", "tgz"]),
    ("Programlar", &["exe", "msi", "dmg", "pkg", "deb", "rpm", "apk", "appimage", "jar"]),
    ("Resimler", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "svg", "heic", "avif"]),
];

/// Dosya adına düşen kategori klasörü.
///
/// Tanınmayan uzantılar `None` dönüyor ve dosya indirme klasörünün kökünde
/// kalıyor. "Diğer" diye bir klasör açmak, kullanıcının aradığı dosyayı bir
/// klasör daha derine gömerdi.
pub fn folder_for(file_name: &str) -> Option<&'static str> {
    let uzanti = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    if uzanti.is_empty() {
        return None;
    }

    KATEGORILER
        .iter()
        .find(|(_, uzantilar)| uzantilar.contains(&uzanti.as_str()))
        .map(|(ad, _)| *ad)
}

/// Bütün kategori klasörü adları.
///
/// Oturumlar arası geri yükleme taraması bunları kullanıyor: kategori açıkken
/// yarım indirmeler kökte değil bu klasörlerde duruyor ve taranmasalardı liste
/// yeniden açılışta boş gelirdi (bkz. [`super::resume::scan_directory`]).
pub fn folder_names() -> impl Iterator<Item = &'static str> {
    KATEGORILER.iter().map(|(ad, _)| *ad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uzantiya_gore_klasor_seciliyor() {
        assert_eq!(folder_for("film.mkv"), Some("Video"));
        assert_eq!(folder_for("parca.MP3"), Some("Müzik"));
        assert_eq!(folder_for("kitap.epub"), Some("Belgeler"));
        assert_eq!(folder_for("arsiv.tar.gz"), Some("Arşivler"));
        assert_eq!(folder_for("kurulum.exe"), Some("Programlar"));
        assert_eq!(folder_for("ekran.png"), Some("Resimler"));
    }

    #[test]
    fn taninmayan_uzanti_kokte_kaliyor() {
        assert_eq!(folder_for("veri.xyzzy"), None);
        assert_eq!(folder_for("uzantisiz"), None);
        assert_eq!(folder_for("nokta."), None);
    }

    #[test]
    fn gizli_dosya_uzanti_sayilmiyor() {
        // `.gitignore` uzantısı olan bir dosya değil; gövdesi boş.
        // `rsplit_once` gövdeyi boş, uzantıyı "gitignore" verir — eşleşme yok,
        // yani sonuç yine kök. Testin amacı bunun kazara değişmemesi.
        assert_eq!(folder_for(".gitignore"), None);
    }

    #[test]
    fn klasor_adlari_benzersiz() {
        let adlar: Vec<_> = folder_names().collect();
        let mut sirali = adlar.clone();
        sirali.sort_unstable();
        sirali.dedup();
        assert_eq!(adlar.len(), sirali.len(), "kategori adları benzersiz olmalı");
    }
}
