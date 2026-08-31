//! WebVTT parçalarını tek bir altyazı dosyasında birleştirme.
//!
//! ## Neden uç uca eklemek yetmiyor
//!
//! Video parçalarında olduğu gibi altyazı parçalarını da art arda yazmak
//! **geçersiz** bir dosya veriyor: her HLS altyazı parçası kendi başına tam bir
//! WebVTT belgesi, yani her birinin başında `WEBVTT` satırı var. Oynatıcı ilk
//! `WEBVTT`ten sonrasını okuyunca ikinci başlığı bir cue sanıyor ve dosyayı
//! ya orada kesiyor ya da tamamen reddediyor.
//!
//! ## `X-TIMESTAMP-MAP`
//!
//! İkinci sorun zamanlama. HLS altyazısı MPEG-TS zaman eksenine bağlanıyor:
//!
//! ```text
//! X-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:900000
//! ```
//!
//! "Bu dosyadaki `LOCAL` anı, medyanın `MPEGTS/90000` saniyesine denk geliyor"
//! demek. Sağlayıcıların çoğu her parçada aynı haritayı yazıp cue'ları mutlak
//! veriyor, bazıları ise her parçada sıfırdan başlıyor. İkisini de doğru
//! çevirmenin tek yolu haritayı okumak.
//!
//! Mutlak MPEG-TS anına değil, **ilk parçanın** offsetine göre hizalıyoruz:
//! MPEG-TS akışları tipik olarak 900000'de (10 sn) başlıyor ve o offseti
//! olduğu gibi bırakmak, ffmpeg `.mp4`e çevirdiğinde (ki orada zaman ekseni
//! sıfırlanıyor) altyazıyı 10 saniye kaydırırdı. Farkı almak iki çıktıda da
//! doğru sonuç veriyor.
//!
//! ## Yinelenen cue'lar
//!
//! Parça sınırını aşan bir cue her iki parçada da yazılıyor — oynatıcı hangi
//! parçadan başlarsa başlasın altyazıyı görsün diye. Birleştirilmiş dosyada bu
//! aynı satırın iki kez görünmesi demek; aynı (başlangıç, bitiş, metin) üçlüsü
//! bir kez yazılıyor.

use std::collections::HashSet;

/// Tek bir altyazı cue'su.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    /// Zaman satırının sonundaki yerleşim ayarları (`align:start position:10%`).
    pub settings: String,
    pub text: String,
}

/// Bir WebVTT parçasından çıkanlar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fragment {
    /// `X-TIMESTAMP-MAP`ten türeyen saniye cinsinden offset. Harita yoksa 0.
    pub offset: f64,
    /// Haritayı gerçekten gördük mü? Görmediysek offset karşılaştırması
    /// yapılmıyor (bkz. [`merge`]).
    pub has_map: bool,
    pub cues: Vec<Cue>,
    /// `STYLE` ve `REGION` blokları, geldikleri gibi.
    pub blocks: Vec<String>,
}

/// `00:01:02.345` ya da `01:02.345` → saniye.
///
/// WebVTT saat alanını isteğe bağlı bırakıyor ve pratikte iki biçim de
/// geliyor; birini desteklemeyen ayrıştırıcı bazı sağlayıcılarda tek cue bile
/// okuyamıyor.
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (tam, kesir) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };

    let mut toplam = 0f64;
    let parcalar: Vec<&str> = tam.split(':').collect();
    if parcalar.is_empty() || parcalar.len() > 3 {
        return None;
    }
    for p in &parcalar {
        let n: f64 = p.trim().parse().ok()?;
        if n < 0.0 {
            return None;
        }
        toplam = toplam * 60.0 + n;
    }

    if !kesir.is_empty() {
        if !kesir.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: f64 = kesir.parse().ok()?;
        toplam += n / 10f64.powi(kesir.len() as i32);
    }
    Some(toplam)
}

/// Saniye → `00:01:02.345`. Saat alanı her zaman yazılıyor; bazı oynatıcılar
/// kısa biçimi kabul etmiyor.
pub fn format_timestamp(saniye: f64) -> String {
    let s = if saniye.is_finite() && saniye > 0.0 { saniye } else { 0.0 };
    // Milisaniyeye yuvarla, sonra parçala: önce parçalayıp sonra yuvarlamak
    // 59.9996 saniyede "00:00:60.000" üretirdi.
    let toplam_ms = (s * 1000.0).round() as u64;
    let ms = toplam_ms % 1000;
    let toplam_sn = toplam_ms / 1000;
    let sn = toplam_sn % 60;
    let dk = (toplam_sn / 60) % 60;
    let sa = toplam_sn / 3600;
    format!("{sa:02}:{dk:02}:{sn:02}.{ms:03}")
}

