//! Uçtan uca akış (HLS/DASH) indirme testleri.
//!
//! Birim testler ayrıştırıcıların doğruluğunu gösteriyor. Buradaki soru başka:
//! yerel bir sunucudan **paralel** inen yüzlerce parça, çıktı dosyasına
//! **doğru sırada** ekleniyor mu? Sıra bozulursa video sessizce bozulur ve
//! bunu ancak dosyayı açan kullanıcı fark eder.
//!
//! Sunucu yine elle yazıldı (bkz. `indirme_uctan_uca.rs`): burada manifest,
//! parça, anahtar ve DASH belgesi gibi farklı türde yanıtlar tek bir yönlendirme
//! tablosundan veriliyor ve hangi yolun kaç kez istendiği sayılıyor — devam
//! etmenin gerçekten parçaları atladığını başka türlü kanıtlayamıyoruz.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use muiget_lib::download::manager::{DownloadManager, DownloadStatus, ManagerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

const ANAHTAR: [u8; 16] = [
    0x06, 0xa9, 0x21, 0x40, 0x36, 0xb8, 0xa1, 0x5b, 0x51, 0x2e, 0x03, 0xd5, 0x34, 0x12, 0x00, 0x06,
];

/// Parça içeriği: tohum + offsete bağlı deterministik desen. İki parça
/// birbiriyle yer değiştirirse karşılaştırma bunu yakalar.
fn parca(tohum: u8, boyut: usize) -> Vec<u8> {
    (0..boyut).map(|i| ((i + tohum as usize * 7) % 251) as u8).collect()
}

fn sifrele(iv: &[u8; 16], acik: &[u8]) -> Vec<u8> {
    let mut tampon = vec![0u8; acik.len() + 16];
    let n = Aes128CbcEnc::new(&ANAHTAR.into(), iv.into())
        .encrypt_padded_b2b_mut::<Pkcs7>(acik, &mut tampon)
        .unwrap()
        .len();
    tampon.truncate(n);
    tampon
}

/// HLS'in IV türetme kuralı (RFC 8216 §5.2): IV yoksa medya sıra numarasının
/// 128 bitlik big-endian hâli. Test bunu **bağımsız olarak** kuruyor ki
/// motordaki uygulama yanlışsa test geçmesin.
fn iv(sequence: u64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[8..].copy_from_slice(&sequence.to_be_bytes());
    out
}

struct Yanit {
    govde: Vec<u8>,
    icerik_turu: &'static str,
    /// Gövdeyi damla damla gönder — duraklat/iptal testleri için.
    yavas: bool,
}

impl Yanit {
    fn yeni(govde: Vec<u8>, icerik_turu: &'static str) -> Self {
        Yanit { govde, icerik_turu, yavas: false }
    }

    fn metin(s: &str, icerik_turu: &'static str) -> Self {
        Yanit::yeni(s.as_bytes().to_vec(), icerik_turu)
    }

    fn yavas(mut self) -> Self {
        self.yavas = true;
        self
    }
}

const M3U8: &str = "application/vnd.apple.mpegurl";
const MPD: &str = "application/dash+xml";
const IKILI: &str = "application/octet-stream";

struct TestSunucusu {
    adres: String,
    istekler: Arc<Mutex<Vec<String>>>,
    _gorev: tokio::task::JoinHandle<()>,
}

impl TestSunucusu {
    async fn baslat(yollar: HashMap<String, Yanit>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let yollar = Arc::new(yollar);
        let istekler = Arc::new(Mutex::new(Vec::new()));

        let gorev = {
            let istekler = istekler.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else { break };
                    let yollar = yollar.clone();
                    let istekler = istekler.clone();
                    tokio::spawn(async move {
                        let _ = baglantiyi_isle(stream, yollar, istekler).await;
                    });
                }
            })
        };

        TestSunucusu { adres: format!("http://127.0.0.1:{port}"), istekler, _gorev: gorev }
    }

    fn url(&self, yol: &str) -> String {
        format!("{}{yol}", self.adres)
    }

    /// Bu yol kaç kez **GET** ile istendi? (HEAD ve Range yoklaması sayılmıyor.)
    fn istek_sayisi(&self, yol: &str) -> usize {
        self.istekler.lock().unwrap().iter().filter(|y| *y == yol).count()
    }
}

