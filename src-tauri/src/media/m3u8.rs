//! HLS playlist ayrıştırma (RFC 8216).
//!
//! İki tür playlist var ve ikisi de `.m3u8` uzantısını taşıyor:
//!
//! * **Master playlist** — kalite seçeneklerini (`#EXT-X-STREAM-INF`) ve ayrı
//!   ses/altyazı parçalarını (`#EXT-X-MEDIA`) listeler. Segment içermez.
//! * **Medya playlist** — asıl parçaları (`#EXTINF` + adres) listeler.
//!
//! Ayrım tag'e bakılarak yapılıyor, uzantıya değil: aynı adres bir sitede
//! master, başkasında medya playlist oluyor.

use std::collections::HashMap;

use super::{
    url as media_url, ByteRange, Container, MediaManifest, MediaSegment, MediaTrack, Protocol,
    SegmentKey, TrackKind,
};
use crate::download::{DownloadError, Result};

/// Bir medya playlistinden çıkan her şey.
#[derive(Debug, Clone, Default)]
pub struct MediaPlaylist {
    pub segments: Vec<MediaSegment>,
    /// fMP4 akışlarda ilk yazılacak başlangıç parçası (`#EXT-X-MAP`).
    pub init: Option<MediaSegment>,
    pub live: bool,
    pub duration: f64,
    pub container: Container,
}

/// Playlist metnini ayrıştırır. Master ise kalite listesi, medya playlist ise
/// tek bir birleşik parça dönüyor.
pub fn parse(text: &str, url: &str) -> Result<MediaManifest> {
    super::ensure_shape(Protocol::Hls, text)?;

    if text.contains("#EXT-X-STREAM-INF") {
        return parse_master(text, url);
    }

    let playlist = parse_media(text, url)?;
    let parca = MediaTrack {
        id: url.to_string(),
        kind: TrackKind::Muxed,
        container: playlist.container,
        playlist_url: Some(url.to_string()),
        init: playlist.init,
        duration: playlist.duration,
        // Bant genişliği bilinmiyor: master playlist yok. Boyut tahmini
        // parçalar inmeye başlayınca gerçek ölçümle doluyor.
        bandwidth: 0,
        segments: playlist.segments,
        ..MediaTrack::default()
    };

    Ok(MediaManifest {
        protocol: Protocol::Hls,
        url: url.to_string(),
        live: playlist.live,
        duration: Some(parca.duration),
        video: vec![parca],
        audio: Vec::new(),
        // Medya playlisti tek başına altyazı bilmiyor; onlar master playlistte.
        subtitles: Vec::new(),
    })
}

