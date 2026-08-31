//! DASH manifest (`.mpd`) ayrıştırma.
//!
//! HLS'ten farkı: her şey **tek belgede**. Parça adresleri listelenmiyor,
//! bir şablondan üretiliyor (`seg-$Number$.m4s`) — yani parça sayısını
//! süreden ve segment uzunluğundan biz hesaplıyoruz.
//!
//! Desteklenen üç adresleme kipi (DASH'in pratikte kullanılan tamamı):
//!
//! * `SegmentTemplate` + `duration` — eşit uzunlukta parçalar, sayı sayaçtan.
//! * `SegmentTemplate` + `SegmentTimeline` — değişken uzunluk, `$Time$` ile.
//! * `SegmentList` — parçalar tek tek yazılı.
//! * `SegmentBase` — tek dosya; tek parça olarak indiriliyor.
//!
//! DASH'te ses ve video neredeyse her zaman **ayrı** iniyor; birleştirme
//! ffmpeg'in işi (bkz. [`super::mux`]).

use super::{
    url as media_url, ByteRange, Container, MediaManifest, MediaSegment, MediaTrack, Protocol,
    TrackKind,
};
use super::xml::{self, Node};
use crate::download::{DownloadError, Result};

/// Manifesti ayrıştırır.
pub fn parse(text: &str, url: &str) -> Result<MediaManifest> {
    super::ensure_shape(Protocol::Dash, text)?;

    let kok = xml::parse(text).map_err(|e| DownloadError::Manifest(e.0))?;
    if kok.name != "MPD" {
        return Err(DownloadError::Manifest(format!(
            "kök öğe <MPD> değil, <{}>",
            kok.name
        )));
    }

    // DRM her seviyede bildirilebiliyor; tek bir tarama yetiyor.
    if kok.has_descendant("ContentProtection") {
        return Err(DownloadError::Drm(
            "DASH manifesti DRM korumalı (ContentProtection); desteklenmiyor".into(),
        ));
    }

    let canli = kok.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("dynamic"));
    let toplam_sure = kok
        .attr("mediaPresentationDuration")
        .and_then(parse_iso_duration);

    let mpd_base = base_url(&kok, url);

    let mut video: Vec<MediaTrack> = Vec::new();
    let mut audio: Vec<MediaTrack> = Vec::new();
    let mut subtitles: Vec<MediaTrack> = Vec::new();

    for (p_idx, period) in kok.children_named("Period").enumerate() {
        let period_base = base_url(period, &mpd_base);
        let period_sure = period
            .attr("duration")
            .and_then(parse_iso_duration)
            .or(toplam_sure)
            .unwrap_or(0.0);

        for (a_idx, aset) in period.children_named("AdaptationSet").enumerate() {
            let aset_base = base_url(aset, &period_base);
            let tur = icerik_turu(aset);
            if tur.is_none() {
                continue; // Bilinmeyen tür.
            }
            let dil = aset.attr("lang").map(str::to_string);
            // DASH'te varsayılanı `<Role value="main"/>` söylüyor.
            let varsayilan = aset
                .children_named("Role")
                .any(|r| r.attr("value").is_some_and(|v| v.eq_ignore_ascii_case("main")));

            for rep in aset.children_named("Representation") {
                let rep_base = base_url(rep, &aset_base);
                let rep_id = rep.attr("id").unwrap_or("0").to_string();
                let bant: u64 = rep.attr_num("bandwidth").unwrap_or(0);

                let (init, segments) = parcalari_uret(rep, aset, &rep_base, &rep_id, bant, period_sure)?;
                if segments.is_empty() {
                    continue;
                }

                let kind = tur.unwrap();
                let parca = MediaTrack {
                    id: format!("dash:{p_idx}:{a_idx}:{rep_id}"),
                    kind,
                    bandwidth: bant,
                    width: rep.attr_num("width").or_else(|| aset.attr_num("width")),
                    height: rep.attr_num("height").or_else(|| aset.attr_num("height")),
                    codecs: rep
                        .attr("codecs")
                        .or_else(|| aset.attr("codecs"))
                        .map(str::to_string),
                    language: dil.clone(),
                    name: None,
                    // DASH'te tek bir ses grubu kavramı yok; seçim dile göre.
                    group: None,
                    default_track: varsayilan,
                    container: kap(rep, aset),
                    playlist_url: None,
                    init,
                    segments,
                    duration: period_sure,
                };

                match kind {
                    TrackKind::Audio => audio.push(parca),
                    TrackKind::Subtitle => subtitles.push(parca),
                    _ => video.push(parca),
                }
            }
        }
    }

    if video.is_empty() && audio.is_empty() {
        return Err(DownloadError::Manifest(
            "manifestte indirilebilir bir parça yok".into(),
        ));
    }

    // Video hiç yoksa (yalnızca ses yayını) ses parçaları asıl liste oluyor:
    // yoksa arayüzde seçilecek bir şey kalmazdı.
    if video.is_empty() {
        video = std::mem::take(&mut audio);
        for t in video.iter_mut() {
            t.kind = TrackKind::Muxed;
        }
    } else if audio.is_empty() {
        // Ayrı ses yok: video zaten birleşik, ffmpeg'e gerek kalmıyor.
        for t in video.iter_mut() {
            t.kind = TrackKind::Muxed;
        }
    }

    video.sort_by(|a, b| {
        b.height
            .unwrap_or(0)
            .cmp(&a.height.unwrap_or(0))
            .then(b.bandwidth.cmp(&a.bandwidth))
    });
    audio.sort_by_key(|a| std::cmp::Reverse(a.bandwidth));
    subtitles.sort_by_key(|t| !t.default_track);

    Ok(MediaManifest {
        protocol: Protocol::Dash,
        url: url.to_string(),
        live: canli,
        duration: toplam_sure,
        video,
        audio,
        subtitles,
    })
}

