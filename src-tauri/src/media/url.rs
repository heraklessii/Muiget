//! Manifest içindeki göreli adreslerin çözülmesi.
//!
//! HLS ve DASH manifestleri segmentlerini neredeyse hep göreli yazıyor
//! (`seg00042.ts`, `../audio/init.mp4`, `/hls/720p/list.m3u8`). Bunları taban
//! adrese göre mutlak hâle getirmek gerekiyor.
//!
//! Neden `url` crate'i değil: motorun geri kalanı (bkz. `download/http.rs`,
//! `download/throttle.rs`) adresleri zaten dizge olarak işliyor ve tek bir
//! birleştirme fonksiyonu için yeni bir bağımlılık (ve onun `idna`/`percent`
//! ağacı) getirmek orantısız. Buradaki kapsam dar ve testli: şema, `//`,
//! kök-göreli, dizin-göreli ve `.`/`..` normalizasyonu.

/// `base` adresine göre `rel` adresini mutlak hâle getirir.
///
/// `base` mutlak (`http://…`) değilse yapılacak bir şey yok; `rel` olduğu gibi
/// dönüyor — çağıran zaten mutlak bir manifest adresiyle geliyor.
pub fn resolve(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() {
        return base.to_string();
    }
    if sema_var(rel) {
        return rel.to_string();
    }

    let Some((sema, kalan)) = base.split_once("://") else {
        return rel.to_string();
    };

    // `//cdn.example.com/x.ts` — şemayı taban adresten miras alır.
    if rel.starts_with("//") {
        return format!("{sema}:{rel}");
    }

    let son = kalan.find(['/', '?', '#']).unwrap_or(kalan.len());
    let (yetki, yol_ham) = kalan.split_at(son);
    // Taban adresin sorgusu ve parçası atılıyor: göreli adres onları miras
    // almaz (RFC 3986 §5.3).
    let yol = yol_ham.split(['?', '#']).next().unwrap_or("");

    if rel.starts_with('/') {
        return format!("{sema}://{yetki}{}", normalize(rel));
    }

    // `?yeni=sorgu` ve `#parça` taban **yolunu** koruyor.
    if rel.starts_with('?') || rel.starts_with('#') {
        let temiz = if yol.is_empty() { "/" } else { yol };
        return format!("{sema}://{yetki}{temiz}{rel}");
    }

    let dizin = match yol.rfind('/') {
        Some(i) => &yol[..=i],
        None => "/",
    };
    format!("{sema}://{yetki}{}", normalize(&format!("{dizin}{rel}")))
}

/// Adresin dizin kısmı (son `/` dâhil). Segment adları buna ekleniyor.
pub fn directory_of(url: &str) -> String {
    let sorgusuz = url.split(['?', '#']).next().unwrap_or(url);
    match sorgusuz.rfind('/') {
        // `http://a.com/x` içindeki ilk iki `/` şemanın; onlara kadar kırpmak
        // `http:/` gibi bozuk bir taban üretirdi.
        Some(i) if i > sorgusuz.find("://").map(|s| s + 2).unwrap_or(0) => sorgusuz[..=i].to_string(),
        _ => format!("{sorgusuz}/"),
    }
}

/// Adres bir şemayla mı başlıyor (`http:`, `data:`, `blob:`)?
///
/// `contains("://")` yetmiyor: `a/b?x=http://c` göreli bir adres ama o testi
/// geçerdi. Şema, adresin **başında** ve ilk `/`den önce olmak zorunda.
fn sema_var(url: &str) -> bool {
    let bytes = url.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &c) in bytes.iter().enumerate() {
        match c {
            b':' => return i > 0,
            c if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' => {}
            _ => return false,
        }
    }
    false
}