/// Master playlist: kalite seçenekleri ve ayrı ses parçaları.
fn parse_master(text: &str, url: &str) -> Result<MediaManifest> {
    let mut video: Vec<MediaTrack> = Vec::new();
    let mut audio: Vec<MediaTrack> = Vec::new();
    let mut subtitles: Vec<MediaTrack> = Vec::new();
    // Bir sonraki adres satırını bekleyen `#EXT-X-STREAM-INF` öznitelikleri.
    let mut bekleyen: Option<Vec<(String, String)>> = None;
    // URI'si olmayan ses grupları: ses videonun içinde demek.
    let mut ayri_ses_gruplari: Vec<String> = Vec::new();

    for satir in text.lines() {
        let satir = satir.trim();
        if satir.is_empty() {
            continue;
        }

        if let Some(govde) = satir.strip_prefix("#EXT-X-MEDIA:") {
            let attrs = parse_attrs(govde);
            let tur = attr(&attrs, "TYPE").unwrap_or_default().to_ascii_uppercase();
            // `CLOSED-CAPTIONS` (CEA-608/708) video akışının **içinde** taşınıyor;
            // ayrı indirilebilecek bir adresi yok. `URI` alanı da bu yüzden boş
            // geliyor ve aşağıdaki kontrole takılıyor.
            if tur != "AUDIO" && tur != "SUBTITLES" {
                continue;
            }
            let Some(uri) = attr(&attrs, "URI") else {
                // URI yoksa ses zaten video parçasının içinde.
                continue;
            };
            if tur == "SUBTITLES" {
                let adres = media_url::resolve(url, &uri);
                subtitles.push(MediaTrack {
                    id: adres.clone(),
                    kind: TrackKind::Subtitle,
                    language: attr(&attrs, "LANGUAGE"),
                    name: attr(&attrs, "NAME"),
                    group: attr(&attrs, "GROUP-ID"),
                    // Manifest sırası sağlayıcının varsayılanını yansıtmıyor;
                    // `DEFAULT=YES` yazan parça öne alınıyor (aşağıda).
                    default_track: attr(&attrs, "DEFAULT")
                        .is_some_and(|d| d.eq_ignore_ascii_case("YES")),
                    playlist_url: Some(adres),
                    ..MediaTrack::default()
                });
                continue;
            }
            let grup = attr(&attrs, "GROUP-ID");
            if let Some(g) = &grup {
                if !ayri_ses_gruplari.contains(g) {
                    ayri_ses_gruplari.push(g.clone());
                }
            }
            let adres = media_url::resolve(url, &uri);
            audio.push(MediaTrack {
                id: adres.clone(),
                kind: TrackKind::Audio,
                language: attr(&attrs, "LANGUAGE"),
                name: attr(&attrs, "NAME"),
                group: grup,
                default_track: attr(&attrs, "DEFAULT")
                    .is_some_and(|d| d.eq_ignore_ascii_case("YES")),
                playlist_url: Some(adres),
                ..MediaTrack::default()
            });
            continue;
        }

        if let Some(govde) = satir.strip_prefix("#EXT-X-SESSION-KEY:") {
            // Oturum anahtarı da DRM taşıyabiliyor; parçalara bakmadan önce
            // burada yakalanıyor.
            anahtari_coz(&parse_attrs(govde), url)?;
            continue;
        }

        if let Some(govde) = satir.strip_prefix("#EXT-X-STREAM-INF:") {
            bekleyen = Some(parse_attrs(govde));
            continue;
        }

        // `#EXT-X-I-FRAME-STREAM-INF` kendi URI'sini öznitelik olarak taşıyor
        // ve ardından adres satırı gelmiyor; hızlı sarma için, indirilecek bir
        // akış değil.
        if satir.starts_with('#') {
            continue;
        }

        let Some(attrs) = bekleyen.take() else {
            continue; // Başıboş adres satırı.
        };

        let adres = media_url::resolve(url, satir);
        let (genislik, yukseklik) = cozunurluk(attr(&attrs, "RESOLUTION").as_deref());
        video.push(MediaTrack {
            id: adres.clone(),
            // Ses grubunun ayrı bir URI'si varsa bu parça yalnızca video.
            // Karar bütün satırlar okunduktan sonra düzeltiliyor.
            kind: TrackKind::Video,
            bandwidth: attr(&attrs, "AVERAGE-BANDWIDTH")
                .or_else(|| attr(&attrs, "BANDWIDTH"))
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            width: genislik,
            height: yukseklik,
            codecs: attr(&attrs, "CODECS"),
            group: attr(&attrs, "AUDIO"),
            playlist_url: Some(adres),
            ..MediaTrack::default()
        });
    }

    if video.is_empty() {
        return Err(DownloadError::Manifest(
            "master playlistte hiç kalite seçeneği yok".into(),
        ));
    }

    // Ses grubu ayrı URI taşımıyorsa parça zaten birleşik.
    for parca in video.iter_mut() {
        let ayri = parca
            .group
            .as_ref()
            .is_some_and(|g| ayri_ses_gruplari.contains(g));
        if !ayri {
            parca.kind = TrackKind::Muxed;
            parca.group = None;
        }
    }

    // `DEFAULT=YES` olan altyazı başa: seçimin varsayılanı "listenin ilki" ve
    // sağlayıcı hangisini istediğini bu öznitelikle söylüyor. Sıralama kararlı,
    // yani aynı gruptaki geri kalanın manifest sırası bozulmuyor.
    subtitles.sort_by_key(|t| !t.default_track);

    // En iyi kalite başa: seçimin varsayılanı "listenin ilki".
    video.sort_by(|a, b| {
        b.height
            .unwrap_or(0)
            .cmp(&a.height.unwrap_or(0))
            .then(b.bandwidth.cmp(&a.bandwidth))
    });

    Ok(MediaManifest {
        protocol: Protocol::Hls,
        url: url.to_string(),
        // Master playlist canlı olup olmadığını söylemiyor; medya playlist
        // indirilince öğrenilecek.
        live: false,
        duration: None,
        video,
        audio,
        subtitles,
    })
}