/// Öğenin `<BaseURL>`ini üstteki tabana göre çözer. Yoksa taban aynen geçiyor.
fn base_url(node: &Node, ust: &str) -> String {
    match node.child("BaseURL") {
        Some(b) if !b.text.trim().is_empty() => media_url::resolve(ust, b.text.trim()),
        _ => ust.to_string(),
    }
}

fn icerik_turu(aset: &Node) -> Option<TrackKind> {
    let ipucu = aset
        .attr("contentType")
        .or_else(|| aset.attr("mimeType"))
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some(k) = turden(&ipucu) {
        return Some(k);
    }
    // mimeType yalnızca Representation'da yazılmış olabilir.
    let rep = aset.children_named("Representation").next()?;
    let ipucu = rep.attr("mimeType").unwrap_or("").to_ascii_lowercase();
    turden(&ipucu)
}

/// `contentType`/`mimeType` dizgesinden parça türü.
///
/// Altyazı iki farklı şekilde bildiriliyor: düz metin (`text/vtt`,
/// `application/ttml+xml`) ya da fMP4'e sarılmış (`application/mp4` +
/// `codecs="wvtt"`/`"stpp"`). Burada ikisi de altyazı sayılıyor; sarılmış
/// olanı **indirme anında** eleniyor (bkz. [`super::subtitle_format`]), çünkü
/// mp4 kutularını açmak ayrı bir iş ve o olmadan dosya oynatıcıya yaramaz.
fn turden(ipucu: &str) -> Option<TrackKind> {
    if ipucu.starts_with("video") {
        return Some(TrackKind::Video);
    }
    if ipucu.starts_with("audio") {
        return Some(TrackKind::Audio);
    }
    if ipucu.starts_with("text")
        || ipucu.contains("ttml")
        || ipucu.contains("vtt")
        || ipucu.contains("subtitle")
    {
        return Some(TrackKind::Subtitle);
    }
    None
}

