//! Küçük bir XML okuyucu — yalnızca DASH manifesti (`.mpd`) için.
//!
//! Neden hazır bir crate değil: `quick-xml`/`roxmltree` ikisi de iyi ama tek
//! kullanıcısı DASH olan bir okuyucu için bağımlılık ağacını büyütmek bu
//! projenin ölçütüne uymuyor (aynı gerekçe `librqbit` ertelemesinde de var,
//! bkz. `docs/decisions.md`). MPD'nin ihtiyacı olan yüzey dar: öğe, öznitelik,
//! iç içe geçme, yorum, CDATA ve beş varlık kaçışı.
//!
//! **Bilinçli sınırlar:** ad alanı önekleri (`xmlns:cenc`) çözülmüyor, yalnızca
//! atılıyor — MPD'de aynı yerel ada sahip iki farklı ad alanı öğesi
//! kullanılmıyor. DTD, işleme yönergesi ve varlık tanımı atlanıyor: dış varlık
//! okumak (XXE) bu okuyucuda mümkün değil, çünkü hiç uygulanmadı.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError(pub String);

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XML okunamadı: {}", self.0)
    }
}

/// Ayrıştırılmış bir öğe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// Ad alanı öneki atılmış yerel ad (`cenc:pssh` → `pssh`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// Doğrudan metin içeriği (alt öğelerinki değil), kırpılmamış hâlde.
    pub text: String,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(ad, _)| ad == name)
            .map(|(_, deger)| deger.as_str())
    }

    /// Özniteliği sayıya çevirir. Bozuk değer `None` — MPD'de bozuk bir
    /// `bandwidth` yüzünden tüm manifesti reddetmek orantısız olurdu.
    pub fn attr_num<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.attr(name).and_then(|d| d.trim().parse().ok())
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Alt ağacında bu adda bir öğe var mı? (DRM tespiti için.)
    pub fn has_descendant(&self, name: &str) -> bool {
        self.children.iter().any(|c| c.name == name || c.has_descendant(name))
    }
}

/// Belgeyi ayrıştırıp kök öğeyi döner.
pub fn parse(input: &str) -> Result<Node, XmlError> {
    let mut i = 0usize;
    let mut yigin: Vec<Node> = Vec::new();
    let mut kok: Option<Node> = None;

    while i < input.len() {
        if input.as_bytes()[i] != b'<' {
            let sonraki = input[i..].find('<').map(|d| i + d).unwrap_or(input.len());
            if let Some(node) = yigin.last_mut() {
                node.text.push_str(&decode_entities(&input[i..sonraki]));
            }
            i = sonraki;
            continue;
        }

        let kalan = &input[i..];

        if let Some(atlanan) = atla(kalan, "<!--", "-->") {
            i += atlanan;
            continue;
        }
        if let Some(atlanan) = atla(kalan, "<?", "?>") {
            i += atlanan;
            continue;
        }
        if kalan.starts_with("<![CDATA[") {
            let son = kalan
                .find("]]>")
                .ok_or_else(|| XmlError("kapanmayan CDATA".into()))?;
            if let Some(node) = yigin.last_mut() {
                node.text.push_str(&kalan[9..son]);
            }
            i += son + 3;
            continue;
        }
        if kalan.starts_with("<!") {
            // DOCTYPE ve benzeri: `>`e kadar atla. İçindeki varlık tanımları
            // bilerek yok sayılıyor.
            let son = kalan
                .find('>')
                .ok_or_else(|| XmlError("kapanmayan <! bildirimi".into()))?;
            i += son + 1;
            continue;
        }

        let kapanis = kalan.starts_with("</");
        let etiket_sonu =
            etiket_sonunu_bul(kalan).ok_or_else(|| XmlError("kapanmayan etiket".into()))?;
        let ic = &kalan[if kapanis { 2 } else { 1 }..etiket_sonu];

        if kapanis {
            let node = yigin
                .pop()
                .ok_or_else(|| XmlError(format!("fazladan kapanış: {}", ic.trim())))?;
            if node.name != yerel_ad(ic.trim()) {
                return Err(XmlError(format!(
                    "etiket eşleşmiyor: <{}> … </{}>",
                    node.name,
                    ic.trim()
                )));
            }
            yerlestir(node, &mut yigin, &mut kok);
        } else {
            let kendi_kapanan = ic.trim_end().ends_with('/');
            let govde = if kendi_kapanan {
                ic.trim_end().trim_end_matches('/')
            } else {
                ic
            };
            let (ad, attrs) = etiketi_coz(govde);
            let node = Node { name: ad, attrs, children: Vec::new(), text: String::new() };
            if kendi_kapanan {
                yerlestir(node, &mut yigin, &mut kok);
            } else {
                yigin.push(node);
            }
        }

        i += etiket_sonu + 1;
    }

    if !yigin.is_empty() {
        return Err(XmlError(format!("kapanmayan <{}>", yigin.last().unwrap().name)));
    }
    kok.ok_or_else(|| XmlError("kök öğe yok".into()))
}

