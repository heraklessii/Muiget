//! Chrome uzantısı köprüsü.
//!
//! İki parçası var:
//! * [`native_host`] — Chrome ile konuşan stdio protokolü ve köprü süreci
//! * Bu dosya — köprüden gelen isteğin çalışan uygulamada işlenmesi

pub mod native_host;

use std::path::PathBuf;

use crate::download::manager::{DownloadManager, DownloadOptions};
use native_host::DownloadRequest;

/// Komut satırı argümanlarından indirme isteği çıkarır.
///
/// `muiget --add <base64>` biçimi köprü süreci tarafından kullanılıyor. Tek
/// örnek (single instance) eklentisi bu argümanları zaten açık olan pencereye
/// taşıdığı için aynı ayrıştırma hem ilk hem sonraki çalıştırmalarda geçerli.
pub fn parse_add_argument(args: &[String]) -> Option<DownloadRequest> {
    let sira = args.iter().position(|a| a == native_host::ADD_FLAG)?;
    let yuk = args.get(sira + 1)?;
    native_host::decode_payload(yuk)
}

/// Köprüden gelen isteği indirme motoruna verir.
pub fn handle_request(
    manager: &DownloadManager,
    request: DownloadRequest,
    directory: PathBuf,
) -> crate::download::Result<String> {
    // Şema kontrolü köprüde de yapılıyor; burada tekrar edilmesi bilinçli.
    // Bu fonksiyon ileride başka çağıranlar da kazanacak (ör. komut satırı) ve
    // güvenlik kontrolünün tek bir çağrı yolunda kalmasına güvenmek istemiyoruz.
    if !request.is_supported() {
        return Err(crate::download::DownloadError::InvalidUrl(request.url));
    }

    let options = DownloadOptions {
        headers: request.to_headers(),
        file_name: request.file_name.clone(),
    };

    manager.start_with(request.url, directory, options)
}

/// Verilen kimlikleri tarayıcı ailesine göre ayırır (karar #31).
///
/// Kullanıcı tek bir kutuya yazıyor; hangi tarayıcıya ait olduğu kimliğin
/// **biçiminden** anlaşılıyor: Chrome kimliği 32 harflik `a`–`p` dizisi,
/// Firefox kimliği e-posta ya da `{GUID}` biçiminde. Ayrı kutu istemek,
/// kullanıcıya bizim zaten bildiğimiz bir şeyi sordurmak olurdu.
///
/// Firefox tarafına ayrıca [`native_host::FIREFOX_EXTENSION_ID`] ekleniyor:
/// o kimliği paketi üretirken biz yazıyoruz, kullanıcının elinde bir karşılığı
/// yok.
fn kimlikleri_ayir(ids: &[String]) -> (Vec<String>, Vec<String>) {
    let mut chromium = Vec::new();
    let mut firefox = vec![native_host::FIREFOX_EXTENSION_ID.to_string()];

    for id in ids {
        let id = id.trim();
        // Geçersiz kimlik atılıyor. Ayarlar `normalize()` içinde zaten süzüyor;
        // burada tekrar edilmesi bilinçli — komut doğrudan da çağrılabilir ve
        // biçimsiz bir değer manifesti kullanılamaz hâle getirirdi.
        if !crate::settings::gecerli_uzanti_kimligi(id) {
            continue;
        }
        if firefox.iter().any(|v| v == id) || chromium.iter().any(|v| v == id) {
            continue;
        }
        if crate::settings::firefox_uzanti_kimligi(id) {
            firefox.push(id.to_string());
        } else {
            chromium.push(id.to_string());
        }
    }

    (chromium, firefox)
}

