//! Dosyayı paralel indirilecek byte aralıklarına bölme planı.
//!
//! Karar #3: `Accept-Ranges: bytes` destekleniyorsa dosya N parçaya bölünür.
//! Buradaki tek iş **plan** çıkarmak — indirme yok, I/O yok, ağ yok. Bu yüzden
//! tamamen saf fonksiyonlar ve doğrudan test edilebilir.

use serde::{Deserialize, Serialize};

/// Varsayılan segment sayısı (`docs/project_overview.md` → Çekirdek İndirme Motoru).
pub const DEFAULT_SEGMENTS: usize = 8;

/// Bir segmentin altına düşmemesi gereken boyut.
///
/// 2 MB'lik bir dosyayı 8 parçaya bölmek 8 TCP el sıkışması + 8 TLS anlaşması
/// demek; bunların maliyeti kazanılan paralellikten fazla. Bu eşik altında
/// segment sayısı otomatik azaltılır.
pub const MIN_SEGMENT_SIZE: u64 = 1024 * 1024;

/// Üst sınır. Daha fazlası sunucu tarafında bağlantı reddine yol açıyor ve
/// çoğu sunucuda tek IP başına eşzamanlı bağlantı kotasını zorluyor.
pub const MAX_SEGMENTS: usize = 32;

/// Dosyanın bir parçası. `start`/`end` **mutlak** ve `end` **dahil**
/// (HTTP `Range: bytes=start-end` semantiği ile birebir aynı).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    /// Bu segmentten şu ana kadar inen byte sayısı.
    #[serde(default)]
    pub downloaded: u64,
}

impl Segment {
    pub fn new(index: usize, start: u64, end: u64) -> Self {
        Segment { index, start, end, downloaded: 0 }
    }

    /// Segmentin toplam boyutu (`end` dahil olduğu için +1).
    pub fn total(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }

    pub fn remaining(&self) -> u64 {
        self.total().saturating_sub(self.downloaded)
    }

    /// Sıradaki byte'ın mutlak konumu — `Range` başlığı buradan kuruluyor.
    pub fn cursor(&self) -> u64 {
        self.start + self.downloaded
    }

    pub fn is_complete(&self) -> bool {
        self.downloaded >= self.total()
    }
}

/// Segment planı çıkarır.
///
/// * `total_size` — dosyanın tam boyutu
/// * `desired` — kullanıcının istediği segment sayısı (1..=[`MAX_SEGMENTS`] aralığına kırpılır)
/// * `min_segment_size` — bir segmentin altına düşemeyeceği boyut
///
/// Boyut segment sayısına tam bölünmediğinde artan byte'lar **baştaki**
/// segmentlere birer birer dağıtılır; hepsi son segmente yığılmaz. Böylece
/// segmentler arası fark en fazla 1 byte olur ve hiçbir worker diğerlerinden
/// belirgin geç bitmez.
pub fn plan_segments(total_size: u64, desired: usize, min_segment_size: u64) -> Vec<Segment> {
    if total_size == 0 {
        // Boş dosya: indirilecek byte yok. Çağıran tarafın yapacağı tek şey
        // dosyayı oluşturup tamamlandı işaretlemek.
        return Vec::new();
    }

    let desired = desired.clamp(1, MAX_SEGMENTS);
    let min_size = min_segment_size.max(1);

    // Segment sayısı, her segmente en az `min_size` düşecek şekilde kısıtlanır.
    let by_size = (total_size / min_size).max(1) as usize;
    let count = desired.min(by_size).max(1);

    let base = total_size / count as u64;
    let remainder = total_size % count as u64;

    let mut segments = Vec::with_capacity(count);
    let mut cursor = 0u64;
    for index in 0..count {
        // Artan byte'lar baştaki `remainder` segmente birer birer dağıtılıyor.
        let size = base + if (index as u64) < remainder { 1 } else { 0 };
        let end = cursor + size - 1;
        segments.push(Segment::new(index, cursor, end));
        cursor = end + 1;
    }

    debug_assert_eq!(cursor, total_size, "segment planı dosyanın tamamını kapsamalı");
    segments
}

/// Sunucu `Range` desteklemediğinde kullanılan tek parçalık plan.
pub fn single_segment(total_size: u64) -> Vec<Segment> {
    if total_size == 0 {
        Vec::new()
    } else {
        vec![Segment::new(0, 0, total_size - 1)]
    }
}