fn yerlestir(node: Node, yigin: &mut [Node], kok: &mut Option<Node>) {
    match yigin.last_mut() {
        Some(ust) => ust.children.push(node),
        // Kök zaten varsa ikinci kök öğe yok sayılıyor: bozuk bir kuyruk
        // yüzünden okunmuş manifesti çöpe atmak orantısız.
        None if kok.is_none() => *kok = Some(node),
        None => {}
    }
}

/// `bas` ile başlıyorsa `son`a kadar atlar ve atlanan uzunluğu döner.
fn atla(kalan: &str, bas: &str, son: &str) -> Option<usize> {
    if !kalan.starts_with(bas) {
        return None;
    }
    // Kapanış bulunamazsa belgenin sonuna kadar atlanıyor — yarım bir yorum
    // yüzünden hata döndürmenin kazandırdığı bir şey yok.
    match kalan[bas.len()..].find(son) {
        Some(d) => Some(bas.len() + d + son.len()),
        None => Some(kalan.len()),
    }
}

/// Etiketi kapatan `>` işaretinin konumu. Tırnak içindeki `>` sayılmıyor.
fn etiket_sonunu_bul(kalan: &str) -> Option<usize> {
    let mut tirnak: Option<u8> = None;
    for (i, c) in kalan.bytes().enumerate() {
        match (tirnak, c) {
            (Some(t), c) if c == t => tirnak = None,
            (Some(_), _) => {}
            (None, b'"') | (None, b'\'') => tirnak = Some(c),
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// `Representation id="1" bandwidth="800000"` → ad + öznitelikler.
fn etiketi_coz(govde: &str) -> (String, Vec<(String, String)>) {
    let govde = govde.trim();
    let ad_sonu = govde.find(char::is_whitespace).unwrap_or(govde.len());
    let ad = yerel_ad(&govde[..ad_sonu]);

    let mut attrs = Vec::new();
    let mut kalan = govde[ad_sonu..].trim_start();

    while !kalan.is_empty() {
        let Some(esittir) = kalan.find('=') else { break };
        let ad_parcasi = kalan[..esittir].trim();
        let deger_alani = kalan[esittir + 1..].trim_start();
        let Some(tirnak) = deger_alani.chars().next() else { break };

        let (deger, sonrasi) = if tirnak == '"' || tirnak == '\'' {
            match deger_alani[1..].find(tirnak) {
                Some(kapanis) => (&deger_alani[1..1 + kapanis], &deger_alani[2 + kapanis..]),
                None => break,
            }
        } else {
            // Tırnaksız değer geçerli XML değil ama gerçek dünyada görülüyor.
            let son = deger_alani.find(char::is_whitespace).unwrap_or(deger_alani.len());
            (&deger_alani[..son], &deger_alani[son..])
        };

        if !ad_parcasi.is_empty() {
            attrs.push((yerel_ad(ad_parcasi), decode_entities(deger)));
        }
        kalan = sonrasi.trim_start();
    }

    (ad, attrs)
}

/// Ad alanı önekini atar: `cenc:default_KID` → `default_KID`.
fn yerel_ad(ad: &str) -> String {
    match ad.rsplit_once(':') {
        Some((_, yerel)) => yerel.to_string(),
        None => ad.to_string(),
    }
}

/// XML'in beş öntanımlı varlığı + sayısal başvurular.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }

    let mut cikti = String::with_capacity(s.len());
    let mut kalan = s;
    while let Some(bas) = kalan.find('&') {
        cikti.push_str(&kalan[..bas]);
        let govde = &kalan[bas..];
        // `;` yoksa ya da çok uzaktaysa bu bir varlık değil, düz `&`.
        let Some(nokta) = govde.find(';').filter(|s| *s <= 10) else {
            cikti.push('&');
            kalan = &govde[1..];
            continue;
        };

        let ad = &govde[1..nokta];
        let cozum = match ad {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ad
                .strip_prefix('#')
                .and_then(|sayi| match sayi.strip_prefix(['x', 'X']) {
                    Some(onaltilik) => u32::from_str_radix(onaltilik, 16).ok(),
                    None => sayi.parse::<u32>().ok(),
                })
                .and_then(char::from_u32),
        };

        match cozum {
            Some(c) => {
                cikti.push(c);
                kalan = &govde[nokta + 1..];
            }
            None => {
                cikti.push('&');
                kalan = &govde[1..];
            }
        }
    }
    cikti.push_str(kalan);
    cikti
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basit_agac_okunuyor() {
        let node = parse("<a x=\"1\"><b>metin</b><c/></a>").unwrap();
        assert_eq!(node.name, "a");
        assert_eq!(node.attr("x"), Some("1"));
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].text, "metin");
        assert_eq!(node.children[1].name, "c");
    }

    #[test]
    fn xml_bildirimi_ve_yorum_atlaniyor() {
        let node = parse("<?xml version=\"1.0\"?><!-- not --><a><!-- iç --><b/></a>").unwrap();
        assert_eq!(node.name, "a");
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn ad_alani_oneki_atiliyor() {
        let node = parse("<mpd:MPD xmlns:mpd=\"x\"><mpd:Period/></mpd:MPD>").unwrap();
        assert_eq!(node.name, "MPD");
        assert_eq!(node.children[0].name, "Period");
        // Öznitelik adının öneki de atılıyor.
        assert_eq!(node.attr("mpd"), Some("x"));
    }

    #[test]
    fn tirnak_icindeki_buyuktur_isareti_etiketi_bitirmiyor() {
        let node = parse("<a t=\"1 > 0\"><b/></a>").unwrap();
        assert_eq!(node.attr("t"), Some("1 > 0"));
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn varliklar_cozuluyor() {
        let node = parse("<a u=\"x&amp;y=1\">&lt;iyi&gt; &#65;&#x42;</a>").unwrap();
        assert_eq!(node.attr("u"), Some("x&y=1"));
        assert_eq!(node.text, "<iyi> AB");
    }

    #[test]
    fn cozulemeyen_ampersand_oldugu_gibi_kaliyor() {
        let node = parse("<a>bir & iki &bilinmeyen; üç</a>").unwrap();
        assert_eq!(node.text, "bir & iki &bilinmeyen; üç");
    }

    #[test]
    fn cdata_metin_sayiliyor() {
        let node = parse("<a><![CDATA[<ham> & veri]]></a>").unwrap();
        assert_eq!(node.text, "<ham> & veri");
    }

    #[test]
    fn eslesmeyen_etiket_hata_veriyor() {
        assert!(parse("<a><b></a></b>").is_err());
        assert!(parse("<a><b></b>").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn alt_agac_aramasi() {
        let node = parse("<a><b><c><d/></c></b></a>").unwrap();
        assert!(node.has_descendant("d"));
        assert!(!node.has_descendant("e"));
    }
}