/// Native messaging host'unu tarayıcılara tanıtır.
///
/// Manifest dosyalarını yazar; Windows'ta ayrıca kullanıcı kapsamındaki
/// (`HKCU`) registry anahtarlarını oluşturur — tarayıcılar manifesti orada
/// arıyor. Chrome ve Edge aynı manifesti paylaşıyor (ikisi de Chromium),
/// Firefox'unki ayrı: izin listesinin alan adı ve registry kökü farklı.
///
/// Yalnızca kullanıcı ayarlardan açıkça istediğinde çağrılıyor: bir indirme
/// yöneticisinin kurulumda sessizce tarayıcıya kendini tanıtması doğru değil.
///
/// Dönen değer yazılan manifestlerin yolları. Kurulu olmayan tarayıcı için de
/// yazılıyor: dosya ve `HKCU` anahtarı o tarayıcı yokken kimseye görünmüyor,
/// sonradan kurulduğunda ise köprü hazır oluyor.
pub fn install_host(
    config_dir: &std::path::Path,
    executable: &std::path::Path,
    allowed_extension_ids: &[String],
) -> std::io::Result<Vec<PathBuf>> {
    let yazilanlar = write_manifests(config_dir, executable, allowed_extension_ids)?;

    #[cfg(target_os = "windows")]
    for (browser, path) in &yazilanlar {
        for kok in registry_roots(*browser) {
            let anahtar = format!(r"{kok}\{}", native_host::HOST_NAME);
            // Hata yutuluyor: Edge ya da Firefox kurulu olmayabilir ve bu bir
            // başarısızlık değil. Chrome tarafı da yazılamazsa kullanıcı
            // manifest yolunu elle tanıtabilir; dönen yollar tam da bunun için.
            let _ = std::process::Command::new("reg")
                .args(["add", &anahtar, "/ve", "/t", "REG_SZ", "/d"])
                .arg(path.as_os_str())
                .arg("/f")
                .output();
        }
    }

    Ok(yazilanlar.into_iter().map(|(_, yol)| yol).collect())
}

/// Manifest dosyalarını yazar — registry'ye dokunmadan.
///
/// Registry yazımından ayrı tutuldu ki testler dosya içeriğini gerçek
/// `install_host` yolundan doğrulayabilsin: bir birim testinin `HKCU`
/// altındaki gerçek köprü kaydını geçici bir dosyayla ezmesi kabul edilemez.
fn write_manifests(
    config_dir: &std::path::Path,
    executable: &std::path::Path,
    allowed_extension_ids: &[String],
) -> std::io::Result<Vec<(native_host::Browser, PathBuf)>> {
    let (chromium_ids, firefox_ids) = kimlikleri_ayir(allowed_extension_ids);
    let mut yollar = Vec::new();

    for (browser, ids) in [
        (native_host::Browser::Chromium, chromium_ids),
        (native_host::Browser::Firefox, firefox_ids),
    ] {
        let manifest = native_host::manifest_json(browser, executable, &ids);
        let path = native_host::manifest_path(browser, config_dir);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, manifest)?;
        yollar.push((browser, path));
    }

    Ok(yollar)
}