/// Yavaş bir segmentin kalan aralığını ikiye böler (karar #5 — "work stealing").
///
/// Dönen değer `(yeni_end, calinan_start, calinan_end)`. Bölme yapılamıyorsa
/// (kalan çok küçükse) `None` döner — bir segmenti 1 KB için bölmek yeni bir
/// TCP+TLS el sıkışmasına değmez.
///
/// `cursor`, yavaş worker'ın **şu an** yazmakta olduğu mutlak konum. Bölme
/// noktası cursor'ın gerisinde olamaz: worker o byte'ları zaten yazdı.
pub fn split_remaining(cursor: u64, end: u64, min_steal: u64) -> Option<(u64, u64, u64)> {
    let remaining = end.checked_sub(cursor)?;

    // Kalan, eşiğin iki katından küçükse bölmenin anlamı yok: ya çalınan parça
    // çok küçük olur ya da yavaş worker'a iş kalmaz.
    if remaining < min_steal.saturating_mul(2) {
        return None;
    }

    let mid = cursor + remaining / 2;
    Some((mid, mid + 1, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Planın dosyanın tamamını, boşluksuz ve çakışmasız kapsadığını doğrular.
    fn butunluk_kontrolu(segments: &[Segment], total: u64) {
        assert_eq!(segments[0].start, 0, "ilk segment 0'dan başlamalı");
        assert_eq!(segments.last().unwrap().end, total - 1, "son segment dosyanın sonunda bitmeli");

        for pair in segments.windows(2) {
            assert_eq!(pair[1].start, pair[0].end + 1, "segmentler arasında boşluk/çakışma var");
        }

        let toplam: u64 = segments.iter().map(|s| s.total()).sum();
        assert_eq!(toplam, total, "segment boyutları toplamı dosya boyutunu vermeli");
    }

    #[test]
    fn tam_bolunen_boyut_esit_paylara_ayriliyor() {
        let segments = plan_segments(8 * 1024 * 1024, 8, MIN_SEGMENT_SIZE);
        assert_eq!(segments.len(), 8);
        assert!(segments.iter().all(|s| s.total() == 1024 * 1024));
        butunluk_kontrolu(&segments, 8 * 1024 * 1024);
    }

    #[test]
    fn artan_byte_bastaki_segmentlere_dagitiliyor() {
        // 10 MB + 3 byte, 4 segment → ilk 3 segment 1 byte fazla almalı.
        let total = 10 * 1024 * 1024 + 3;
        let segments = plan_segments(total, 4, MIN_SEGMENT_SIZE);

        assert_eq!(segments.len(), 4);
        let base = total / 4;
        assert_eq!(segments[0].total(), base + 1);
        assert_eq!(segments[1].total(), base + 1);
        assert_eq!(segments[2].total(), base + 1);
        assert_eq!(segments[3].total(), base);
        butunluk_kontrolu(&segments, total);
    }

    #[test]
    fn kucuk_dosya_asiri_parcalanmiyor() {
        // 3 MB dosya, 8 segment istendi ama minimum 1 MB → en fazla 3 segment.
        let segments = plan_segments(3 * 1024 * 1024, 8, MIN_SEGMENT_SIZE);
        assert_eq!(segments.len(), 3);
        butunluk_kontrolu(&segments, 3 * 1024 * 1024);
    }

    #[test]
    fn minimumun_altindaki_dosya_tek_segment_kaliyor() {
        let segments = plan_segments(500, 8, MIN_SEGMENT_SIZE);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], Segment::new(0, 0, 499));
    }

    #[test]
    fn bos_dosya_segment_uretmiyor() {
        assert!(plan_segments(0, 8, MIN_SEGMENT_SIZE).is_empty());
        assert!(single_segment(0).is_empty());
    }

    #[test]
    fn segment_sayisi_ust_sinira_kirpiliyor() {
        let total = 1024 * 1024 * 1024; // 1 GB — boyut sınırlayıcı değil
        let segments = plan_segments(total, 500, MIN_SEGMENT_SIZE);
        assert_eq!(segments.len(), MAX_SEGMENTS);
        butunluk_kontrolu(&segments, total);
    }

    #[test]
    fn sifir_segment_istegi_tek_segmente_yuvarlaniyor() {
        let segments = plan_segments(5 * 1024 * 1024, 0, MIN_SEGMENT_SIZE);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn cesitli_boyutlarda_butunluk_bozulmuyor() {
        let boyutlar = [1u64, 2, 999, 1024, 1_048_576, 1_048_577, 33_554_432, 99_999_999];
        for &total in &boyutlar {
            for desired in [1usize, 2, 3, 8, 16, 32] {
                let segments = plan_segments(total, desired, MIN_SEGMENT_SIZE);
                assert!(!segments.is_empty(), "boyut {total}, {desired} segment: plan boş");
                butunluk_kontrolu(&segments, total);
            }
        }
    }

    #[test]
    fn segment_ilerleme_hesaplari() {
        let mut s = Segment::new(0, 100, 199);
        assert_eq!(s.total(), 100);
        assert_eq!(s.cursor(), 100);
        assert_eq!(s.remaining(), 100);
        assert!(!s.is_complete());

        s.downloaded = 40;
        assert_eq!(s.cursor(), 140);
        assert_eq!(s.remaining(), 60);

        s.downloaded = 100;
        assert!(s.is_complete());
        assert_eq!(s.remaining(), 0);
    }

    #[test]
    fn bolme_kalan_araligi_ikiye_ayiriyor() {
        // cursor 1000, end 3000 → kalan 2000, orta nokta 2000.
        let (yeni_end, calinan_start, calinan_end) = split_remaining(1000, 3000, 100).unwrap();
        assert_eq!(yeni_end, 2000);
        assert_eq!(calinan_start, 2001);
        assert_eq!(calinan_end, 3000);
        // Bölme sonrası iki aralık birleşince orijinali vermeli.
        assert_eq!(calinan_end - 1000, 2000);
    }

    #[test]
    fn kucuk_kalan_bolunmuyor() {
        // Kalan 150, eşik 100 → 2*100'den küçük, bölme yok.
        assert!(split_remaining(1000, 1150, 100).is_none());
        // Kalan tam eşiğin iki katı → bölünebilir.
        assert!(split_remaining(1000, 1200, 100).is_some());
    }

    #[test]
    fn cursor_endin_gerisindeyse_bolme_yok() {
        assert!(split_remaining(3000, 1000, 100).is_none());
    }
}
