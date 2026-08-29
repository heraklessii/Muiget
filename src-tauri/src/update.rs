//! Yeni sürüm kontrolü.
//!
//! Neden imzalı otomatik güncelleyici değil (karar #23): Tauri'nin updater
//! eklentisi bir imza anahtar çifti ve her yayında imzalanan bir `latest.json`
//! istiyor. Anahtar İlker'in elinde olmadan yarım kurulan bir updater,
//! uygulamayı hiç güncellenemez hâle getirirdi. Buradaki çözüm daha küçük:
//! GitHub'ın yayın listesine bakıp "yeni sürüm var" demek, indirmeyi
//! kullanıcının tarayıcısına bırakmak. İmzalı otomatik güncelleme geldiğinde
//! bu modül onun yerini bırakır.
//!
//! **Bu, uygulamanın kendiliğinden yaptığı tek dış istek.** Gönderilen tek şey
//! istek başlıkları; hiçbir kullanıcı verisi taşınmıyor ve ayarlardan
//! kapatılabiliyor (`checkUpdates`).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::download::{DownloadError, Result};

/// `https://github.com/sahip/depo` → `sahip/depo`.
fn repo_slug() -> Option<&'static str> {
    env!("CARGO_PKG_REPOSITORY")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .strip_prefix("https://github.com/")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    /// Yayının GitHub sayfası. Arayüz bunu tarayıcıda açıyor.
    pub url: String,
    pub available: bool,
}

/// GitHub'ın son yayınına bakar.
///
/// Ağ hatası da, beklenmeyen yanıt da hata olarak dönüyor: arayüz bunu sessizce
/// yutuyor. Sürüm kontrolünün başarısız olması kullanıcıyı ilgilendiren bir
/// olay değil.
pub async fn check(client: &reqwest::Client, current: &str) -> Result<UpdateInfo> {
    let slug = repo_slug()
        .ok_or_else(|| DownloadError::Other("depo adresi GitHub değil".into()))?;

    // **`/releases/latest` değil.** O uç nokta ön sürümleri atlıyor ve
    // Muiget'in bütün yayınları `prerelease: true` — dolayısıyla 404 dönüyordu
    // ve sürüm kontrolü her seferinde sessizce başarısız oluyordu (8. oturumun
    // `--native-host` hatasıyla aynı sınıf: derleniyor ama çalışmıyor).
    let adres = format!("https://api.github.com/repos/{slug}/releases?per_page=10");
    let yanit = client
        .get(&adres)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !yanit.status().is_success() {
        return Err(DownloadError::HttpStatus { status: yanit.status().as_u16() });
    }

    // `reqwest`'in `json` özelliği bilinçli olarak kapalı (bir bağımlılık
    // daha getiriyor); yanıt metin olarak alınıp ayrıştırılıyor.
    let ham = yanit.text().await?;
    let govde: serde_json::Value = serde_json::from_str(&ham)
        .map_err(|e| DownloadError::Other(format!("yayın bilgisi ayrıştırılamadı: {e}")))?;
    let (etiket, sayfa) = en_yeni_yayin(&govde)
        .ok_or_else(|| DownloadError::Other("depoda yayın bulunamadı".into()))?;

    log::info!("sürüm kontrolü: kurulu {current}, depodaki son yayın {etiket}");

    Ok(UpdateInfo {
        current: current.to_string(),
        latest: etiket.trim_start_matches('v').to_string(),
        url: if sayfa.is_empty() {
            format!("https://github.com/{slug}/releases")
        } else {
            sayfa
        },
        available: compare(&etiket, current) == Ordering::Greater,
    })
}