async fn baglantiyi_isle(
    mut stream: TcpStream,
    yollar: Arc<HashMap<String, Yanit>>,
    istekler: Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    let mut tampon = Vec::new();
    let mut parca = [0u8; 1024];

    loop {
        let okunan = stream.read(&mut parca).await?;
        if okunan == 0 {
            return Ok(());
        }
        tampon.extend_from_slice(&parca[..okunan]);
        if tampon.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let istek = String::from_utf8_lossy(&tampon).to_string();
    let ilk_satir = istek.lines().next().unwrap_or_default().to_string();
    let head_mi = ilk_satir.starts_with("HEAD");
    let yol = ilk_satir.split_whitespace().nth(1).unwrap_or("/").to_string();

    let range = istek
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()));

    let Some(yanit) = yollar.get(&yol) else {
        let govde = "yok";
        let bas = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            govde.len()
        );
        stream.write_all(bas.as_bytes()).await?;
        stream.write_all(govde.as_bytes()).await?;
        stream.flush().await?;
        return stream.shutdown().await;
    };

    // Yalnızca gerçek gövde istekleri sayılıyor: yoklamanın attığı
    // `Range: bytes=0-0` isteği "parça yeniden indirildi" anlamına gelmiyor.
    if !head_mi && range.as_deref() != Some("bytes=0-0") {
        istekler.lock().unwrap().push(yol.clone());
    }

    let toplam = yanit.govde.len();
    let (durum, bas, son) = match &range {
        Some(raw) => {
            let (b, s) = range_ayristir(raw, toplam);
            ("206 Partial Content", b, s)
        }
        None => ("200 OK", 0, toplam.saturating_sub(1)),
    };
    let govde: &[u8] = if toplam == 0 { &[] } else { &yanit.govde[bas..=son] };

    let mut basliklar = format!(
        "HTTP/1.1 {durum}\r\n\
         Content-Length: {}\r\n\
         Accept-Ranges: bytes\r\n\
         Content-Type: {}\r\n\
         Connection: close\r\n",
        govde.len(),
        yanit.icerik_turu
    );
    if range.is_some() {
        basliklar.push_str(&format!("Content-Range: bytes {bas}-{son}/{toplam}\r\n"));
    }
    basliklar.push_str("\r\n");

    stream.write_all(basliklar.as_bytes()).await?;
    if !head_mi {
        if yanit.yavas {
            for dilim in govde.chunks(4 * 1024) {
                stream.write_all(dilim).await?;
                stream.flush().await?;
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        } else {
            stream.write_all(govde).await?;
        }
    }
    stream.flush().await?;
    stream.shutdown().await
}

fn range_ayristir(raw: &str, toplam: usize) -> (usize, usize) {
    let deger = raw.trim_start_matches("bytes=").trim();
    let (bas, son) = deger.split_once('-').unwrap_or((deger, ""));
    let bas: usize = bas.trim().parse().unwrap_or(0);
    let son: usize = son.trim().parse().unwrap_or(toplam.saturating_sub(1));
    (bas.min(toplam.saturating_sub(1)), son.min(toplam.saturating_sub(1)))
}

/// Test ayarları.
///
/// `ffmpeg_path` bilerek var olmayan bir yol: ayarlarda yol yazılıysa yalnızca
/// o deneniyor (bkz. `media::mux::adaylar`), yani geliştiricinin makinesinde
/// ffmpeg kurulu olsa bile testler aynı sonucu veriyor. Aksi hâlde çıktı
/// uzantısı makineden makineye değişirdi.
fn test_config() -> ManagerConfig {
    ManagerConfig {
        max_retries: 1,
        connect_timeout_secs: 5,
        read_timeout_secs: 10,
        media_concurrency: 3,
        ffmpeg_path: "/kesinlikle/olmayan/ffmpeg".to_string(),
        ..ManagerConfig::default()
    }
}