/// `.` ve `..` parçalarını çözer. Sorgu ve parça dokunulmadan sona ekleniyor.
fn normalize(path: &str) -> String {
    let (yol, kuyruk) = match path.find(['?', '#']) {
        Some(i) => path.split_at(i),
        None => (path, ""),
    };

    let mutlak = yol.starts_with('/');
    // Sondaki `/`, `/.` ve `/..` dizin anlamı taşıyor; korunuyor ki
    // `a/b/../` ile `a/b/..` farklı sonuç vermesin.
    let dizinle_bitiyor = yol.ends_with('/') || yol.ends_with("/.") || yol.ends_with("/..");

    let mut yigin: Vec<&str> = Vec::new();
    for parca in yol.split('/') {
        match parca {
            "" | "." => {}
            ".." => {
                yigin.pop();
            }
            p => yigin.push(p),
        }
    }

    let mut cikti = String::new();
    if mutlak {
        cikti.push('/');
    }
    cikti.push_str(&yigin.join("/"));
    if dizinle_bitiyor && !cikti.ends_with('/') {
        cikti.push('/');
    }
    cikti.push_str(kuyruk);
    cikti
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABAN: &str = "https://cdn.example.com/hls/720p/list.m3u8";

    #[test]
    fn mutlak_adres_oldugu_gibi_kaliyor() {
        assert_eq!(resolve(TABAN, "https://baska.com/a.ts"), "https://baska.com/a.ts");
        assert_eq!(resolve(TABAN, "http://baska.com/a.ts"), "http://baska.com/a.ts");
    }

    #[test]
    fn sema_gorunumlu_goreli_adres_mutlak_sayilmiyor() {
        // Sorgu içinde şema geçiyor ama adresin kendisi göreli.
        assert_eq!(
            resolve(TABAN, "seg.ts?u=http://x.com/y"),
            "https://cdn.example.com/hls/720p/seg.ts?u=http://x.com/y"
        );
    }

    #[test]
    fn dizin_goreli_cozuluyor() {
        assert_eq!(resolve(TABAN, "seg1.ts"), "https://cdn.example.com/hls/720p/seg1.ts");
    }

    #[test]
    fn kok_goreli_cozuluyor() {
        assert_eq!(resolve(TABAN, "/other/a.ts"), "https://cdn.example.com/other/a.ts");
    }

    #[test]
    fn semasiz_adres_tabanin_semasini_aliyor() {
        assert_eq!(resolve(TABAN, "//ikinci.cdn/a.ts"), "https://ikinci.cdn/a.ts");
    }

    #[test]
    fn ust_dizin_cozuluyor() {
        assert_eq!(resolve(TABAN, "../audio/init.mp4"), "https://cdn.example.com/hls/audio/init.mp4");
        assert_eq!(resolve(TABAN, "../../x.ts"), "https://cdn.example.com/x.ts");
        // Kökün üstüne çıkmak yok sayılıyor.
        assert_eq!(resolve(TABAN, "../../../../x.ts"), "https://cdn.example.com/x.ts");
    }

    #[test]
    fn nokta_parcasi_atiliyor() {
        assert_eq!(resolve(TABAN, "./seg.ts"), "https://cdn.example.com/hls/720p/seg.ts");
    }

    #[test]
    fn tabanin_sorgusu_miras_alinmiyor() {
        assert_eq!(
            resolve("https://a.com/p/list.m3u8?token=abc", "seg.ts"),
            "https://a.com/p/seg.ts"
        );
    }

    #[test]
    fn segmentin_kendi_sorgusu_korunuyor() {
        assert_eq!(
            resolve(TABAN, "seg.ts?token=xyz"),
            "https://cdn.example.com/hls/720p/seg.ts?token=xyz"
        );
    }

    #[test]
    fn yolsuz_taban_koke_cozuluyor() {
        assert_eq!(resolve("https://a.com", "seg.ts"), "https://a.com/seg.ts");
    }

    #[test]
    fn dizin_hesabi() {
        assert_eq!(directory_of(TABAN), "https://cdn.example.com/hls/720p/");
        assert_eq!(directory_of("https://a.com/x.m3u8?t=1"), "https://a.com/");
        assert_eq!(directory_of("https://a.com"), "https://a.com/");
    }
}