/// `X-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:900000` → offset (saniye).
///
/// MPEG-TS saati 90 kHz. Sarma (2^33) yok sayılıyor: VOD altyazılarında
/// pratikte görülmüyor ve yanlış bir düzeltme, doğru bir kaymadan kötü olurdu.
fn timestamp_map(satir: &str) -> Option<f64> {
    let govde = satir.split_once('=')?.1;
    let mut local = 0f64;
    let mut mpegts = 0f64;
    let mut gorulen = false;

    for alan in govde.split(',') {
        let (ad, deger) = alan.split_once(':')?;
        match ad.trim().to_ascii_uppercase().as_str() {
            "LOCAL" => {
                local = parse_timestamp(deger)?;
                gorulen = true;
            }
            "MPEGTS" => {
                mpegts = deger.trim().parse::<f64>().ok()?;
                gorulen = true;
            }
            _ => {}
        }
    }
    if !gorulen {
        return None;
    }
    Some(mpegts / 90_000.0 - local)
}

/// Zaman satırı mı? (`00:00:01.000 --> 00:00:04.000 align:middle`)
fn zaman_satiri(satir: &str) -> Option<(f64, f64, String)> {
    let (sol, sag) = satir.split_once("-->")?;
    let bas = parse_timestamp(sol)?;
    let sag = sag.trim_start();
    // Bitiş damgasından sonrası yerleşim ayarı.
    let (bitis_metni, ayarlar) = match sag.find(char::is_whitespace) {
        Some(i) => (&sag[..i], sag[i..].trim()),
        None => (sag, ""),
    };
    let bitis = parse_timestamp(bitis_metni)?;
    Some((bas, bitis, ayarlar.to_string()))
}

/// Tek bir WebVTT belgesini ayrıştırır.
///
/// Bozuk bir blok tüm parçayı düşürmüyor: atlanıyor. Altyazı, indirmenin
/// yanında duran ikincil bir dosya — tek bozuk cue yüzünden hiç altyazı
/// vermemek orantısız olurdu.
pub fn parse_fragment(metin: &str) -> Fragment {
    let mut cikti = Fragment::default();
    // BOM ve satır sonu farkları: sağlayıcılar üçünü de gönderiyor.
    let metin = metin.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");

    let mut bloklar = metin.split("\n\n").peekable();

    // İlk blok başlık: `WEBVTT` satırı ve varsa `X-TIMESTAMP-MAP`.
    if let Some(ilk) = bloklar.peek() {
        if ilk.trim_start().starts_with("WEBVTT") {
            for satir in ilk.lines() {
                let satir = satir.trim();
                if satir.to_ascii_uppercase().starts_with("X-TIMESTAMP-MAP") {
                    if let Some(o) = timestamp_map(satir) {
                        cikti.offset = o;
                        cikti.has_map = true;
                    }
                }
            }
            bloklar.next();
        }
    }

    for blok in bloklar {
        let blok = blok.trim_matches('\n');
        if blok.trim().is_empty() {
            continue;
        }
        let ilk_satir = blok.lines().next().unwrap_or("").trim();
        let bas = ilk_satir.to_ascii_uppercase();

        if bas.starts_with("NOTE") {
            continue;
        }
        if bas == "STYLE" || bas == "REGION" {
            cikti.blocks.push(blok.to_string());
            continue;
        }

        // Cue: ilk satır kimlik olabilir, zaman satırı ikinci gelir.
        let mut satirlar = blok.lines();
        let mut zaman = satirlar.next().unwrap_or("");
        let mut zamanlama = zaman_satiri(zaman);
        if zamanlama.is_none() {
            // İlk satır kimlikti; zaman satırı bir sonraki.
            zaman = satirlar.next().unwrap_or("");
            zamanlama = zaman_satiri(zaman);
        }
        let Some((bas_sn, bitis_sn, ayarlar)) = zamanlama else {
            continue; // Ne kimlik ne zaman — tanımadığımız bir blok.
        };

        let metin: Vec<&str> = satirlar.collect();
        if metin.is_empty() {
            continue;
        }
        cikti.cues.push(Cue {
            start: bas_sn,
            end: bitis_sn,
            settings: ayarlar,
            text: metin.join("\n").trim_end().to_string(),
        });
    }

    cikti
}

