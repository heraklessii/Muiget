//! HLS `AES-128` parça şifrelemesinin çözülmesi.
//!
//! ## Neden bu var, DRM neden yok
//!
//! HLS'in `METHOD=AES-128` kipinde anahtar, manifestin gösterdiği adresten
//! **herkese açık** olarak veriliyor: tarayıcıdaki oynatıcı da tam olarak bu
//! adresi çekip parçaları çözüyor. Burada atlatılan bir koruma yok; şifreleme
//! aktarım katmanında, anahtar teslimi serbest. Desteklenmemesi hâlinde
//! sıradan bir video sitesinin yarısı inmezdi.
//!
//! Buna karşılık `SAMPLE-AES` (FairPlay), Widevine ve PlayReady **bilinçli
//! olarak reddediliyor** — orada anahtar bir lisans sunucusundan cihaz
//! kimliğiyle alınıyor ve onu aşmak `CLAUDE.md`'deki kapsam sınırının dışına
//! çıkmak olurdu. Reddetme ayrıştırma anında yapılıyor (bkz. [`super::m3u8`],
//! [`super::mpd`]), yarım bir indirmeden sonra değil.
//!
//! ## Neden hazır crate
//!
//! `aes` + `cbc` (RustCrypto) saf Rust, sistem bağımlılığı getirmiyor ve proje
//! zaten aynı ailenin `sha2`/`md-5` crate'lerini kullanıyor. AES'i elle yazmak
//! bu dosyayı ~150 satır büyütür ve gözden geçirilmemiş bir blok şifre
//! bırakırdı.

use std::collections::HashMap;
use std::sync::Mutex;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use reqwest::Client;

use crate::download::{DownloadError, Result};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Anahtar uzunluğu — `AES-128` sabit.
pub const KEY_LEN: usize = 16;

/// `data`yı yerinde çözer ve PKCS#7 dolgusunu atar.
///
/// HLS'te **her parça** kendi başına tam bir CBC şifreli gövde: dolgu her
/// parçanın sonunda var, zincir parçalar arasında devam etmiyor.
pub fn decrypt_aes128_cbc(key: &[u8; KEY_LEN], iv: &[u8; 16], data: &mut Vec<u8>) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if data.len() % 16 != 0 {
        return Err(DownloadError::Manifest(format!(
            "şifreli parça 16'nın katı değil ({} byte); anahtar ya da aralık yanlış",
            data.len()
        )));
    }

    let cozucu = Aes128CbcDec::new(key.into(), iv.into());
    let uzunluk = cozucu
        .decrypt_padded_mut::<Pkcs7>(data.as_mut_slice())
        .map(|acik| acik.len())
        .map_err(|_| {
            DownloadError::Manifest(
                "parça çözülemedi: dolgu geçersiz (anahtar ya da IV yanlış olabilir)".into(),
            )
        })?;

    data.truncate(uzunluk);
    Ok(())
}