fn kap(rep: &Node, aset: &Node) -> Container {
    let tur = rep
        .attr("mimeType")
        .or_else(|| aset.attr("mimeType"))
        .unwrap_or("")
        .to_ascii_lowercase();
    if tur.contains("mp2t") {
        Container::Ts
    } else {
        // DASH'in ezici çoğunluğu fMP4; bilinmeyen türde de en makul tahmin bu.
        Container::Fmp4
    }
}

/// Bir Representation'ın parçalarını üretir.
fn parcalari_uret(
    rep: &Node,
    aset: &Node,
    base: &str,
    rep_id: &str,
    bandwidth: u64,
    sure: f64,
) -> Result<(Option<MediaSegment>, Vec<MediaSegment>)> {
    // Şablon Representation'da da AdaptationSet'te de olabiliyor; yakın olan
    // kazanıyor.
    if let Some(sablon) = rep.child("SegmentTemplate").or_else(|| aset.child("SegmentTemplate")) {
        return Ok(sablondan(sablon, base, rep_id, bandwidth, sure));
    }
    if let Some(liste) = rep.child("SegmentList").or_else(|| aset.child("SegmentList")) {
        return Ok(listeden(liste, base));
    }
    if rep.child("SegmentBase").is_some() || aset.child("SegmentBase").is_some() {
        // Tek dosya: aralık bölmesi yapmadan tamamı iniyor. Bu durumda akış
        // aslında sıradan bir HTTP dosyası ama manifest üzerinden geldiği için
        // aynı boru hattında kalıyor.
        return Ok((
            None,
            vec![MediaSegment { duration: sure, ..MediaSegment::plain(base.to_string()) }],
        ));
    }

    Err(DownloadError::Manifest(format!(
        "Representation {rep_id}: tanınan bir segment tanımı yok"
    )))
}

fn sablondan(
    sablon: &Node,
    base: &str,
    rep_id: &str,
    bandwidth: u64,
    sure: f64,
) -> (Option<MediaSegment>, Vec<MediaSegment>) {
    let timescale: u64 = sablon.attr_num("timescale").unwrap_or(1).max(1);
    let baslangic: u64 = sablon.attr_num("startNumber").unwrap_or(1);

    let init = sablon.attr("initialization").map(|sablon_metni| {
        MediaSegment::plain(media_url::resolve(
            base,
            &genislet(sablon_metni, rep_id, bandwidth, baslangic, 0),
        ))
    });

    let Some(medya) = sablon.attr("media") else {
        return (init, Vec::new());
    };

    let mut segments = Vec::new();
    let mut numara = baslangic;

    if let Some(zaman_cizgisi) = sablon.child("SegmentTimeline") {
        let mut t: u64 = 0;
        for s in zaman_cizgisi.children_named("S") {
            // `t` verilmişse zinciri oradan devam ettir (boşluk olabilir).
            if let Some(bas) = s.attr_num::<u64>("t") {
                t = bas;
            }
            let d: u64 = s.attr_num("d").unwrap_or(0);
            // `r` tekrar sayısı; -1 "periyot sonuna kadar" demek ama sınırsız
            // üretmek yerine yok sayılıyor (canlı yayın kipi zaten reddediliyor).
            let tekrar: i64 = s.attr_num("r").unwrap_or(0);
            let adet = if tekrar < 0 { 1 } else { tekrar + 1 };

            for _ in 0..adet {
                segments.push(MediaSegment {
                    duration: d as f64 / timescale as f64,
                    sequence: numara,
                    ..MediaSegment::plain(media_url::resolve(
                        base,
                        &genislet(medya, rep_id, bandwidth, numara, t),
                    ))
                });
                t += d;
                numara += 1;
            }
        }
        return (init, segments);
    }

    // Zaman çizgisi yok: eşit uzunlukta parçalar.
    let parca_suresi: u64 = sablon.attr_num("duration").unwrap_or(0);
    if parca_suresi == 0 || sure <= 0.0 {
        return (init, segments);
    }
    let saniye = parca_suresi as f64 / timescale as f64;
    let adet = (sure / saniye).ceil() as u64;

    for i in 0..adet {
        let n = baslangic + i;
        segments.push(MediaSegment {
            duration: saniye,
            sequence: n,
            ..MediaSegment::plain(media_url::resolve(
                base,
                &genislet(medya, rep_id, bandwidth, n, i * parca_suresi),
            ))
        });
    }

    (init, segments)
}