/// Parçaları tek bir WebVTT belgesinde birleştirir.
///
/// Zaman ekseni ilk **haritalı** parçaya göre hizalanıyor; hiç harita yoksa
/// cue'lar olduğu gibi alınıyor (sağlayıcı zaten mutlak zaman yazmış demektir).
pub fn merge(parcalar: &[String]) -> String {
    let cozulmus: Vec<Fragment> = parcalar.iter().map(|p| parse_fragment(p)).collect();
    let taban = cozulmus.iter().find(|f| f.has_map).map(|f| f.offset).unwrap_or(0.0);

    let mut cikti = String::from("WEBVTT\n");
    let mut yazilan_blok: HashSet<String> = HashSet::new();
    for parca in &cozulmus {
        for blok in &parca.blocks {
            if yazilan_blok.insert(blok.clone()) {
                cikti.push('\n');
                cikti.push_str(blok);
                cikti.push('\n');
            }
        }
    }

    let mut cues: Vec<Cue> = Vec::new();
    for parca in &cozulmus {
        let kaydir = if parca.has_map { parca.offset - taban } else { 0.0 };
        for cue in &parca.cues {
            cues.push(Cue {
                start: (cue.start + kaydir).max(0.0),
                end: (cue.end + kaydir).max(0.0),
                settings: cue.settings.clone(),
                text: cue.text.clone(),
            });
        }
    }

    // Sıralama kararlı olmalı: aynı anda başlayan iki cue'nun sırası dosyada
    // göründükleri sıra. `sort_by` Rust'ta kararlı, `sort_unstable_by` değil.
    cues.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));

    let mut gorulen: HashSet<String> = HashSet::new();
    for cue in cues {
        let anahtar = format!(
            "{}|{}|{}",
            format_timestamp(cue.start),
            format_timestamp(cue.end),
            cue.text
        );
        if !gorulen.insert(anahtar) {
            continue; // Parça sınırını aşan cue ikinci kez geldi.
        }
        cikti.push('\n');
        cikti.push_str(&format_timestamp(cue.start));
        cikti.push_str(" --> ");
        cikti.push_str(&format_timestamp(cue.end));
        if !cue.settings.is_empty() {
            cikti.push(' ');
            cikti.push_str(&cue.settings);
        }
        cikti.push('\n');
        cikti.push_str(&cue.text);
        cikti.push('\n');
    }

    cikti
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zaman_damgasi_iki_bicimi_de_okuyor() {
        assert_eq!(parse_timestamp("00:00:01.500"), Some(1.5));
        assert_eq!(parse_timestamp("01:02.250"), Some(62.25));
        assert_eq!(parse_timestamp("01:00:00.000"), Some(3600.0));
        assert_eq!(parse_timestamp("12"), Some(12.0));
        assert_eq!(parse_timestamp(""), None);
        assert_eq!(parse_timestamp("abc"), None);
        assert_eq!(parse_timestamp("00:00:01.abc"), None);
    }

    #[test]
    fn zaman_damgasi_yazimi_yuvarlamada_tasmiyor() {
        assert_eq!(format_timestamp(0.0), "00:00:00.000");
        assert_eq!(format_timestamp(62.25), "00:01:02.250");
        assert_eq!(format_timestamp(3661.5), "01:01:01.500");
        // Önce parçalayıp sonra yuvarlayan bir kod burada "00:00:60.000" yazardı.
        assert_eq!(format_timestamp(59.9996), "00:01:00.000");
        // Negatif ve NaN sıfıra çekiliyor, panik yok.
        assert_eq!(format_timestamp(-5.0), "00:00:00.000");
        assert_eq!(format_timestamp(f64::NAN), "00:00:00.000");
    }

    #[test]
    fn timestamp_map_okunuyor() {
        assert_eq!(
            timestamp_map("X-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:900000"),
            Some(10.0)
        );
        assert_eq!(
            timestamp_map("X-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:05.000"),
            Some(5.0)
        );
        assert_eq!(timestamp_map("X-TIMESTAMP-MAP=BOZUK"), None);
    }

    #[test]
    fn kimlikli_ve_kimliksiz_cue_ayristiriliyor() {
        let f = parse_fragment(
            "WEBVTT\n\n\
             1\n00:00:01.000 --> 00:00:02.000\nMerhaba\n\n\
             00:00:03.000 --> 00:00:04.000 align:start\nDünya\n",
        );
        assert_eq!(f.cues.len(), 2);
        assert_eq!(f.cues[0].text, "Merhaba");
        assert_eq!(f.cues[0].settings, "");
        assert_eq!(f.cues[1].text, "Dünya");
        assert_eq!(f.cues[1].settings, "align:start");
    }

    #[test]
    fn not_atlaniyor_stil_korunuyor() {
        let f = parse_fragment(
            "WEBVTT\n\n\
             NOTE bu bir açıklama\n\n\
             STYLE\n::cue { color: yellow }\n\n\
             00:00:01.000 --> 00:00:02.000\nX\n",
        );
        assert_eq!(f.cues.len(), 1);
        assert_eq!(f.blocks.len(), 1);
        assert!(f.blocks[0].starts_with("STYLE"));
    }

    #[test]
    fn crlf_ve_bom_temizleniyor() {
        let f = parse_fragment("\u{feff}WEBVTT\r\n\r\n00:00:01.000 --> 00:00:02.000\r\nX\r\n");
        assert_eq!(f.cues.len(), 1);
        assert_eq!(f.cues[0].text, "X");
    }

    #[test]
    fn parcalar_ilk_haritaya_gore_hizalaniyor() {
        // İki parça da MPEGTS 900000'de başlıyor gibi görünüyor ama ikincisi
        // 10 saniye ileride. Fark alındığı için çıktı sıfırdan başlamalı.
        let a = "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:900000\n\n\
                 00:00:00.000 --> 00:00:02.000\nBir\n"
            .to_string();
        let b = "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:1800000\n\n\
                 00:00:00.000 --> 00:00:02.000\nİki\n"
            .to_string();
        let cikti = merge(&[a, b]);
        assert!(cikti.starts_with("WEBVTT\n"));
        assert!(cikti.contains("00:00:00.000 --> 00:00:02.000\nBir"));
        assert!(cikti.contains("00:00:10.000 --> 00:00:12.000\nİki"));
    }

    #[test]
    fn parca_sinirini_asan_cue_bir_kez_yaziliyor() {
        let a = "WEBVTT\n\n00:00:08.000 --> 00:00:12.000\nOrtak\n".to_string();
        let b = "WEBVTT\n\n00:00:08.000 --> 00:00:12.000\nOrtak\n".to_string();
        let cikti = merge(&[a, b]);
        assert_eq!(cikti.matches("Ortak").count(), 1);
    }

    #[test]
    fn cikti_tek_webvtt_basligi_tasiyor() {
        let a = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nA\n".to_string();
        let b = "WEBVTT\n\n00:00:03.000 --> 00:00:04.000\nB\n".to_string();
        let cikti = merge(&[a, b]);
        assert_eq!(cikti.matches("WEBVTT").count(), 1);
        // Sıra korunuyor. Aranan metin başlıkta geçmemeli: "WEBVTT" içinde
        // 'B' harfi var ve tek harflik bir arama onu bulurdu.
        assert!(cikti.find("00:00:01").unwrap() < cikti.find("00:00:03").unwrap());
    }

    #[test]
    fn haritasiz_parcalar_oldugu_gibi_aliniyor() {
        let a = "WEBVTT\n\n00:00:05.000 --> 00:00:06.000\nA\n".to_string();
        let cikti = merge(&[a]);
        assert!(cikti.contains("00:00:05.000 --> 00:00:06.000"));
    }

    #[test]
    fn bozuk_blok_parcayi_dusurmuyor() {
        let f = parse_fragment(
            "WEBVTT\n\n\
             bu blok ne kimlik ne zaman\n\n\
             00:00:01.000 --> 00:00:02.000\nSağlam\n",
        );
        assert_eq!(f.cues.len(), 1);
        assert_eq!(f.cues[0].text, "Sağlam");
    }

    #[test]
    fn cok_satirli_cue_metni_korunuyor() {
        let f = parse_fragment("WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nBir\nİki\n");
        assert_eq!(f.cues[0].text, "Bir\nİki");
    }

    #[test]
    fn bos_giris_gecerli_bos_belge_veriyor() {
        assert_eq!(merge(&[]), "WEBVTT\n");
    }
}