/// Windows'ta manifest yolunun yazılacağı `HKCU` anahtarları.
///
/// Chrome ve Edge aynı manifesti paylaşıyor (ikisi de Chromium ve aynı
/// `chrome-extension://` kaynağını veriyor); Firefox'un kökü de manifesti de
/// ayrı.
#[cfg(target_os = "windows")]
fn registry_roots(browser: native_host::Browser) -> &'static [&'static str] {
    match browser {
        native_host::Browser::Chromium => &[
            r"HKCU\Software\Google\Chrome\NativeMessagingHosts",
            r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts",
        ],
        native_host::Browser::Firefox => &[r"HKCU\Software\Mozilla\NativeMessagingHosts"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_host::encode_payload;

    #[test]
    fn add_argumani_ayristiriliyor() {
        let istek = DownloadRequest {
            url: "https://ornek.com/a.zip".into(),
            file_name: Some("a.zip".into()),
            ..Default::default()
        };

        let args = vec![
            "muiget.exe".to_string(),
            native_host::ADD_FLAG.to_string(),
            encode_payload(&istek),
        ];

        assert_eq!(parse_add_argument(&args), Some(istek));
    }

    #[test]
    fn add_argumani_yoksa_none() {
        let args = vec!["muiget.exe".to_string()];
        assert_eq!(parse_add_argument(&args), None);
    }

    #[test]
    fn add_argumani_yuksuzse_none() {
        let args = vec!["muiget.exe".to_string(), native_host::ADD_FLAG.to_string()];
        assert_eq!(parse_add_argument(&args), None);
    }

    #[test]
    fn bozuk_yuk_none_donuyor() {
        let args = vec![
            "muiget.exe".to_string(),
            native_host::ADD_FLAG.to_string(),
            "!!!bozuk!!!".to_string(),
        ];
        assert_eq!(parse_add_argument(&args), None);
    }

    #[test]
    fn kimlikler_tarayiciya_gore_ayriliyor() {
        let (chromium, firefox) = kimlikleri_ayir(&[
            "abcdefghijklmnopabcdefghijklmnop".to_string(),
            "baska@ornek.com".to_string(),
            "  ".to_string(),
        ]);

        assert_eq!(chromium, vec!["abcdefghijklmnopabcdefghijklmnop".to_string()]);
        // Kendi kimliğimiz kullanıcı yazmadan listede olmalı.
        assert_eq!(
            firefox,
            vec![
                native_host::FIREFOX_EXTENSION_ID.to_string(),
                "baska@ornek.com".to_string()
            ]
        );
    }

    #[test]
    fn gecersiz_kimlik_manifeste_girmiyor() {
        let (chromium, firefox) = kimlikleri_ayir(&[
            "kisa".to_string(),
            "aaaa/\", \"allowed_origins\": [\"*\"]".to_string(),
        ]);

        assert!(chromium.is_empty());
        assert_eq!(firefox, vec![native_host::FIREFOX_EXTENSION_ID.to_string()]);
    }

    #[test]
    fn kendi_firefox_kimligimiz_iki_kez_yazilmiyor() {
        let (_, firefox) = kimlikleri_ayir(&[native_host::FIREFOX_EXTENSION_ID.to_string()]);
        assert_eq!(firefox.len(), 1);
    }

    // Yalnızca Windows: diğer platformlarda manifest yolu tarayıcının sabit
    // dizini (`~/.mozilla/native-messaging-hosts` gibi) ve bir testin oraya
    // dosya yazması makinedeki gerçek kurulumu etkilerdi. Manifestin içeriği
    // platformdan bağımsız olarak `native_host` testlerinde sınanıyor.
    #[cfg(target_os = "windows")]
    #[test]
    fn iki_manifest_de_yaziliyor() {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::path::Path::new("C:\\Program Files\\Muiget\\muiget.exe");

        // Registry'ye dokunmayan iç fonksiyon: bir test, makinedeki gerçek
        // köprü kaydını geçici bir yolla ezmemeli.
        let yollar =
            write_manifests(dir.path(), exe, &["abcdefghijklmnopabcdefghijklmnop".to_string()])
                .unwrap();

        assert_eq!(yollar.len(), 2, "Chromium ve Firefox manifestleri ayrı dosya");

        let chromium: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&yollar[0].1).unwrap()).unwrap();
        let firefox: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&yollar[1].1).unwrap()).unwrap();

        assert_eq!(chromium["allowed_origins"][0], "chrome-extension://abcdefghijklmnopabcdefghijklmnop/");
        assert_eq!(firefox["allowed_extensions"][0], native_host::FIREFOX_EXTENSION_ID);
        // İkisi de aynı çalıştırılabilir dosyayı göstermeli.
        assert_eq!(chromium["path"], firefox["path"]);
    }

    // Yönetici kurulumu çalışma zamanı handle'ı istiyor (bkz. `Inner::runtime`).
    #[tokio::test]
    async fn desteklenmeyen_sema_motora_ulasmiyor() {
        let manager =
            DownloadManager::new(crate::download::manager::ManagerConfig::default()).unwrap();
        let istek = DownloadRequest { url: "file:///etc/passwd".into(), ..Default::default() };

        let sonuc = handle_request(&manager, istek, PathBuf::from("."));
        assert!(matches!(
            sonuc,
            Err(crate::download::DownloadError::InvalidUrl(_))
        ));
        assert!(manager.list().is_empty(), "reddedilen istek listeye girmemeli");
    }
}