fn listeden(liste: &Node, base: &str) -> (Option<MediaSegment>, Vec<MediaSegment>) {
    let timescale: u64 = liste.attr_num("timescale").unwrap_or(1).max(1);
    let sure = liste.attr_num::<u64>("duration").unwrap_or(0) as f64 / timescale as f64;

    let init = liste.child("Initialization").and_then(|i| {
        let adres = i.attr("sourceURL").map(|u| media_url::resolve(base, u));
        let aralik = i.attr("range").and_then(aralik_coz);
        match (adres, aralik) {
            (Some(a), r) => Some(MediaSegment { range: r, ..MediaSegment::plain(a) }),
            // `sourceURL` yoksa init taban adresin bir aralığı.
            (None, Some(r)) => Some(MediaSegment {
                range: Some(r),
                ..MediaSegment::plain(base.to_string())
            }),
            _ => None,
        }
    });

    let segments = liste
        .children_named("SegmentURL")
        .enumerate()
        .map(|(i, s)| MediaSegment {
            // `media` yoksa parça taban adresin bir aralığı demek.
            url: match s.attr("media") {
                Some(m) => media_url::resolve(base, m),
                None => base.to_string(),
            },
            range: s.attr("mediaRange").and_then(aralik_coz),
            duration: sure,
            sequence: i as u64,
            key: None,
        })
        .collect();

    (init, segments)
}

/// DASH aralıkları `bas-son` (ikisi de dâhil) yazılıyor.
fn aralik_coz(deger: &str) -> Option<ByteRange> {
    let (bas, son) = deger.trim().split_once('-')?;
    let bas: u64 = bas.trim().parse().ok()?;
    let son: u64 = son.trim().parse().ok()?;
    if son < bas {
        return None;
    }
    Some(ByteRange { offset: bas, length: son - bas + 1 })
}

/// `$Number%05d$`, `$RepresentationID$`, `$Bandwidth$`, `$Time$`, `$$`.
fn genislet(sablon: &str, rep_id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut cikti = String::with_capacity(sablon.len() + 8);
    let mut kalan = sablon;

    while let Some(bas) = kalan.find('$') {
        cikti.push_str(&kalan[..bas]);
        let govde = &kalan[bas + 1..];

        // `$$` kaçışı: tek `$` üret.
        if let Some(sonrasi) = govde.strip_prefix('$') {
            cikti.push('$');
            kalan = sonrasi;
            continue;
        }

        let Some(kapanis) = govde.find('$') else {
            // Kapanmayan `$`: olduğu gibi bırak.
            cikti.push('$');
            kalan = govde;
            continue;
        };

        let ifade = &govde[..kapanis];
        let (ad, bicim) = match ifade.split_once('%') {
            Some((a, b)) => (a, Some(b)),
            None => (ifade, None),
        };

        let deger = match ad {
            "RepresentationID" => Some(rep_id.to_string()),
            "Bandwidth" => Some(bandwidth.to_string()),
            "Number" => Some(bicimle(number, bicim)),
            "Time" => Some(bicimle(time, bicim)),
            _ => None,
        };

        match deger {
            Some(d) => cikti.push_str(&d),
            // Tanınmayan değişken olduğu gibi kalıyor; adres muhtemelen bozuk
            // ama sessizce yanlış bir adres üretmekten iyi.
            None => cikti.push_str(&format!("${ifade}$")),
        }
        kalan = &govde[kapanis + 1..];
    }

    cikti.push_str(kalan);
    cikti
}