/// IV verilmemişse medya sırası numarasından türetilir (RFC 8216 §5.2):
/// 128 bitlik big-endian sayı.
pub fn iv_from_sequence(sequence: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// `0x1a2b…` ya da öneksiz onaltılık dizgeyi 16 byte'a çevirir.
pub fn parse_hex16(s: &str) -> Option<[u8; 16]> {
    let temiz = s.trim();
    let temiz = temiz
        .strip_prefix("0x")
        .or_else(|| temiz.strip_prefix("0X"))
        .unwrap_or(temiz);
    if temiz.len() != 32 || !temiz.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let mut out = [0u8; 16];
    for (i, hedef) in out.iter_mut().enumerate() {
        *hedef = u8::from_str_radix(&temiz[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// İndirilen anahtarları saklar.
///
/// Bir playlistte yüzlerce parça aynı `#EXT-X-KEY` satırını paylaşıyor; her
/// parça için anahtarı yeniden çekmek sunucuyu gereksiz yere N kez yormak
/// olurdu. Anahtar rotasyonlu yayınlarda (her N parçada bir yeni URI) önbellek
/// adrese göre tutulduğu için doğru anahtar yine kullanılıyor.
#[derive(Debug, Default)]
pub struct KeyStore {
    keys: Mutex<HashMap<String, [u8; KEY_LEN]>>,
    /// İndirme kapısı — aynı anda yalnızca bir anahtar isteği.
    ///
    /// Yalnızca haritayı kilitlemek yetmiyordu: parçalar paralel indiği için
    /// ilk N parça önbelleği aynı anda boş buluyor ve hepsi anahtarı ayrı ayrı
    /// çekiyordu (uçtan uca testte üç istek görüldü). Kapı, ilk istek bitene
    /// kadar diğerlerini bekletiyor; bekleyenler uyandığında önbelleği yeniden
    /// kontrol edip hazır anahtarı buluyor.
    ///
    /// `tokio::sync::Mutex`: kilit ağ isteği boyunca tutuluyor, yani bir
    /// `await` noktasını geçiyor. Anahtarlar 16 byte ve sayıları bir elin
    /// parmaklarını geçmediği için tüm anahtar isteklerini sıraya sokmanın
    /// bedeli yok.
    gate: tokio::sync::Mutex<()>,
}

impl KeyStore {
    pub fn new() -> Self {
        KeyStore::default()
    }

    /// Anahtarı önbellekten verir, yoksa indirir.
    pub async fn get(
        &self,
        client: &Client,
        uri: &str,
        headers: &[(String, String)],
    ) -> Result<[u8; KEY_LEN]> {
        if let Some(k) = self.onbellek(uri) {
            return Ok(k);
        }

        let _kapi = self.gate.lock().await;
        // Kapıda beklerken başka bir parça anahtarı indirmiş olabilir.
        if let Some(k) = self.onbellek(uri) {
            return Ok(k);
        }

        let mut istek = client.get(uri);
        for (ad, deger) in headers {
            istek = istek.header(ad, deger);
        }
        let yanit = istek.send().await?;
        if !yanit.status().is_success() {
            return Err(DownloadError::HttpStatus { status: yanit.status().as_u16() });
        }

        let govde = yanit.bytes().await?;
        if govde.len() != KEY_LEN {
            return Err(DownloadError::Manifest(format!(
                "şifreleme anahtarı {} byte geldi, {KEY_LEN} bekleniyordu",
                govde.len()
            )));
        }

        let mut anahtar = [0u8; KEY_LEN];
        anahtar.copy_from_slice(&govde);
        self.keys.lock().unwrap().insert(uri.to_string(), anahtar);
        Ok(anahtar)
    }

    /// Testler ve önceden bilinen anahtarlar için.
    pub fn insert(&self, uri: &str, key: [u8; KEY_LEN]) {
        self.keys.lock().unwrap().insert(uri.to_string(), key);
    }

    fn onbellek(&self, uri: &str) -> Option<[u8; KEY_LEN]> {
        self.keys.lock().unwrap().get(uri).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{block_padding::Pkcs7 as Dolgu, BlockEncryptMut};

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

    /// RFC 3602 §4, test case #1.
    const RFC_KEY: [u8; 16] = [
        0x06, 0xa9, 0x21, 0x40, 0x36, 0xb8, 0xa1, 0x5b, 0x51, 0x2e, 0x03, 0xd5, 0x34, 0x12, 0x00,
        0x06,
    ];
    const RFC_IV: [u8; 16] = [
        0x3d, 0xaf, 0xba, 0x42, 0x9d, 0x9e, 0xb4, 0x30, 0xb4, 0x22, 0xda, 0x80, 0x2c, 0x9f, 0xac,
        0x41,
    ];
    const RFC_ACIK: &[u8] = b"Single block msg";
    const RFC_SIFRELI: [u8; 16] = [
        0xe3, 0x53, 0x77, 0x9c, 0x10, 0x79, 0xae, 0xb8, 0x27, 0x08, 0x94, 0x2d, 0xbe, 0x77, 0x18,
        0x1a,
    ];

    fn sifrele(key: &[u8; 16], iv: &[u8; 16], acik: &[u8]) -> Vec<u8> {
        let mut tampon = vec![0u8; acik.len() + 16];
        let n = Aes128CbcEnc::new(key.into(), iv.into())
            .encrypt_padded_b2b_mut::<Dolgu>(acik, &mut tampon)
            .unwrap()
            .len();
        tampon.truncate(n);
        tampon
    }

    #[test]
    fn bilinen_vektore_karsi_dogru_sifreliyor() {
        // PKCS#7 tam blokluk mesaja ikinci bir dolgu bloğu ekliyor; ilk blok
        // dolgudan etkilenmediği için RFC vektörüyle birebir tutmalı.
        let sifreli = sifrele(&RFC_KEY, &RFC_IV, RFC_ACIK);
        assert_eq!(sifreli.len(), 32);
        assert_eq!(&sifreli[..16], &RFC_SIFRELI);
    }

    #[test]
    fn bilinen_vektor_geri_cozuluyor() {
        let mut veri = sifrele(&RFC_KEY, &RFC_IV, RFC_ACIK);
        decrypt_aes128_cbc(&RFC_KEY, &RFC_IV, &mut veri).unwrap();
        assert_eq!(veri, RFC_ACIK);
    }

    #[test]
    fn parca_boyu_veri_gidip_geliyor() {
        let acik: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let mut veri = sifrele(&RFC_KEY, &RFC_IV, &acik);
        assert_ne!(veri, acik);
        decrypt_aes128_cbc(&RFC_KEY, &RFC_IV, &mut veri).unwrap();
        assert_eq!(veri, acik);
    }

    #[test]
    fn yanlis_anahtar_hata_veriyor() {
        let mut veri = sifrele(&RFC_KEY, &RFC_IV, b"gizli veri");
        let yanlis = [0u8; 16];
        // Dolgu doğrulaması yanlış anahtarı neredeyse her zaman yakalıyor.
        assert!(decrypt_aes128_cbc(&yanlis, &RFC_IV, &mut veri).is_err());
    }

    #[test]
    fn blok_hizasiz_veri_reddediliyor() {
        let mut veri = vec![1u8; 17];
        let hata = decrypt_aes128_cbc(&RFC_KEY, &RFC_IV, &mut veri).unwrap_err();
        assert!(hata.to_string().contains("16"));
    }

    #[test]
    fn bos_veri_sorun_cikarmiyor() {
        let mut veri = Vec::new();
        decrypt_aes128_cbc(&RFC_KEY, &RFC_IV, &mut veri).unwrap();
        assert!(veri.is_empty());
    }

    #[test]
    fn iv_sira_numarasindan_turetiliyor() {
        assert_eq!(iv_from_sequence(0), [0u8; 16]);
        let iv = iv_from_sequence(1);
        assert_eq!(iv[15], 1);
        assert!(iv[..15].iter().all(|b| *b == 0));
        let iv = iv_from_sequence(0x0102030405060708);
        assert_eq!(&iv[8..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn onaltilik_iv_cozuluyor() {
        assert_eq!(parse_hex16("0x00000000000000000000000000000001").unwrap()[15], 1);
        assert_eq!(parse_hex16("3dafba429d9eb430b422da802c9fac41").unwrap(), RFC_IV);
        assert_eq!(parse_hex16("0X3DAFBA429D9EB430B422DA802C9FAC41").unwrap(), RFC_IV);
        assert!(parse_hex16("0x12").is_none());
        assert!(parse_hex16("0xzz000000000000000000000000000001").is_none());
    }

    #[tokio::test]
    async fn onbellekteki_anahtar_ag_istegi_yapmiyor() {
        let depo = KeyStore::new();
        depo.insert("https://x/key", RFC_KEY);
        // Ulaşılamaz bir istemciyle çağrılıyor: önbellek çalışmasaydı hata dönerdi.
        let client = Client::builder().build().unwrap();
        let k = depo.get(&client, "https://x/key", &[]).await.unwrap();
        assert_eq!(k, RFC_KEY);
    }
}