/// Medya playlist: asıl parçalar.
pub fn parse_media(text: &str, url: &str) -> Result<MediaPlaylist> {
    super::ensure_shape(Protocol::Hls, text)?;

    let mut cikti = MediaPlaylist::default();
    let mut anahtar: Option<SegmentKey> = None;
    let mut sira: u64 = 0;
    let mut sonraki_sure = 0.0f64;
    let mut bekleyen_aralik: Option<(u64, Option<u64>)> = None;
    // Aynı dosyaya ait bir önceki byte aralığının bittiği yer — offset'siz
    // `#EXT-X-BYTERANGE` bunun devamı demek.
    let mut son_offset: HashMap<String, u64> = HashMap::new();
    let mut endlist = false;
    let mut vod = false;

    for satir in text.lines() {
        let satir = satir.trim();
        if satir.is_empty() {
            continue;
        }

        if let Some(govde) = satir.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sira = govde.trim().parse().unwrap_or(0);
            continue;
        }
        if let Some(govde) = satir.strip_prefix("#EXT-X-PLAYLIST-TYPE:") {
            vod = govde.trim().eq_ignore_ascii_case("VOD");
            continue;
        }
        if satir.starts_with("#EXT-X-ENDLIST") {
            endlist = true;
            continue;
        }
        if let Some(govde) = satir.strip_prefix("#EXT-X-KEY:") {
            anahtar = anahtari_coz(&parse_attrs(govde), url)?;
            continue;
        }
        if let Some(govde) = satir.strip_prefix("#EXT-X-MAP:") {
            let attrs = parse_attrs(govde);
            let Some(uri) = attr(&attrs, "URI") else { continue };
            let adres = media_url::resolve(url, &uri);
            cikti.init = Some(MediaSegment {
                url: adres,
                range: attr(&attrs, "BYTERANGE")
                    .as_deref()
                    .and_then(byterange_coz)
                    .map(|(uzunluk, offset)| ByteRange {
                        offset: offset.unwrap_or(0),
                        length: uzunluk,
                    }),
                duration: 0.0,
                sequence: 0,
                // Başlangıç parçası da o anki anahtarla şifreli olabiliyor.
                key: anahtar.clone(),
            });
            continue;
        }
        if let Some(govde) = satir.strip_prefix("#EXTINF:") {
            sonraki_sure = govde
                .split(',')
                .next()
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0.0);
            continue;
        }
        if let Some(govde) = satir.strip_prefix("#EXT-X-BYTERANGE:") {
            bekleyen_aralik = byterange_coz(govde.trim());
            continue;
        }
        if satir.starts_with('#') {
            continue;
        }

        let adres = media_url::resolve(url, satir);
        let aralik = bekleyen_aralik.take().map(|(uzunluk, offset)| {
            let bas = offset.unwrap_or_else(|| son_offset.get(&adres).copied().unwrap_or(0));
            son_offset.insert(adres.clone(), bas + uzunluk);
            ByteRange { offset: bas, length: uzunluk }
        });

        cikti.duration += sonraki_sure;
        cikti.segments.push(MediaSegment {
            url: adres,
            range: aralik,
            duration: sonraki_sure,
            sequence: sira,
            key: anahtar.clone(),
        });
        sira += 1;
        sonraki_sure = 0.0;
    }

    if cikti.segments.is_empty() {
        return Err(DownloadError::Manifest("playlistte hiç parça yok".into()));
    }

    cikti.live = !endlist && !vod;
    cikti.container = kap_tahmini(&cikti);
    Ok(cikti)
}