/// `%05d` gibi bir genişlik belirtimini uygular.
fn bicimle(deger: u64, bicim: Option<&str>) -> String {
    let Some(b) = bicim else { return deger.to_string() };
    let rakamlar: String = b.chars().filter(|c| c.is_ascii_digit()).collect();
    match rakamlar.parse::<usize>() {
        Ok(genislik) if genislik > 0 && genislik <= 20 => format!("{deger:0genislik$}"),
        _ => deger.to_string(),
    }
}

/// ISO 8601 süresi (`PT1H2M3.5S`, `P1DT30M`) → saniye.
pub fn parse_iso_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let govde = s.strip_prefix('P').or_else(|| s.strip_prefix('p'))?;

    let (tarih, saat) = match govde.find(['T', 't']) {
        Some(i) => (&govde[..i], &govde[i + 1..]),
        None => (govde, ""),
    };

    let mut toplam = 0.0f64;
    let mut sayi = String::new();

    for c in tarih.chars() {
        if c.is_ascii_digit() || c == '.' {
            sayi.push(c);
            continue;
        }
        let deger: f64 = sayi.parse().unwrap_or(0.0);
        sayi.clear();
        toplam += match c {
            'Y' | 'y' => deger * 365.0 * 86_400.0,
            'M' | 'm' => deger * 30.0 * 86_400.0,
            'W' | 'w' => deger * 7.0 * 86_400.0,
            'D' | 'd' => deger * 86_400.0,
            _ => return None,
        };
    }

    sayi.clear();
    for c in saat.chars() {
        if c.is_ascii_digit() || c == '.' {
            sayi.push(c);
            continue;
        }
        let deger: f64 = sayi.parse().unwrap_or(0.0);
        sayi.clear();
        toplam += match c {
            'H' | 'h' => deger * 3600.0,
            'M' | 'm' => deger * 60.0,
            'S' | 's' => deger,
            _ => return None,
        };
    }

    Some(toplam)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SABLONLU: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
  <BaseURL>https://cdn.example.com/vod/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <SegmentTemplate initialization="init-$RepresentationID$.mp4"
                       media="seg-$RepresentationID$-$Number%03d$.m4s"
                       startNumber="1" duration="4" timescale="1"/>
      <Representation id="v360" bandwidth="800000" width="640" height="360" codecs="avc1.42c01e"/>
      <Representation id="v1080" bandwidth="5000000" width="1920" height="1080"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" lang="tr">
      <SegmentTemplate initialization="a-init.mp4" media="a-$Number$.m4s" startNumber="1" duration="4" timescale="1"/>
      <Representation id="a0" bandwidth="128000"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    const ALTYAZILI: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
  <BaseURL>https://cdn.example.com/vod/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <SegmentTemplate media="v-$Number$.m4s" startNumber="1" duration="4" timescale="1"/>
      <Representation id="v0" bandwidth="800000" width="640" height="360"/>
    </AdaptationSet>
    <AdaptationSet contentType="text" mimeType="text/vtt" lang="en">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <Representation id="s_en" bandwidth="1000">
        <BaseURL>subs/en.vtt</BaseURL>
        <SegmentBase/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="text" mimeType="application/mp4" codecs="wvtt" lang="tr">
      <SegmentTemplate media="s-tr-$Number$.m4s" startNumber="1" duration="4" timescale="1"/>
      <Representation id="s_tr" bandwidth="1000"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    #[test]
    fn altyazi_adaptation_set_ayri_listede() {
        let m = parse(ALTYAZILI, "https://x/y/manifest.mpd").unwrap();
        assert_eq!(m.video.len(), 1);
        assert!(m.audio.is_empty());
        assert_eq!(m.subtitles.len(), 2);

        // `Role value="main"` olan başa alınıyor.
        assert_eq!(m.subtitles[0].language.as_deref(), Some("en"));
        assert!(m.subtitles[0].default_track);
        assert_eq!(m.subtitles[0].kind, TrackKind::Subtitle);
        assert_eq!(m.subtitles[0].segments[0].url, "https://cdn.example.com/vod/subs/en.vtt");

        // Ayrı ses yok: video birleşik sayılıyor, ffmpeg şartı doğmuyor.
        assert_eq!(m.video[0].kind, TrackKind::Muxed);
    }

    #[test]
    fn fmp4e_sarili_altyazi_indirilebilir_sayilmiyor() {
        let m = parse(ALTYAZILI, "https://x/y/manifest.mpd").unwrap();
        let wvtt = m.subtitles.iter().find(|t| t.language.as_deref() == Some("tr")).unwrap();
        // Ayrıştırma listeliyor ama indirme elemesi burada:
        // `codecs="wvtt"` mp4 kutularının açılmasını gerektiriyor.
        assert!(!super::super::subtitle_downloadable(wvtt));
        assert!(super::super::subtitle_downloadable(&m.subtitles[0]));
    }

    #[test]
    fn sablonlu_manifest_okunuyor() {
        let m = parse(SABLONLU, "https://x/y/manifest.mpd").unwrap();
        assert_eq!(m.protocol, Protocol::Dash);
        assert!(!m.live);
        assert_eq!(m.duration, Some(20.0));

        assert_eq!(m.video.len(), 2);
        // En iyi kalite başta.
        assert_eq!(m.video[0].height, Some(1080));
        assert_eq!(m.video[0].kind, TrackKind::Video);
        assert_eq!(m.audio.len(), 1);
        assert_eq!(m.audio[0].language.as_deref(), Some("tr"));

        let v = &m.video[1]; // 360p
        assert_eq!(v.init.as_ref().unwrap().url, "https://cdn.example.com/vod/init-v360.mp4");
        assert_eq!(v.segments.len(), 5); // 20 s / 4 s
        assert_eq!(v.segments[0].url, "https://cdn.example.com/vod/seg-v360-001.m4s");
        assert_eq!(v.segments[4].url, "https://cdn.example.com/vod/seg-v360-005.m4s");
        assert_eq!(v.container, Container::Fmp4);
    }

    #[test]
    fn zaman_cizgisi_tekrarlari_aciliyor() {
        let metin = r#"<MPD mediaPresentationDuration="PT12S"><Period>
          <AdaptationSet contentType="video" mimeType="video/mp4">
            <SegmentTemplate media="s-$Time$.m4s" timescale="1000">
              <SegmentTimeline>
                <S t="0" d="2000" r="2"/>
                <S d="1500"/>
              </SegmentTimeline>
            </SegmentTemplate>
            <Representation id="v" bandwidth="1000" width="640" height="360"/>
          </AdaptationSet></Period></MPD>"#;
        let m = parse(metin, "https://c/m.mpd").unwrap();
        let s = &m.video[0].segments;
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].url, "https://c/s-0.m4s");
        assert_eq!(s[1].url, "https://c/s-2000.m4s");
        assert_eq!(s[2].url, "https://c/s-4000.m4s");
        assert_eq!(s[3].url, "https://c/s-6000.m4s");
        assert!((s[3].duration - 1.5).abs() < 0.001);
    }

    #[test]
    fn segment_list_okunuyor() {
        let metin = r#"<MPD><Period><AdaptationSet mimeType="video/mp4">
          <Representation id="v" bandwidth="1000" height="480">
            <SegmentList duration="4" timescale="1">
              <Initialization sourceURL="init.mp4"/>
              <SegmentURL media="s1.m4s"/>
              <SegmentURL media="s2.m4s" mediaRange="100-199"/>
            </SegmentList>
          </Representation></AdaptationSet></Period></MPD>"#;
        let m = parse(metin, "https://c/v/m.mpd").unwrap();
        let t = &m.video[0];
        assert_eq!(t.init.as_ref().unwrap().url, "https://c/v/init.mp4");
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[1].range.unwrap(), ByteRange { offset: 100, length: 100 });
        // Ayrı ses yok: parça birleşik sayılıyor.
        assert_eq!(t.kind, TrackKind::Muxed);
    }

    #[test]
    fn segment_base_tek_parca_uretiyor() {
        let metin = r#"<MPD mediaPresentationDuration="PT30S"><Period><AdaptationSet mimeType="video/mp4">
          <Representation id="v" bandwidth="1000" height="720">
            <BaseURL>film.mp4</BaseURL>
            <SegmentBase indexRange="0-800"><Initialization range="0-800"/></SegmentBase>
          </Representation></AdaptationSet></Period></MPD>"#;
        let m = parse(metin, "https://c/v/m.mpd").unwrap();
        assert_eq!(m.video[0].segments.len(), 1);
        assert_eq!(m.video[0].segments[0].url, "https://c/v/film.mp4");
    }

    #[test]
    fn drm_reddediliyor() {
        let metin = r#"<MPD><Period><AdaptationSet mimeType="video/mp4">
          <ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>
          <Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(matches!(parse(metin, "https://c/m.mpd").unwrap_err(), DownloadError::Drm(_)));
    }

    #[test]
    fn canli_yayin_isaretleniyor() {
        let metin = r#"<MPD type="dynamic" mediaPresentationDuration="PT8S"><Period>
          <AdaptationSet mimeType="video/mp4">
          <SegmentTemplate media="s$Number$.m4s" duration="4" timescale="1"/>
          <Representation id="v" bandwidth="1" height="360"/>
          </AdaptationSet></Period></MPD>"#;
        assert!(parse(metin, "https://c/m.mpd").unwrap().live);
    }

    #[test]
    fn iso_suresi_cozuluyor() {
        assert_eq!(parse_iso_duration("PT30S"), Some(30.0));
        assert_eq!(parse_iso_duration("PT1H2M3S"), Some(3723.0));
        assert_eq!(parse_iso_duration("PT0H10M0.500S"), Some(600.5));
        assert_eq!(parse_iso_duration("P1DT2H"), Some(93_600.0));
        assert_eq!(parse_iso_duration("saçma"), None);
    }

    #[test]
    fn sablon_degiskenleri_genisliyor() {
        assert_eq!(genislet("s-$Number$.ts", "v", 0, 7, 0), "s-7.ts");
        assert_eq!(genislet("s-$Number%05d$.ts", "v", 0, 7, 0), "s-00007.ts");
        assert_eq!(genislet("$RepresentationID$/$Bandwidth$/$Time$", "v1", 900, 3, 42), "v1/900/42");
        // `$$` tek dolara iniyor, tanınmayan değişken olduğu gibi kalıyor.
        assert_eq!(genislet("a$$b$Yok$", "v", 0, 1, 0), "a$b$Yok$");
    }

    #[test]
    fn goreli_base_url_zinciri_cozuluyor() {
        let metin = r#"<MPD mediaPresentationDuration="PT4S"><BaseURL>https://a.com/x/</BaseURL>
          <Period><BaseURL>p1/</BaseURL><AdaptationSet mimeType="video/mp4">
            <SegmentTemplate media="s$Number$.m4s" duration="4" timescale="1"/>
            <Representation id="v" bandwidth="1" height="360"><BaseURL>hi/</BaseURL></Representation>
          </AdaptationSet></Period></MPD>"#;
        let m = parse(metin, "https://c/m.mpd").unwrap();
        assert_eq!(m.video[0].segments[0].url, "https://a.com/x/p1/hi/s1.m4s");
    }

    #[test]
    fn bos_manifest_hata_veriyor() {
        assert!(parse("<MPD></MPD>", "https://c/m.mpd").is_err());
        assert!(parse("#EXTM3U", "https://c/m.mpd").is_err());
    }
}
