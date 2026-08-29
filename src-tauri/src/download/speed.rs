//! Hız ölçümü (Faz 2).
//!
//! Anlık hız (son saniyede inen byte) arayüzde okunaksız: sayı her yenilemede
//! zıplıyor. Bunun yerine üstel ağırlıklı hareketli ortalama (EWMA) kullanılıyor
//! — eski ölçümler yarı-ömürle sönüyor, gösterge yumuşak kalıyor ama gerçek bir
//! yavaşlamaya birkaç saniyede tepki veriyor.
//!
//! Ölçüm ayrıca adaptif segment bölmenin girdisi: hangi segmentin "yavaş"
//! olduğuna bu sayılara bakarak karar veriliyor (karar #5).

use std::time::{Duration, Instant};

/// Ortalamanın yarıya inme süresi. Kısa tutmak göstergeyi titretiyor, uzun
/// tutmak yavaşlamayı geç fark ettiriyor; 3 saniye ikisinin arasında.
const HALF_LIFE: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub struct SpeedMeter {
    /// Byte/saniye cinsinden yumuşatılmış hız.
    ewma: f64,
    /// Son örneklemeden bu yana biriken byte.
    pending: u64,
    last_sample: Instant,
    /// Ölçüme hiç örnek girmediyse EWMA'yı ilk gerçek değere eşitle — aksi
    /// hâlde gösterge sıfırdan tırmanıyor ve ilk saniyeler yanlış görünüyor.
    primed: bool,
}

impl SpeedMeter {
    pub fn new(now: Instant) -> Self {
        SpeedMeter { ewma: 0.0, pending: 0, last_sample: now, primed: false }
    }

    /// İnen byte'ları biriktirir. Ucuz olmalı: her chunk'ta çağrılıyor.
    pub fn record(&mut self, bytes: u64) {
        self.pending += bytes;
    }

    /// Biriken byte'ları hıza dönüştürür. Periyodik olarak (arayüz yenileme
    /// aralığında) çağrılır ve güncel hızı byte/saniye döndürür.
    pub fn sample_at(&mut self, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(self.last_sample);
        // Çok kısa aralıkta bölme sonucu patlıyor; örneklemeyi atla.
        if elapsed < Duration::from_millis(50) {
            return self.ewma;
        }

        let anlik = self.pending as f64 / elapsed.as_secs_f64();
        self.pending = 0;
        self.last_sample = now;

        if !self.primed {
            self.ewma = anlik;
            self.primed = true;
        } else {
            // Yarı-ömürden türeyen ağırlık: aralık uzadıkça yeni ölçüm ağır basar.
            let alpha = 1.0 - 0.5_f64.powf(elapsed.as_secs_f64() / HALF_LIFE.as_secs_f64());
            self.ewma += alpha * (anlik - self.ewma);
        }

        self.ewma
    }

    pub fn speed(&self) -> f64 {
        self.ewma
    }

    /// Bağlantı tamamen koptuğunda göstergeyi sıfıra çeker; bir sonraki örnek
    /// ilk değer gibi davranır.
    pub fn reset(&mut self, now: Instant) {
        self.ewma = 0.0;
        self.pending = 0;
        self.last_sample = now;
        self.primed = false;
    }
}