/// Parçaların kabını tahmin eder.
///
/// `#EXT-X-MAP` varsa akış kesin fMP4: başlangıç parçası yalnızca orada var.
/// Yoksa ilk parçanın uzantısına bakılıyor; tanınmayan uzantı MPEG-TS sayılıyor
/// çünkü HLS'in varsayılanı o.
fn kap_tahmini(playlist: &MediaPlaylist) -> Container {
    if playlist.init.is_some() {
        return Container::Fmp4;
    }
    let Some(ilk) = playlist.segments.first() else {
        return Container::Ts;
    };
    let yol = ilk.url.split(['?', '#']).next().unwrap_or(&ilk.url).to_ascii_lowercase();
    if yol.ends_with(".mp4") || yol.ends_with(".m4s") || yol.ends_with(".m4a") || yol.ends_with(".cmfv") || yol.ends_with(".cmfa") {
        Container::Fmp4
    } else if yol.ends_with(".aac") || yol.ends_with(".mp3") || yol.ends_with(".ac3") {
        Container::RawAudio
    } else {
        Container::Ts
    }
}

/// `#EXT-X-KEY` / `#EXT-X-SESSION-KEY` özniteliklerini çözer.
///
/// DRM burada reddediliyor — indirme başlamadan, ayrıştırma anında. Yarım bir
/// dosya indirip sonunda "çözülemedi" demek kullanıcının zamanını ve
/// bant genişliğini boşa harcamak olurdu.
fn anahtari_coz(attrs: &[(String, String)], base: &str) -> Result<Option<SegmentKey>> {
    let yontem = attr(attrs, "METHOD").unwrap_or_else(|| "NONE".into()).to_ascii_uppercase();

    match yontem.as_str() {
        "NONE" => return Ok(None),
        "AES-128" => {}
        diger => {
            return Err(DownloadError::Drm(format!(
                "{diger} ile korunan yayınlar desteklenmiyor"
            )))
        }
    }

    // `KEYFORMAT` verilmemişse `identity` demek (RFC 8216 §4.3.2.4). Başka bir
    // değer, anahtarın lisans sunucusundan alındığı anlamına geliyor.
    let bicim = attr(attrs, "KEYFORMAT").unwrap_or_else(|| "identity".into());
    if !bicim.eq_ignore_ascii_case("identity") {
        return Err(DownloadError::Drm(format!(
            "anahtar biçimi \"{bicim}\" bir DRM sistemine ait; desteklenmiyor"
        )));
    }

    let Some(uri) = attr(attrs, "URI") else {
        return Err(DownloadError::Manifest("AES-128 anahtarının URI'si yok".into()));
    };

    Ok(Some(SegmentKey {
        uri: media_url::resolve(base, &uri),
        iv: attr(attrs, "IV").as_deref().and_then(super::crypt::parse_hex16),
    }))
}

/// `1920x1080` → (1920, 1080).
fn cozunurluk(deger: Option<&str>) -> (Option<u32>, Option<u32>) {
    let Some(d) = deger else { return (None, None) };
    match d.trim().split_once(['x', 'X']) {
        Some((w, h)) => (w.trim().parse().ok(), h.trim().parse().ok()),
        None => (None, None),
    }
}

/// `<uzunluk>[@<offset>]` → (uzunluk, offset).
fn byterange_coz(deger: &str) -> Option<(u64, Option<u64>)> {
    let deger = deger.trim();
    match deger.split_once('@') {
        Some((uzunluk, offset)) => {
            Some((uzunluk.trim().parse().ok()?, offset.trim().parse().ok()))
        }
        None => Some((deger.parse().ok()?, None)),
    }
}