async fn tamamlanmayi_bekle(manager: &DownloadManager, id: &str) -> DownloadStatus {
    let son = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let anlik = manager.get(id).expect("indirme kaydı kayboldu");
        if !anlik.status.is_active() {
            return anlik.status;
        }
        if tokio::time::Instant::now() > son {
            panic!(
                "akış 30 saniyede bitmedi: durum {:?}, {}/{} byte, hata {:?}",
                anlik.status, anlik.downloaded, anlik.total_size, anlik.error
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// En az `adet` parça inene kadar bekler. Sabit `sleep` yerine olayın kendisini
/// bekliyor (aynı gerekçe: `indirme_uctan_uca.rs` → `veri_akmaya_baslayinca`).
async fn parca_inene_kadar(manager: &DownloadManager, id: &str, adet: usize) {
    let son = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let anlik = manager.get(id).expect("indirme kaydı kayboldu");
        let inen = anlik.media.as_ref().map(|m| m.segments_done).unwrap_or(0);
        if inen >= adet {
            return;
        }
        assert!(
            anlik.status.is_active(),
            "indirme {adet} parçaya ulaşmadan bitti: durum {:?}, hata {:?}",
            anlik.status,
            anlik.error
        );
        assert!(
            tokio::time::Instant::now() < son,
            "20 saniyede {adet} parça inmedi (inen: {inen})"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/* -------------------------------------------------------------------------
 * HLS
 * ---------------------------------------------------------------------- */

/// Beş parçalık düz bir HLS medya playlisti ve beklenen birleşik içerik.
fn hls_dosyalari(onek: &str, boyut: usize, adet: u8) -> (HashMap<String, Yanit>, Vec<u8>) {
    let mut yollar = HashMap::new();
    let mut beklenen = Vec::new();
    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-PLAYLIST-TYPE:VOD\n");

    for i in 0..adet {
        let govde = parca(i, boyut);
        beklenen.extend_from_slice(&govde);
        yollar.insert(format!("{onek}/s{i}.ts"), Yanit::yeni(govde, IKILI));
        playlist.push_str(&format!("#EXTINF:4.0,\ns{i}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    yollar.insert(format!("{onek}/list.m3u8"), Yanit::metin(&playlist, M3U8));

    (yollar, beklenen)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_parcalari_sirayla_birlestiriliyor() {
    let (yollar, beklenen) = hls_dosyalari("/vod", 40 * 1024, 9);
    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();

    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/vod/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);

    let anlik = manager.get(&id).unwrap();
    let inen = tokio::fs::read(&anlik.target_path).await.unwrap();
    assert_eq!(inen.len(), beklenen.len(), "toplam boyut tutmuyor");
    assert_eq!(inen, beklenen, "parçalar yanlış sırada birleşmiş");

    // ffmpeg yok: MPEG-TS olduğu gibi kaydediliyor.
    assert!(anlik.target_path.ends_with(".ts"), "beklenmeyen ad: {}", anlik.target_path);

    let medya = anlik.media.expect("akış ilerlemesi yok");
    assert_eq!(medya.protocol, "HLS");
    assert_eq!(medya.segments_done, 9);
    assert_eq!(medya.segments_total, 9);
    assert!(!medya.estimated, "bitmiş indirmede boyut hâlâ tahmin");
    assert_eq!(anlik.downloaded, anlik.total_size, "ilerleme %100'de kapanmadı");

    // Yarım dosya ve devam noktası temizlenmiş olmalı.
    assert!(!PathBuf::from(format!("{}.mgpart", anlik.target_path)).exists());
    assert!(!PathBuf::from(format!("{}.muiget", anlik.target_path)).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn master_playlistte_en_iyi_kalite_seciliyor() {
    let (mut yollar, dusuk) = hls_dosyalari("/360p", 8 * 1024, 4);
    let (yuksek_yollar, yuksek) = hls_dosyalari("/1080p", 20 * 1024, 4);
    yollar.extend(yuksek_yollar);

    yollar.insert(
        "/master.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n\
             #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n\
             360p/list.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n\
             1080p/list.m3u8\n",
            M3U8,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/master.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);

    let anlik = manager.get(&id).unwrap();
    let inen = tokio::fs::read(&anlik.target_path).await.unwrap();
    assert_eq!(inen, yuksek, "varsayılan seçim en yüksek kalite olmalı");
    assert_ne!(inen, dusuk);
    assert_eq!(
        sunucu.istek_sayisi("/360p/s0.ts"),
        0,
        "seçilmeyen kalitenin parçaları indirilmemeli"
    );
    assert!(anlik.media.unwrap().label.unwrap().contains("1920x1080"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kalite_tercihi_uygulaniyor() {
    let (mut yollar, dusuk) = hls_dosyalari("/360p", 8 * 1024, 4);
    let (yuksek_yollar, _) = hls_dosyalari("/1080p", 20 * 1024, 4);
    yollar.extend(yuksek_yollar);
    yollar.insert(
        "/master.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n\
             #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n360p/list.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080p/list.m3u8\n",
            M3U8,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        media_quality: "480".to_string(),
        ..test_config()
    })
    .unwrap();
    let id = manager
        .start(sunucu.url("/master.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    // "En fazla 480p" istendi; 360p tek uyan seçenek.
    assert_eq!(tokio::fs::read(&anlik.target_path).await.unwrap(), dusuk);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aes128_sifreli_parcalar_cozuluyor() {
    let mut yollar = HashMap::new();
    let mut beklenen = Vec::new();
    let mut playlist = String::from(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-PLAYLIST-TYPE:VOD\n\
         #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n",
    );

    for i in 0..6u8 {
        let acik = parca(i, 12 * 1024 + i as usize); // 16'nın katı değil: dolgu sınanıyor.
        beklenen.extend_from_slice(&acik);
        // IV verilmedi: motorun sıra numarasından türetmesi gerekiyor.
        yollar.insert(
            format!("/enc/s{i}.ts"),
            Yanit::yeni(sifrele(&iv(i as u64), &acik), IKILI),
        );
        playlist.push_str(&format!("#EXTINF:4.0,\ns{i}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");

    yollar.insert("/enc/list.m3u8".to_string(), Yanit::metin(&playlist, M3U8));
    yollar.insert("/enc/key.bin".to_string(), Yanit::yeni(ANAHTAR.to_vec(), IKILI));

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/enc/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    assert_eq!(tokio::fs::read(&anlik.target_path).await.unwrap(), beklenen);

    // Anahtar altı parça için bir kez indirilmeli: aksi hâlde önbellek yok
    // demektir ve gerçek bir yayında sunucuya yüzlerce gereksiz istek giderdi.
    assert_eq!(sunucu.istek_sayisi("/enc/key.bin"), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fmp4_baslangic_parcasi_dosyanin_basina_yaziliyor() {
    let mut yollar = HashMap::new();
    let init = parca(200, 2048);
    let mut beklenen = init.clone();
    let mut playlist = String::from(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"init.mp4\"\n",
    );

    yollar.insert("/cmaf/init.mp4".to_string(), Yanit::yeni(init, IKILI));
    for i in 0..5u8 {
        let govde = parca(i, 10 * 1024);
        beklenen.extend_from_slice(&govde);
        yollar.insert(format!("/cmaf/{i}.m4s"), Yanit::yeni(govde, IKILI));
        playlist.push_str(&format!("#EXTINF:4.0,\n{i}.m4s\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    yollar.insert("/cmaf/list.m3u8".to_string(), Yanit::metin(&playlist, M3U8));

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/cmaf/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    assert_eq!(tokio::fs::read(&anlik.target_path).await.unwrap(), beklenen);
    // fMP4 uç uca eklenince zaten geçerli MP4: ffmpeg olmadan da `.mp4`.
    assert!(anlik.target_path.ends_with(".mp4"), "ad: {}", anlik.target_path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duraklatilan_akis_kaldigi_parcadan_devam_ediyor() {
    // Yavaş parçalar: duraklatma komutu indirme akarken yetişsin.
    let mut yollar = HashMap::new();
    let mut beklenen = Vec::new();
    let mut playlist = String::from("#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n");
    for i in 0..8u8 {
        let govde = parca(i, 32 * 1024);
        beklenen.extend_from_slice(&govde);
        yollar.insert(format!("/slow/s{i}.ts"), Yanit::yeni(govde, IKILI).yavas());
        playlist.push_str(&format!("#EXTINF:4.0,\ns{i}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    yollar.insert("/slow/list.m3u8".to_string(), Yanit::metin(&playlist, M3U8));

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        // Tek bağlantı: hangi parçanın indiği belirsizliği kalmasın.
        media_concurrency: 1,
        ..test_config()
    })
    .unwrap();
    let id = manager
        .start(sunucu.url("/slow/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    parca_inene_kadar(&manager, &id, 2).await;
    manager.pause(&id).unwrap();
    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Paused);

    let duraklamada = manager.get(&id).unwrap().media.unwrap().segments_done;
    assert!((2..8).contains(&duraklamada), "duraklamada {duraklamada} parça inmişti");

    manager.resume(&id).unwrap();
    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);

    let anlik = manager.get(&id).unwrap();
    assert_eq!(
        tokio::fs::read(&anlik.target_path).await.unwrap(),
        beklenen,
        "devam eden indirme parçaları tekrar ya da eksik yazmış"
    );

    // Devam etmenin anlamı bu: inmiş parçalar bir daha istenmiyor.
    assert_eq!(
        sunucu.istek_sayisi("/slow/s0.ts"),
        1,
        "ilk parça devam ederken yeniden indirilmiş"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn canli_yayin_reddediliyor() {
    // `#EXT-X-ENDLIST` yok ve PLAYLIST-TYPE verilmemiş → canlı.
    let yollar = HashMap::from([(
        "/live/list.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:1200\n#EXTINF:6.0,\ns1.ts\n",
            M3U8,
        ),
    )]);

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/live/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(hata.contains("canlı"), "anlaşılmaz hata: {hata}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drm_korumali_yayin_reddediliyor() {
    let yollar = HashMap::from([(
        "/drm/list.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n\
             #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://anahtar\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n\
             #EXTINF:4.0,\ns0.ts\n#EXT-X-ENDLIST\n",
            M3U8,
        ),
    )]);

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/drm/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(
        hata.contains("SAMPLE-AES") && hata.contains("desteklenmiyor"),
        "DRM reddi açık söylenmemiş: {hata}"
    );
    // Tek bir parça bile indirilmemiş olmalı: ret ayrıştırma anında.
    assert_eq!(sunucu.istek_sayisi("/drm/s0.ts"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ayri_ses_ffmpeg_yoksa_indirmeye_hic_baslamiyor() {
    let (mut yollar, _) = hls_dosyalari("/v", 8 * 1024, 3);
    let (ses_yollar, _) = hls_dosyalari("/a", 2 * 1024, 3);
    yollar.extend(ses_yollar);
    yollar.insert(
        "/master.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n\
             #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"tr\",DEFAULT=YES,URI=\"a/list.m3u8\"\n\
             #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=1280x720,AUDIO=\"aac\"\n\
             v/list.m3u8\n",
            M3U8,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/master.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(hata.contains("ffmpeg"), "ffmpeg eksiği söylenmemiş: {hata}");

    // Asıl mesele bu: yüzlerce parçayı indirip sonunda birleştirememek yerine
    // tek byte inmeden duruyoruz.
    assert_eq!(sunucu.istek_sayisi("/v/s0.ts"), 0);
    assert_eq!(sunucu.istek_sayisi("/a/s0.ts"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn yalnizca_video_secimi_ffmpeg_olmadan_iniyor() {
    let (mut yollar, video) = hls_dosyalari("/v", 8 * 1024, 3);
    let (ses_yollar, _) = hls_dosyalari("/a", 2 * 1024, 3);
    yollar.extend(ses_yollar);
    yollar.insert(
        "/master.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n\
             #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"tr\",DEFAULT=YES,URI=\"a/list.m3u8\"\n\
             #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=1280x720,AUDIO=\"aac\"\n\
             v/list.m3u8\n",
            M3U8,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start_media(
            sunucu.url("/master.m3u8"),
            dir.path().to_path_buf(),
            Default::default(),
            muiget_lib::media::MediaSelection { video_only: true, ..Default::default() },
        )
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    assert_eq!(tokio::fs::read(&anlik.target_path).await.unwrap(), video);
    assert_eq!(sunucu.istek_sayisi("/a/s0.ts"), 0, "yalnızca video istendi, ses inmemeli");
}

/* -------------------------------------------------------------------------
 * ffmpeg birleştirmesi
 *
 * Gerçek ffmpeg her makinede yok (CI'da da yok) ve olsa bile testi ona
 * bağlamak, geliştiricinin sürümüne göre değişen bir sonuç demek olurdu.
 * Bunun yerine ffmpeg'in yerine geçen küçük bir betik yazılıyor: `-version`e
 * yanıt veriyor, çağrıldığı argümanları kaydediyor ve bir çıktı dosyası
 * üretiyor.
 *
 * Sınanan şey ffmpeg'in kendisi değil — **etrafındaki her şey**: bulunması,
 * doğru argümanlarla çağrılması, çıktının nihai ada taşınması ve parça
 * dosyalarının temizlenmesi. Argüman üretiminin doğruluğu ayrıca birim testli
 * (`media::mux::tests`).
 * ---------------------------------------------------------------------- */

/// Çalıştırılabilir sahte bir ffmpeg yazar; dönen ikinci değer argüman
/// günlüğünün yolu.
fn sahte_ffmpeg(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let gunluk = dir.join("son-cagri.txt");

    #[cfg(windows)]
    {
        let yol = dir.join("sahte-ffmpeg.cmd");
        let betik = format!(
            "@echo off\r\n\
             if \"%~1\"==\"-version\" (\r\n\
             echo ffmpeg version sahte-muiget\r\n\
             exit /b 0\r\n\
             )\r\n\
             echo %*> \"{gunluk}\"\r\n\
             set \"son=\"\r\n\
             for %%A in (%*) do set \"son=%%A\"\r\n\
             echo BIRLESIK> \"%son%\"\r\n\
             exit /b 0\r\n",
            gunluk = gunluk.display()
        );
        std::fs::write(&yol, betik).unwrap();
        (yol, gunluk)
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let yol = dir.join("sahte-ffmpeg.sh");
        let betik = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"-version\" ]; then echo 'ffmpeg version sahte-muiget'; exit 0; fi\n\
             echo \"$@\" > '{gunluk}'\n\
             for a in \"$@\"; do son=\"$a\"; done\n\
             printf 'BIRLESIK' > \"$son\"\n\
             exit 0\n",
            gunluk = gunluk.display()
        );
        std::fs::write(&yol, betik).unwrap();
        std::fs::set_permissions(&yol, std::fs::Permissions::from_mode(0o755)).unwrap();
        (yol, gunluk)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ayri_ses_ffmpeg_ile_birlestiriliyor() {
    let (mut yollar, video) = hls_dosyalari("/v", 8 * 1024, 3);
    let (ses_yollar, ses) = hls_dosyalari("/a", 2 * 1024, 3);
    yollar.extend(ses_yollar);
    yollar.insert(
        "/master.m3u8".to_string(),
        Yanit::metin(
            "#EXTM3U\n\
             #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"tr\",DEFAULT=YES,URI=\"a/list.m3u8\"\n\
             #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=1280x720,AUDIO=\"aac\"\n\
             v/list.m3u8\n",
            M3U8,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let araclar = tempfile::tempdir().unwrap();
    let (ffmpeg, gunluk) = sahte_ffmpeg(araclar.path());

    let manager = DownloadManager::new(ManagerConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        ..test_config()
    })
    .unwrap();
    let id = manager
        .start(sunucu.url("/master.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();

    // Ayrı ses varken çıktı her zaman `.mp4`.
    assert!(anlik.target_path.ends_with(".mp4"), "ad: {}", anlik.target_path);

    // Nihai dosya ffmpeg'in ürettiği: parçalar oraya doğrudan kopyalanmadı.
    let icerik = tokio::fs::read_to_string(&anlik.target_path).await.unwrap();
    assert!(icerik.starts_with("BIRLESIK"), "çıktı ffmpeg'den gelmemiş: {icerik:?}");

    // İkisi de gerçekten indi ve ffmpeg ikisini de girdi olarak aldı.
    let cagri = tokio::fs::read_to_string(&gunluk).await.unwrap();
    assert!(cagri.contains(".mgpart"), "girdi yolları geçmiyor: {cagri}");
    assert!(cagri.contains("0:v:0") && cagri.contains("1:a:0"), "akış eşlemesi yok: {cagri}");
    assert_eq!(sunucu.istek_sayisi("/v/s0.ts"), 1);
    assert_eq!(sunucu.istek_sayisi("/a/s0.ts"), 1);
    assert!(!video.is_empty() && !ses.is_empty());

    // Yarım dosyalar temizlenmiş olmalı: kullanıcı klasöründe üç kopya kalması
    // hem kafa karıştırır hem diski boşuna doldurur.
    for artik in ["", ".audio", ".mux"] {
        let yol = PathBuf::from(format!("{}{artik}.mgpart", anlik.target_path));
        assert!(!yol.exists(), "{} silinmemiş", yol.display());
    }
    assert!(!PathBuf::from(format!("{}.muiget", anlik.target_path)).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ffmpeg_varken_ts_mp4e_cevriliyor() {
    let (yollar, _) = hls_dosyalari("/vod", 8 * 1024, 3);
    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let araclar = tempfile::tempdir().unwrap();
    let (ffmpeg, gunluk) = sahte_ffmpeg(araclar.path());

    let manager = DownloadManager::new(ManagerConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        ..test_config()
    })
    .unwrap();
    let id = manager
        .start(sunucu.url("/vod/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    assert!(anlik.target_path.ends_with(".mp4"), "ad: {}", anlik.target_path);

    // Tek girdi: akış eşlemesi yok, yalnızca kap değişiyor.
    let cagri = tokio::fs::read_to_string(&gunluk).await.unwrap();
    assert!(!cagri.contains("-map"), "tek girdide eşleme gereksiz: {cagri}");
    assert!(cagri.contains("-c copy"), "yeniden kodlama yapılmamalı: {cagri}");
}

/* -------------------------------------------------------------------------
 * DASH
 * ---------------------------------------------------------------------- */

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dash_sablonlu_manifest_iniyor() {
    let mut yollar = HashMap::new();
    let init = parca(150, 1024);
    let mut beklenen = init.clone();
    yollar.insert("/dash/init.mp4".to_string(), Yanit::yeni(init, IKILI));

    // 20 saniye / 4 saniyelik parça = 5 parça, numaralar 1'den başlıyor.
    for n in 1..=5u8 {
        let govde = parca(n, 16 * 1024);
        beklenen.extend_from_slice(&govde);
        yollar.insert(format!("/dash/seg-{n}.m4s"), Yanit::yeni(govde, IKILI));
    }

    yollar.insert(
        "/dash/video.mpd".to_string(),
        Yanit::metin(
            r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
  <Period>
    <AdaptationSet mimeType="video/mp4" contentType="video">
      <SegmentTemplate initialization="init.mp4" media="seg-$Number$.m4s" startNumber="1" duration="4" timescale="1"/>
      <Representation id="v" bandwidth="2000000" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>"#,
            MPD,
        ),
    );

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/dash/video.mpd"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Completed);
    let anlik = manager.get(&id).unwrap();
    assert_eq!(tokio::fs::read(&anlik.target_path).await.unwrap(), beklenen);
    assert_eq!(anlik.media.as_ref().unwrap().protocol, "DASH");
    assert_eq!(anlik.media.as_ref().unwrap().segments_total, 5);
    assert!(anlik.target_path.ends_with(".mp4"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dash_drm_manifesti_reddediliyor() {
    let yollar = HashMap::from([(
        "/drm/av.mpd".to_string(),
        Yanit::metin(
            r#"<MPD type="static" mediaPresentationDuration="PT8S"><Period>
  <AdaptationSet mimeType="video/mp4">
    <ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>
    <SegmentTemplate media="s$Number$.m4s" duration="4" timescale="1"/>
    <Representation id="v" bandwidth="1000" width="640" height="360"/>
  </AdaptationSet></Period></MPD>"#,
            MPD,
        ),
    )]);

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/drm/av.mpd"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(hata.contains("DRM"), "DRM reddi açık söylenmemiş: {hata}");
}

/* -------------------------------------------------------------------------
 * Ortak davranış
 * ---------------------------------------------------------------------- */

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manifest_yerine_html_gelirse_anlasilir_hata() {
    let yollar = HashMap::from([(
        "/x/list.m3u8".to_string(),
        Yanit::metin("<html><body>404 bulunamadı</body></html>", "text/html"),
    )]);

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/x/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(hata.contains("manifesti gibi görünmüyor"), "hata: {hata}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eksik_parca_indirmeyi_dusuruyor() {
    let (mut yollar, _) = hls_dosyalari("/k", 8 * 1024, 4);
    // Ortadaki parçayı sil: sunucu 404 dönecek.
    yollar.remove("/k/s2.ts");

    let sunucu = TestSunucusu::baslat(yollar).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config()).unwrap();
    let id = manager
        .start(sunucu.url("/k/list.m3u8"), dir.path().to_path_buf())
        .unwrap();

    assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Failed);
    let anlik = manager.get(&id).unwrap();
    assert!(anlik.error.unwrap_or_default().contains("404"));
    // Eksik parçalı bir dosya nihai adına taşınmamalı: kullanıcı onu tam
    // sanıp izlemeye kalkardı.
    assert!(!PathBuf::from(&anlik.target_path).exists());
}