/// Yayın listesinden en yüksek sürümü seçer: `(etiket, sayfa adresi)`.
///
/// GitHub listeyi oluşturulma tarihine göre sıralıyor, sürüm numarasına göre
/// değil; eski bir dala atılan yama yayını başa geçebiliyor. Bu yüzden sıraya
/// güvenilmiyor, karşılaştırma [`compare`] ile yapılıyor.
///
/// Taslaklar atlanıyor (yayımlanmamış), ön sürümler **atlanmıyor**: projenin
/// bugüne kadarki bütün yayınları ön sürüm ve onları elemek özelliği tümüyle
/// işlevsiz bırakırdı.
fn en_yeni_yayin(govde: &serde_json::Value) -> Option<(String, String)> {
    govde
        .as_array()?
        .iter()
        .filter(|y| !y.get("draft").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter_map(|y| {
            let etiket = y.get("tag_name")?.as_str()?.to_string();
            let sayfa = y.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some((etiket, sayfa))
        })
        .max_by(|a, b| compare(&a.0, &b.0))
}

/// İki sürüm dizesini karşılaştırır (`v0.2.0` ile `0.10.0` aynı biçimde).
///
/// Sözlük sırası kullanılamaz: `"0.10.0" < "0.9.0"` çıkardı. Parçalar sayı
/// olarak karşılaştırılıyor. Tire sonrası ek (`0.2.0-rc1`) **ön sürüm** sayılıp
/// aynı numaralı kararlı sürümün altına konuyor — yoksa `rc` çıkan bir yayın,
/// kararlı sürümü kullanan herkese "güncelleme var" derdi.
pub fn compare(a: &str, b: &str) -> Ordering {
    let (a_sayilar, a_on) = ayristir(a);
    let (b_sayilar, b_on) = ayristir(b);

    let uzunluk = a_sayilar.len().max(b_sayilar.len());
    for i in 0..uzunluk {
        let x = a_sayilar.get(i).copied().unwrap_or(0);
        let y = b_sayilar.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            farkli => return farkli,
        }
    }

    // Sayılar eşit: ön sürüm kararlı sürümün altında.
    match (a_on, b_on) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn ayristir(raw: &str) -> (Vec<u64>, bool) {
    let temiz = raw.trim().trim_start_matches(['v', 'V']);
    let (sayilar, on_surum) = match temiz.split_once('-') {
        Some((s, _)) => (s, true),
        None => (temiz, false),
    };

    let parcalar = sayilar
        .split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0))
        .collect();

    (parcalar, on_surum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surum_karsilastirmasi_sayisal() {
        assert_eq!(compare("0.2.0", "0.1.2"), Ordering::Greater);
        assert_eq!(compare("0.1.2", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.1.2", "0.1.2"), Ordering::Equal);
        // Sözlük sırası burada yanılırdı.
        assert_eq!(compare("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare("1.0.0", "0.99.99"), Ordering::Greater);
    }

    #[test]
    fn v_oneki_onemsiz() {
        assert_eq!(compare("v0.2.0", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("V1.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn eksik_parca_sifir_sayiliyor() {
        assert_eq!(compare("0.2", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("0.2.1", "0.2"), Ordering::Greater);
    }

    #[test]
    fn on_surum_kararlinin_altinda() {
        assert_eq!(compare("0.2.0-rc1", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.2.0", "0.2.0-rc1"), Ordering::Greater);
        assert_eq!(compare("0.2.0-rc1", "0.1.9"), Ordering::Greater);
    }

    #[test]
    fn bozuk_surum_uygulamayi_dusurmuyor() {
        // Elle atılmış anlamsız bir etiket "daha yeni" sayılmamalı.
        assert_eq!(compare("deneme", "0.1.2"), Ordering::Less);
        assert_eq!(compare("", "0.0.0"), Ordering::Equal);
    }

    #[test]
    fn depo_adresi_slug_veriyor() {
        assert_eq!(repo_slug(), Some("heraklessii/Muiget"));
    }

    /// GitHub'ın gerçek yanıtından kısaltılmış örnek (2026-08-30 itibarıyla
    /// deponun durumu): üç yayın da ön sürüm.
    fn ornek_liste() -> serde_json::Value {
        serde_json::json!([
            {"tag_name": "v0.1.1", "prerelease": true, "draft": false,
             "html_url": "https://github.com/heraklessii/Muiget/releases/tag/v0.1.1"},
            {"tag_name": "v0.1.2", "prerelease": true, "draft": false,
             "html_url": "https://github.com/heraklessii/Muiget/releases/tag/v0.1.2"},
            {"tag_name": "v0.1.0", "prerelease": true, "draft": false,
             "html_url": "https://github.com/heraklessii/Muiget/releases/tag/v0.1.0"}
        ])
    }

    #[test]
    fn on_surumler_de_sayiliyor() {
        // `/releases/latest` bu depoda 404 veriyor; liste ucu kullanılıyor.
        let (etiket, sayfa) = en_yeni_yayin(&ornek_liste()).unwrap();
        assert_eq!(etiket, "v0.1.2");
        assert!(sayfa.ends_with("/v0.1.2"));
    }

    #[test]
    fn liste_sirasina_guvenilmiyor() {
        // Örnekte v0.1.2 ortada duruyor: ilk öğe alınsaydı v0.1.1 çıkardı.
        assert_eq!(en_yeni_yayin(&ornek_liste()).unwrap().0, "v0.1.2");
    }

    #[test]
    fn taslak_yayin_atlaniyor() {
        let liste = serde_json::json!([
            {"tag_name": "v0.9.0", "draft": true, "html_url": "x"},
            {"tag_name": "v0.2.0", "draft": false, "html_url": "y"}
        ]);
        assert_eq!(en_yeni_yayin(&liste).unwrap().0, "v0.2.0");
    }

    #[test]
    fn bos_liste_none_veriyor() {
        assert_eq!(en_yeni_yayin(&serde_json::json!([])), None);
        // Beklenmeyen biçim (nesne, dizi değil) çökmemeli.
        assert_eq!(en_yeni_yayin(&serde_json::json!({"message": "Not Found"})), None);
    }
}
