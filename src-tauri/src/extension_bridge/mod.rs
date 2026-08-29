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

/// Native messaging host'unu tarayıcıya tanıtır.
///
/// Manifest dosyasını yazar; Windows'ta ayrıca kullanıcı kapsamındaki
/// (`HKCU`) registry anahtarını oluşturur — Chrome manifesti orada arıyor.
/// Edge de aynı protokolü kullandığı için ona da yazılıyor.
///
/// Yalnızca kullanıcı ayarlardan açıkça istediğinde çağrılıyor: bir indirme
/// yöneticisinin kurulumda sessizce tarayıcıya kendini tanıtması doğru değil.
pub fn install_host(
    config_dir: &std::path::Path,
    executable: &std::path::Path,
    allowed_extension_ids: &[String],
) -> std::io::Result<PathBuf> {
    let manifest = native_host::manifest_json(executable, allowed_extension_ids);
    let path = native_host::manifest_path(config_dir);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, manifest)?;

    #[cfg(target_os = "windows")]
    {
        for kok in [
            r"HKCU\Software\Google\Chrome\NativeMessagingHosts",
            r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts",
        ] {
            let anahtar = format!(r"{kok}\{}", native_host::HOST_NAME);
            // Hata yutuluyor: Edge kurulu olmayabilir ve bu bir başarısızlık
            // değil. Chrome tarafı da yazılamazsa kullanıcı manifest yolunu
            // elle tanıtabilir; dönen yol tam da bunun için veriliyor.
            let _ = std::process::Command::new("reg")
                .args(["add", &anahtar, "/ve", "/t", "REG_SZ", "/d"])
                .arg(path.as_os_str())
                .arg("/f")
                .output();
        }
    }

    Ok(path)
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