/// `A=1,B="x,y",C=3` → [(A,1),(B,x,y),(C,3)].
///
/// Elle yazıldı çünkü tırnak içindeki virgül ayırıcı sayılmıyor ve
/// `CODECS="avc1.4d401f,mp4a.40.2"` bunun tam örneği; naif bir `split(',')`
/// kodek listesini ikiye bölerdi.
fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut cikti = Vec::new();
    let mut kalan = s.trim();

    while !kalan.is_empty() {
        let Some(esittir) = kalan.find('=') else { break };
        let ad = kalan[..esittir].trim().to_ascii_uppercase();
        let deger_alani = &kalan[esittir + 1..];

        let (deger, sonrasi) = match deger_alani.strip_prefix('"') {
            Some(govde) => match govde.find('"') {
                Some(kapanis) => (&govde[..kapanis], &govde[kapanis + 1..]),
                None => (govde, ""),
            },
            None => match deger_alani.find(',') {
                Some(virgul) => (&deger_alani[..virgul], &deger_alani[virgul..]),
                None => (deger_alani, ""),
            },
        };

        if !ad.is_empty() {
            cikti.push((ad, deger.trim().to_string()));
        }
        kalan = sonrasi.trim_start().trim_start_matches(',').trim_start();
    }

    cikti
}

