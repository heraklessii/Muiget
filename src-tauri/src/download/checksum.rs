//! İnen dosyanın özetini (checksum) hesaplar.
//!
//! Neden otomatik değil de istek üzerine (karar #21): 8 GB'lık bir dosyayı
//! hash'lemek diski baştan sona bir kez daha okumak demek. IDM de bunu her
//! indirmede yapmıyor. Kullanıcı doğrulamak istediğinde — sitede yayımlanmış
//! bir SHA-256 ile karşılaştırmak gibi — sağ tık menüsünden tetikliyor.
//!
//! MD5 çakışmaya açık olduğu için imza doğrulamada kullanılmamalı; yine de
//! duruyor, çünkü indirme sitelerinin çoğu hâlâ MD5 yayımlıyor ve elimizdeki
//! tek karşılaştırma değeri o oluyor.

use std::path::Path;

use md5::Digest as _;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use super::{DownloadError, Result};

/// Tek seferde okunan blok. 1 MB, sistem çağrısı sayısı ile bellek arasında
/// makul bir orta yol; daha büyüğü ölçülebilir bir kazanç vermiyor.
const BLOK: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha256,
    Md5,
}

impl Algorithm {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "sha256" => Ok(Algorithm::Sha256),
            "md5" => Ok(Algorithm::Md5),
            other => Err(DownloadError::Other(format!("bilinmeyen özet algoritması: {other}"))),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Md5 => "MD5",
        }
    }
}

/// Dosyanın özetini küçük harfli hex olarak döner.
///
/// Dosya akış hâlinde okunuyor: tamamını belleğe almak, indirme yöneticisinin
/// ilgilendiği dosya boyutlarında sürdürülebilir değil.
pub async fn compute(path: &Path, algorithm: Algorithm) -> Result<String> {
    let mut dosya = tokio::fs::File::open(path).await?;
    let mut tampon = vec![0u8; BLOK];

    let mut sha = sha2::Sha256::new();
    let mut md5 = md5::Md5::new();

    loop {
        let okunan = dosya.read(&mut tampon).await?;
        if okunan == 0 {
            break;
        }
        match algorithm {
            Algorithm::Sha256 => sha.update(&tampon[..okunan]),
            Algorithm::Md5 => md5.update(&tampon[..okunan]),
        }
        // Büyük dosyada bu döngü tek başına bir çekirdeği doldurabiliyor;
        // sıra bırakmak arayüzün donmasını engelliyor.
        tokio::task::yield_now().await;
    }

    Ok(match algorithm {
        Algorithm::Sha256 => hex(&sha.finalize()),
        Algorithm::Md5 => hex(&md5.finalize()),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn yaz(icerik: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let yol = dir.path().join("dosya.bin");
        tokio::fs::write(&yol, icerik).await.unwrap();
        (dir, yol)
    }

    #[tokio::test]
    async fn sha256_bilinen_vektorle_ayni() {
        let (_dir, yol) = yaz(b"abc").await;
        assert_eq!(
            compute(&yol, Algorithm::Sha256).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn md5_bilinen_vektorle_ayni() {
        let (_dir, yol) = yaz(b"abc").await;
        assert_eq!(
            compute(&yol, Algorithm::Md5).await.unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[tokio::test]
    async fn bos_dosya_bos_ozetini_veriyor() {
        let (_dir, yol) = yaz(b"").await;
        assert_eq!(
            compute(&yol, Algorithm::Sha256).await.unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn blok_sinirini_asan_dosya_dogru_hesaplaniyor() {
        // Tek okumaya sığmayan boyut: döngünün birikimli olduğunu doğrular.
        let veri = vec![7u8; BLOK + 12345];
        let (_dir, yol) = yaz(&veri).await;

        let mut beklenen = sha2::Sha256::new();
        beklenen.update(&veri);

        assert_eq!(compute(&yol, Algorithm::Sha256).await.unwrap(), hex(&beklenen.finalize()));
    }

    #[tokio::test]
    async fn olmayan_dosya_hata_veriyor() {
        let dir = tempfile::tempdir().unwrap();
        assert!(compute(&dir.path().join("yok.bin"), Algorithm::Sha256).await.is_err());
    }

    #[test]
    fn algoritma_adi_esnek_okunuyor() {
        assert_eq!(Algorithm::parse("sha256").unwrap(), Algorithm::Sha256);
        assert_eq!(Algorithm::parse(" SHA-256 ").unwrap(), Algorithm::Sha256);
        assert_eq!(Algorithm::parse("MD5").unwrap(), Algorithm::Md5);
        assert!(Algorithm::parse("sha1").is_err());
    }
}