/// Kalan süre tahmini (saniye).
///
/// Hız sıfıra çok yakınken bölme sonucu anlamsız büyüklüklere çıkıyor
/// ("kalan süre: 47 gün"); böyle durumlarda tahmin göstermemek daha dürüst.
pub fn eta_seconds(remaining_bytes: u64, speed_bps: f64) -> Option<u64> {
    if remaining_bytes == 0 {
        return Some(0);
    }
    if speed_bps < 1024.0 {
        return None;
    }
    Some((remaining_bytes as f64 / speed_bps).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ilk_ornek_dogrudan_hiz_oluyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);

        m.record(1_000_000);
        let hiz = m.sample_at(t0 + Duration::from_secs(1));

        // 1 saniyede 1 MB → 1 MB/s, yumuşatma olmadan.
        assert!((hiz - 1_000_000.0).abs() < 1.0, "beklenmeyen hız: {hiz}");
    }

    #[test]
    fn ewma_ani_dususu_yumusatiyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);

        m.record(1_000_000);
        m.sample_at(t0 + Duration::from_secs(1));

        // Hız aniden sıfırlanıyor; gösterge tek adımda sıfıra düşmemeli.
        let hiz = m.sample_at(t0 + Duration::from_secs(2));
        assert!(hiz > 0.0, "EWMA tek adımda çöktü: {hiz}");
        assert!(hiz < 1_000_000.0, "EWMA hiç düşmedi: {hiz}");
    }

    /// Belgelenen davranış: hız kesilince ortalama her yarı-ömürde yarıya iner.
    /// Keyfi bir eşik yerine bu özelliği doğrudan sınıyoruz.
    #[test]
    fn ortalama_her_yari_omurde_yariya_iniyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);
        m.record(1_000_000);
        let baslangic = m.sample_at(t0 + Duration::from_secs(1));

        // Hız tamamen kesildi; bir yarı-ömür sonra yarısı kalmalı.
        let bir_yari_omur = m.sample_at(t0 + Duration::from_secs(1) + HALF_LIFE);
        let oran = bir_yari_omur / baslangic;
        assert!((oran - 0.5).abs() < 0.02, "yarı-ömür sonrası oran {oran}, 0.5 bekleniyordu");
    }

    #[test]
    fn surekli_sifir_hizda_ortalama_sifira_yaklasiyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);
        m.record(1_000_000);
        m.sample_at(t0 + Duration::from_secs(1));

        let mut onceki = f64::MAX;
        let mut son = f64::MAX;
        for saniye in 2..=40 {
            son = m.sample_at(t0 + Duration::from_secs(saniye));
            assert!(son < onceki, "ortalama {saniye}. saniyede artmış");
            onceki = son;
        }

        // 39 saniye = 13 yarı-ömür → 1 MB/s'nin ~8000'de biri.
        assert!(son < 1_000.0, "40 saniye sonra hâlâ {son} B/s");
    }

    #[test]
    fn cok_kisa_aralikta_orneklenmiyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);
        m.record(500);

        let hiz = m.sample_at(t0 + Duration::from_millis(10));
        assert_eq!(hiz, 0.0, "10 ms'lik aralık örneklenmemeli");
        // Biriken byte korunmalı: bir sonraki gerçek örneklemede kullanılacak.
        assert_eq!(m.pending, 500);
    }

    #[test]
    fn reset_gostergeyi_sifirliyor() {
        let t0 = Instant::now();
        let mut m = SpeedMeter::new(t0);
        m.record(1_000_000);
        m.sample_at(t0 + Duration::from_secs(1));
        assert!(m.speed() > 0.0);

        m.reset(t0 + Duration::from_secs(1));
        assert_eq!(m.speed(), 0.0);

        // Reset sonrası ilk örnek yine doğrudan değeri almalı.
        m.record(2_000_000);
        let hiz = m.sample_at(t0 + Duration::from_secs(2));
        assert!((hiz - 2_000_000.0).abs() < 1.0);
    }

    #[test]
    fn eta_hesabi() {
        assert_eq!(eta_seconds(0, 0.0), Some(0), "kalan yoksa süre de yok");
        assert_eq!(eta_seconds(10_000_000, 1_000_000.0), Some(10));
        // Küsurat yukarı yuvarlanmalı: 1.5 saniye → 2.
        assert_eq!(eta_seconds(1_500_000, 1_000_000.0), Some(2));
    }

    #[test]
    fn cok_dusuk_hizda_eta_gosterilmiyor() {
        assert_eq!(eta_seconds(1_000_000_000, 0.0), None);
        assert_eq!(eta_seconds(1_000_000_000, 500.0), None);
        assert!(eta_seconds(1_000_000_000, 2048.0).is_some());
    }
}