fn attr(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(ad, _)| ad == name)
        .map(|(_, deger)| deger.clone())
        .filter(|d| !d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "\
#EXTM3U
#EXT-X-VERSION:4
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"Türkçe\",LANGUAGE=\"tr\",DEFAULT=YES,URI=\"audio/tr.m3u8\"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"English\",LANGUAGE=\"en\",URI=\"audio/en.m3u8\"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub\",NAME=\"tr\",URI=\"sub/tr.m3u8\"
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.42c01e,mp4a.40.2\",AUDIO=\"aac\"
360p/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000000,AVERAGE-BANDWIDTH=4500000,RESOLUTION=1920x1080,AUDIO=\"aac\"
1080p/index.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=100000,URI=\"iframe.m3u8\"
";

    const MEDYA: &str = "\
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:9.009,
seg0.ts
#EXTINF:9.009,başlık
seg1.ts
#EXTINF:3.003,
https://baska.cdn/seg2.ts
#EXT-X-ENDLIST
";

    #[test]
    fn master_kalite_ve_ses_okunuyor() {
        let m = parse(MASTER, "https://cdn/x/master.m3u8").unwrap();
        assert_eq!(m.protocol, Protocol::Hls);
        assert_eq!(m.video.len(), 2);
        // En iyi kalite başta.
        assert_eq!(m.video[0].height, Some(1080));
        assert_eq!(m.video[0].id, "https://cdn/x/1080p/index.m3u8");
        // AVERAGE-BANDWIDTH varsa o kullanılıyor: tahmin ortalamayla daha doğru.
        assert_eq!(m.video[0].bandwidth, 4_500_000);
        assert_eq!(m.video[1].width, Some(640));
        assert_eq!(m.video[1].codecs.as_deref(), Some("avc1.42c01e,mp4a.40.2"));

        // Ayrı ses grubu var: video parçaları yalnızca video.
        assert_eq!(m.video[0].kind, TrackKind::Video);
        assert_eq!(m.video[0].group.as_deref(), Some("aac"));

        assert_eq!(m.audio.len(), 2);
        assert_eq!(m.audio[0].language.as_deref(), Some("tr"));
        assert_eq!(m.audio[0].id, "https://cdn/x/audio/tr.m3u8");
        assert!(m.audio[0].default_track, "DEFAULT=YES okunmalı");
        assert!(!m.audio[1].default_track);
    }

    #[test]
    fn altyazi_parcalari_ayri_listede() {
        let m = parse(MASTER, "https://cdn/x/master.m3u8").unwrap();
        assert_eq!(m.subtitles.len(), 1);
        assert_eq!(m.subtitles[0].kind, TrackKind::Subtitle);
        assert_eq!(m.subtitles[0].id, "https://cdn/x/sub/tr.m3u8");
        assert_eq!(m.subtitles[0].name.as_deref(), Some("tr"));
        // Altyazı ne video ne ses listesine sızmamalı.
        assert!(!m.video.iter().any(|t| t.id.contains("/sub/")));
        assert!(!m.audio.iter().any(|t| t.id.contains("/sub/")));
    }

    #[test]
    fn varsayilan_altyazi_basa_aliniyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"English\",LANGUAGE=\"en\",URI=\"en.m3u8\"\n\
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"Türkçe\",LANGUAGE=\"tr\",DEFAULT=YES,URI=\"tr.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=640x360\n\
            v.m3u8\n";
        let m = parse(metin, "https://cdn/master.m3u8").unwrap();
        assert_eq!(m.subtitles.len(), 2);
        assert_eq!(m.subtitles[0].language.as_deref(), Some("tr"), "DEFAULT=YES başa");
        assert_eq!(m.subtitles[1].language.as_deref(), Some("en"));
    }

    #[test]
    fn urisiz_altyazi_atlaniyor() {
        // `CLOSED-CAPTIONS` video akışının içinde taşınıyor; ayrı indirilecek
        // bir adresi olmadığı için listede yeri yok.
        let metin = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"cc\",NAME=\"CC1\",INSTREAM-ID=\"CC1\"\n\
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"yok\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=640x360\n\
            v.m3u8\n";
        let m = parse(metin, "https://cdn/master.m3u8").unwrap();
        assert!(m.subtitles.is_empty());
    }

    #[test]
    fn iframe_akisi_kalite_sayilmiyor() {
        let m = parse(MASTER, "https://cdn/x/master.m3u8").unwrap();
        assert!(!m.video.iter().any(|t| t.id.contains("iframe")));
    }

    #[test]
    fn urisiz_ses_grubu_parcayi_birlesik_birakiyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"main\",DEFAULT=YES\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=640x360,AUDIO=\"a\"\n\
            v.m3u8\n";
        let m = parse(metin, "https://cdn/master.m3u8").unwrap();
        assert_eq!(m.video[0].kind, TrackKind::Muxed);
        assert!(m.video[0].group.is_none());
        assert!(m.audio.is_empty());
    }

    #[test]
    fn medya_playlist_parcalari_okunuyor() {
        let m = parse(MEDYA, "https://cdn/x/720p/list.m3u8").unwrap();
        assert_eq!(m.video.len(), 1);
        assert!(!m.live);
        let t = &m.video[0];
        assert_eq!(t.kind, TrackKind::Muxed);
        assert_eq!(t.segments.len(), 3);
        assert_eq!(t.segments[0].url, "https://cdn/x/720p/seg0.ts");
        assert_eq!(t.segments[2].url, "https://baska.cdn/seg2.ts");
        assert_eq!(t.segments[1].sequence, 1);
        assert!((t.duration - 21.021).abs() < 0.001);
        assert_eq!(t.container, Container::Ts);
    }

    #[test]
    fn medya_sirasi_iv_icin_taniniyor() {
        let metin = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:97\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n#EXT-X-ENDLIST\n";
        let p = parse_media(metin, "https://c/l.m3u8").unwrap();
        assert_eq!(p.segments[0].sequence, 97);
        assert_eq!(p.segments[1].sequence, 98);
    }

    #[test]
    fn endlist_yoksa_canli_sayiliyor() {
        let metin = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n";
        assert!(parse_media(metin, "https://c/l.m3u8").unwrap().live);
    }

    #[test]
    fn vod_isareti_endlist_eksigini_kapatiyor() {
        let metin = "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:6,\na.ts\n";
        assert!(!parse_media(metin, "https://c/l.m3u8").unwrap().live);
    }

    #[test]
    fn fmp4_baslangic_parcasi_okunuyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-MAP:URI=\"init.mp4\"\n\
            #EXTINF:4,\n0.m4s\n#EXTINF:4,\n1.m4s\n#EXT-X-ENDLIST\n";
        let p = parse_media(metin, "https://c/v/l.m3u8").unwrap();
        assert_eq!(p.init.unwrap().url, "https://c/v/init.mp4");
        assert_eq!(p.container, Container::Fmp4);
    }

    #[test]
    fn byterange_zinciri_cozuluyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"800@0\"\n\
            #EXTINF:4,\n#EXT-X-BYTERANGE:1000@800\nall.mp4\n\
            #EXTINF:4,\n#EXT-X-BYTERANGE:2000\nall.mp4\n\
            #EXT-X-ENDLIST\n";
        let p = parse_media(metin, "https://c/v/l.m3u8").unwrap();
        assert_eq!(p.init.unwrap().range.unwrap(), ByteRange { offset: 0, length: 800 });
        assert_eq!(p.segments[0].range.unwrap(), ByteRange { offset: 800, length: 1000 });
        // Offset'siz aralık bir öncekinin devamı.
        assert_eq!(p.segments[1].range.unwrap(), ByteRange { offset: 1800, length: 2000 });
    }

    #[test]
    fn aes128_anahtari_okunuyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x00000000000000000000000000000009\n\
            #EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
        let p = parse_media(metin, "https://c/v/l.m3u8").unwrap();
        let k = p.segments[0].key.clone().unwrap();
        assert_eq!(k.uri, "https://c/v/key.bin");
        assert_eq!(k.iv.unwrap()[15], 9);
    }

    #[test]
    fn method_none_sifrelemeyi_kapatiyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.ts\n\
            #EXT-X-KEY:METHOD=NONE\n#EXTINF:4,\nb.ts\n#EXT-X-ENDLIST\n";
        let p = parse_media(metin, "https://c/l.m3u8").unwrap();
        assert!(p.segments[0].key.is_some());
        assert!(p.segments[1].key.is_none());
    }

    #[test]
    fn sample_aes_reddediliyor() {
        let metin = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:4,\na.ts\n";
        let hata = parse_media(metin, "https://c/l.m3u8").unwrap_err();
        assert!(matches!(hata, DownloadError::Drm(_)), "{hata}");
    }

    #[test]
    fn drm_anahtar_bicimi_reddediliyor() {
        let metin = "#EXTM3U\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"https://lis/x\",KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"\n\
            #EXTINF:4,\na.ts\n";
        assert!(matches!(
            parse_media(metin, "https://c/l.m3u8").unwrap_err(),
            DownloadError::Drm(_)
        ));
    }

    #[test]
    fn bos_playlist_hata_veriyor() {
        assert!(parse_media("#EXTM3U\n#EXT-X-ENDLIST\n", "https://c/l.m3u8").is_err());
        assert!(parse("<html>hata</html>", "https://c/l.m3u8").is_err());
    }

    #[test]
    fn oznitelik_ayristirici_tirnakli_virgulu_bolmuyor() {
        let a = parse_attrs("BANDWIDTH=100,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=1280x720");
        assert_eq!(attr(&a, "BANDWIDTH").unwrap(), "100");
        assert_eq!(attr(&a, "CODECS").unwrap(), "avc1.4d401f,mp4a.40.2");
        assert_eq!(attr(&a, "RESOLUTION").unwrap(), "1280x720");
    }

    #[test]
    fn cozunurluk_ayristirmasi() {
        assert_eq!(cozunurluk(Some("1920x1080")), (Some(1920), Some(1080)));
        assert_eq!(cozunurluk(Some("bozuk")), (None, None));
        assert_eq!(cozunurluk(None), (None, None));
    }
}
