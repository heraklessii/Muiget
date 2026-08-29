//! Uçtan uca indirme testleri.
//!
//! Birim testler parçaların doğruluğunu gösteriyor; burada asıl soru şu:
//! gerçek bir HTTP sunucusundan paralel inen segmentler diske **doğru sırada**
//! yazılıyor mu? Bu yüzden test, yerelde konuşan küçük bir HTTP sunucusu
//! kaldırıp motoru ona karşı çalıştırıyor.
//!
//! Sunucu bilinçli olarak elle yazıldı: `Range` davranışını (206, 200, hatta
//! "Range'i yok say") teste göre değiştirebilmek gerekiyor ve hazır bir test
//! sunucusu kütüphanesi bu kontrolü vermiyor.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use muiget_lib::download::manager::{DownloadManager, DownloadStatus, ManagerConfig};
use muiget_lib::download::resume::ResumeMeta;
use muiget_lib::download::segmenter;
use muiget_lib::download::writer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Test dosyasının içeriği: offset'e bağlı deterministik desen.
/// Bir segment yanlış offsete yazarsa karşılaştırma bunu yakalar.
fn beklenen_icerik(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum SunucuKipi {
    /// Normal: Range destekli, 206 döner.
    RangeDestekli,
    /// `Accept-Ranges` yok, Range başlığı yok sayılır, hep 200 + tam gövde.
    RangeYok,
    /// Range destekli ama gövdeyi damla damla gönderiyor.
    ///
    /// Duraklat/iptal testleri için şart: localhost'ta birkaç MB milisaniyeler
    /// içinde iniyor ve indirme, iptal komutu gelmeden bitiyordu. Yavaş sunucu
    /// bu testleri zamanlamaya bağlı olmaktan çıkarıyor.
    Yavas,
}

impl SunucuKipi {
    fn range_destekli(self) -> bool {
        matches!(self, SunucuKipi::RangeDestekli | SunucuKipi::Yavas)
    }
}

struct TestSunucusu {
    adres: String,
    _gorev: tokio::task::JoinHandle<()>,
}

impl TestSunucusu {
    async fn baslat(icerik: Vec<u8>, kip: SunucuKipi) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let icerik = Arc::new(icerik);

        let gorev = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let icerik = icerik.clone();
                tokio::spawn(async move {
                    let _ = baglantiyi_isle(stream, icerik, kip).await;
                });
            }
        });

        TestSunucusu { adres: format!("http://127.0.0.1:{port}"), _gorev: gorev }
    }

    fn url(&self, yol: &str) -> String {
        format!("{}{yol}", self.adres)
    }
}

async fn baglantiyi_isle(
    mut stream: TcpStream,
    icerik: Arc<Vec<u8>>,
    kip: SunucuKipi,
) -> std::io::Result<()> {
    let mut tampon = Vec::new();
    let mut parca = [0u8; 1024];

    // İstek başlıklarının sonunu (\r\n\r\n) bul.
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

    let range = istek
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()));

    let toplam = icerik.len();
    let accept_ranges = if kip.range_destekli() { "bytes" } else { "none" };

    let (durum, govde_araligi, content_range) = match &range {
        Some(raw) if kip.range_destekli() => {
            let (bas, son) = range_ayristir(raw, toplam);
            (
                "206 Partial Content",
                bas..=son,
                Some(format!("bytes {bas}-{son}/{toplam}")),
            )
        }
        // Range yok ya da sunucu desteklemiyor → tam gövde.
        _ => ("200 OK", 0..=toplam.saturating_sub(1), None),
    };

    let govde: &[u8] = if toplam == 0 { &[] } else { &icerik[*govde_araligi.start()..=*govde_araligi.end()] };

    let mut basliklar = format!(
        "HTTP/1.1 {durum}\r\n\
         Content-Length: {}\r\n\
         Accept-Ranges: {accept_ranges}\r\n\
         ETag: \"test-etag-{toplam}\"\r\n\
         Content-Type: application/octet-stream\r\n\
         Connection: close\r\n",
        govde.len()
    );
    if let Some(cr) = content_range {
        basliklar.push_str(&format!("Content-Range: {cr}\r\n"));
    }
    basliklar.push_str("\r\n");

    stream.write_all(basliklar.as_bytes()).await?;
    if !head_mi {
        if kip == SunucuKipi::Yavas {
            // Damla damla: her parça arasında bekle ki iptal/duraklat komutu
            // indirme akarken yetişsin.
            for parca in govde.chunks(8 * 1024) {
                stream.write_all(parca).await?;
                stream.flush().await?;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        } else {
            stream.write_all(govde).await?;
        }
    }
    stream.flush().await?;
    stream.shutdown().await
}

/// `bytes=100-199` → `(100, 199)`. Açık uçlu (`bytes=100-`) da destekleniyor.
fn range_ayristir(raw: &str, toplam: usize) -> (usize, usize) {
    let deger = raw.trim_start_matches("bytes=").trim();
    let (bas, son) = deger.split_once('-').unwrap_or((deger, ""));
    let bas: usize = bas.trim().parse().unwrap_or(0);
    let son: usize = son.trim().parse().unwrap_or(toplam.saturating_sub(1));
    (bas.min(toplam.saturating_sub(1)), son.min(toplam.saturating_sub(1)))
}

/// İndirme bitene kadar bekler. Testin sonsuza kadar asılı kalmaması için
/// zaman aşımı var.
async fn tamamlanmayi_bekle(manager: &DownloadManager, id: &str) -> DownloadStatus {
    let son = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let snapshot = manager.get(id).expect("indirme kaydı kayboldu");
        if !snapshot.status.is_active() {
            return snapshot.status;
        }
        if tokio::time::Instant::now() > son {
            panic!(
                "indirme 30 saniyede bitmedi: durum {:?}, {}/{} byte, hata: {:?}",
                snapshot.status, snapshot.downloaded, snapshot.total_size, snapshot.error
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn test_config(segments: usize) -> ManagerConfig {
    ManagerConfig {
        segments,
        // Test dosyaları küçük; varsayılan 1 MB eşiği her şeyi tek segmente
        // indirir ve paralelliği hiç sınamazdık.
        min_segment_size: 4 * 1024,
        min_steal_size: 4 * 1024,
        max_retries: 2,
        connect_timeout_secs: 5,
        read_timeout_secs: 10,
        ..ManagerConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmentli_indirme_dosyayi_dogru_yaziyor() {
    const BOYUT: usize = 512 * 1024; // 8 segment × 64 KB
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(8)).unwrap();

    let id = manager
        .start(sunucu.url("/buyuk.bin"), dir.path().to_path_buf())
        .unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    // Gerçekten segmentlere bölünmüş mü?
    assert!(snapshot.supports_ranges, "sunucu Range destekliyor olarak görülmeliydi");
    assert!(
        snapshot.segments.len() > 1,
        "8 segment istendi, {} segment kullanıldı",
        snapshot.segments.len()
    );
    assert_eq!(snapshot.downloaded, BOYUT as u64);
    assert_eq!(snapshot.total_size, BOYUT as u64);

    // Asıl kontrol: dosya byte-byte doğru mu?
    let hedef = PathBuf::from(&snapshot.target_path);
    let yazilan = tokio::fs::read(&hedef).await.unwrap();
    assert_eq!(yazilan.len(), BOYUT, "dosya boyutu tutmuyor");
    assert!(yazilan == icerik, "dosya içeriği bozuk — segmentler yanlış offsete yazmış");

    // Ara dosyalar temizlenmiş olmalı.
    assert!(!writer::part_path(&hedef).exists(), ".mgpart dosyası kalmış");
    assert!(
        !muiget_lib::download::resume::meta_path(&hedef).exists(),
        ".muiget meta dosyası kalmış"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn range_desteklemeyen_sunucu_tek_baglantiyla_iniyor() {
    const BOYUT: usize = 64 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeYok).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(8)).unwrap();

    let id = manager.start(sunucu.url("/tek.bin"), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();

    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);
    assert!(!snapshot.supports_ranges);
    assert_eq!(snapshot.segments.len(), 1, "Range yokken tek segment olmalı");
    assert!(snapshot.warning.is_some(), "kullanıcı tek bağlantı konusunda uyarılmalı");

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(yazilan == icerik);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn yarim_dosya_kaldigi_yerden_devam_ediyor() {
    const BOYUT: usize = 256 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeDestekli).await;
    let url = sunucu.url("/devam.bin");

    let dir = tempfile::tempdir().unwrap();
    let hedef = dir.path().join("devam.bin");
    let part = writer::part_path(&hedef);

    // Önceki oturumdan kalmış gibi bir durum kur: dosya tam boyutta ayrılmış,
    // 4 segmentin ilk ikisi tamamen inmiş, diğer ikisi hiç başlamamış.
    writer::allocate(&part, BOYUT as u64).await.unwrap();
    let mut segments = segmenter::plan_segments(BOYUT as u64, 4, 4 * 1024);
    assert_eq!(segments.len(), 4);

    for segment in segments.iter_mut().take(2) {
        let mut w = writer::SegmentWriter::open(&part, segment.start).await.unwrap();
        w.write_chunk(&icerik[segment.start as usize..=segment.end as usize]).await.unwrap();
        w.flush().await.unwrap();
        segment.downloaded = segment.total();
    }

    let caps = muiget_lib::download::http::ServerCapabilities {
        final_url: url.clone(),
        supports_ranges: true,
        content_length: Some(BOYUT as u64),
        etag: Some(format!("\"test-etag-{BOYUT}\"")),
        last_modified: None,
        file_name: "devam.bin".into(),
        content_type: None,
    };
    let mut meta = ResumeMeta::new("onceki".into(), url.clone(), &caps, segments);
    meta.save(&hedef).await.unwrap();

    let yarim = meta.downloaded();
    assert_eq!(yarim, BOYUT as u64 / 2, "kurulum yarısını indirmiş olmalı");

    // Şimdi motor devreye giriyor.
    let manager = DownloadManager::new(test_config(4)).unwrap();
    let id = manager.start(url, dir.path().to_path_buf()).unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    // Aynı dosyaya devam etmiş olmalı — "devam (1).bin" üretmiş olmamalı.
    assert_eq!(
        PathBuf::from(&snapshot.target_path),
        hedef,
        "resume mevcut dosyaya devam etmeliydi"
    );

    let yazilan = tokio::fs::read(&hedef).await.unwrap();
    assert!(yazilan == icerik, "resume sonrası dosya bozuk");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bayat_meta_bastan_indirtiyor() {
    const BOYUT: usize = 128 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeDestekli).await;
    let url = sunucu.url("/bayat.bin");

    let dir = tempfile::tempdir().unwrap();
    let hedef = dir.path().join("bayat.bin");
    let part = writer::part_path(&hedef);

    // Yarım dosyayı ÇÖP veriyle doldur ve metaya "yarısı indi" yazdır; ama
    // ETag'i sunucununkinden farklı ver. Motor bunu fark edip baştan indirmeli.
    writer::allocate(&part, BOYUT as u64).await.unwrap();
    let mut w = writer::SegmentWriter::open(&part, 0).await.unwrap();
    w.write_chunk(&vec![0xFF; BOYUT / 2]).await.unwrap();
    w.flush().await.unwrap();

    let mut segments = segmenter::plan_segments(BOYUT as u64, 2, 4 * 1024);
    segments[0].downloaded = segments[0].total();

    let caps = muiget_lib::download::http::ServerCapabilities {
        final_url: url.clone(),
        supports_ranges: true,
        content_length: Some(BOYUT as u64),
        etag: Some("\"ESKI-SURUM\"".into()),
        last_modified: None,
        file_name: "bayat.bin".into(),
        content_type: None,
    };
    ResumeMeta::new("onceki".into(), url.clone(), &caps, segments)
        .save(&hedef)
        .await
        .unwrap();

    let manager = DownloadManager::new(test_config(2)).unwrap();
    let id = manager.start(url, dir.path().to_path_buf()).unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(
        yazilan == icerik,
        "bayat meta'ya güvenilmiş: dosyada eski çöp veri kalmış"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duraklat_ve_devam_et_dosyayi_bozmuyor() {
    // Yavaş sunucu: 4 segment × 128 KB, parça başına 25 ms → indirme ~400 ms
    // sürüyor. 80 ms'de duraklatmak indirmeyi kesin olarak akarken yakalıyor.
    const BOYUT: usize = 512 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();
    let id = manager.start(sunucu.url("/duraklat.bin"), dir.path().to_path_buf()).unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    manager.pause(&id).unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    assert_eq!(durum, DownloadStatus::Paused, "duraklatma uygulanmadı");

    let duraklamada = manager.get(&id).unwrap();
    assert!(duraklamada.downloaded > 0, "duraklatmadan önce hiç veri inmemiş");
    assert!(
        duraklamada.downloaded < BOYUT as u64,
        "duraklatma indirme bitmeden yakalamalıydı"
    );

    manager.resume(&id).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(yazilan == icerik, "duraklat/devam et sonrası dosya bozuk");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn iptal_edilen_indirme_dosya_birakmiyor() {
    const BOYUT: usize = 512 * 1024;
    let sunucu = TestSunucusu::baslat(beklenen_icerik(BOYUT), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();
    let id = manager.start(sunucu.url("/iptal.bin"), dir.path().to_path_buf()).unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    manager.cancel(&id).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    assert_eq!(durum, DownloadStatus::Cancelled, "iptal uygulanmadı");

    manager.remove(&id, true).await.unwrap();

    assert!(manager.get(&id).is_none(), "kayıt listeden silinmeliydi");

    let mut girdiler = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut kalanlar = Vec::new();
    while let Some(g) = girdiler.next_entry().await.unwrap() {
        kalanlar.push(g.file_name().to_string_lossy().into_owned());
    }
    assert!(kalanlar.is_empty(), "iptal sonrası dosya kalmış: {kalanlar:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn olmayan_adres_hata_veriyor() {
    let sunucu = TestSunucusu::baslat(Vec::new(), SunucuKipi::RangeDestekli).await;
    // Sunucu boş içerik döndürüyor; asıl sınadığımız bağlantı kurulamayan port.
    drop(sunucu);

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();
    // 1 numaralı port neredeyse kesinlikle kapalı.
    let id = manager
        .start("http://127.0.0.1:1/yok.bin".to_string(), dir.path().to_path_buf())
        .unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    assert_eq!(durum, DownloadStatus::Failed);
    assert!(manager.get(&id).unwrap().error.is_some(), "hata mesajı boş");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kuyruk_es_zamanli_indirme_sinirini_asmiyor() {
    // Yavaş sunucu şart: localhost'ta üç dosya milisaniyeler içinde inip
    // biterdi ve kuyruğun devreye girip girmediğini hiç göremezdik.
    const BOYUT: usize = 128 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(2);
    config.max_concurrent_downloads = 1;
    let manager = DownloadManager::new(config).unwrap();

    let idler: Vec<String> = (0..3)
        .map(|i| {
            manager
                .start(sunucu.url(&format!("/kuyruk-{i}.bin")), dir.path().to_path_buf())
                .unwrap()
        })
        .collect();

    // Üçü de aynı anda başlatıldı; ikisinin sıraya girmiş olması gerekiyor.
    let bekleyen = manager
        .list()
        .iter()
        .filter(|d| d.status == DownloadStatus::Queued)
        .count();
    assert!(bekleyen >= 2, "sınır 1 iken üç indirmeden {bekleyen} tanesi kuyruğa girdi");

    // İndirmeler sürerken aynı anda kaçının çalıştığını sık sık örnekle.
    let mut en_fazla = 0usize;
    let son = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let liste = manager.list();
        let calisan = liste
            .iter()
            .filter(|d| matches!(d.status, DownloadStatus::Running | DownloadStatus::Probing))
            .count();
        en_fazla = en_fazla.max(calisan);

        if liste.iter().all(|d| !d.status.is_active()) {
            break;
        }
        if tokio::time::Instant::now() > son {
            panic!("kuyruk 60 saniyede boşalmadı: {:?}", liste.iter().map(|d| d.status).collect::<Vec<_>>());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(en_fazla, 1, "eşzamanlılık sınırı 1 iken {en_fazla} indirme birlikte çalıştı");

    // Sıraya girmek indirmeyi bozmamalı: üçü de tamamlanıp doğru inmeli.
    for id in &idler {
        let snapshot = manager.get(id).unwrap();
        assert_eq!(
            snapshot.status,
            DownloadStatus::Completed,
            "{} tamamlanmadı: {:?}",
            snapshot.file_name,
            snapshot.error
        );
        let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
        assert!(yazilan == icerik, "{} bozuk indi", snapshot.file_name);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kuyrukta_bekleyen_indirme_duraklatilabiliyor() {
    const BOYUT: usize = 128 * 1024;
    let sunucu = TestSunucusu::baslat(beklenen_icerik(BOYUT), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(2);
    config.max_concurrent_downloads = 1;
    let manager = DownloadManager::new(config).unwrap();

    let birinci = manager.start(sunucu.url("/onde.bin"), dir.path().to_path_buf()).unwrap();
    let bekleyen = manager.start(sunucu.url("/arkada.bin"), dir.path().to_path_buf()).unwrap();

    // Sırası gelmeden duraklatılan indirme, öndeki bitince başlamamalı.
    manager.pause(&bekleyen).unwrap();
    assert_eq!(manager.get(&bekleyen).unwrap().status, DownloadStatus::Paused);

    assert_eq!(tamamlanmayi_bekle(&manager, &birinci).await, DownloadStatus::Completed);

    // Öndeki bittikten sonra kuyruk pompalandı; duraklatılan hâlâ duraklamış olmalı.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        manager.get(&bekleyen).unwrap().status,
        DownloadStatus::Paused,
        "duraklatılan indirme sırası gelince kendiliğinden başladı"
    );
    assert_eq!(manager.get(&bekleyen).unwrap().downloaded, 0, "hiç veri inmemeli");

    // Kullanıcı devam derse normal şekilde inmeli.
    manager.resume(&bekleyen).unwrap();
    assert_eq!(tamamlanmayi_bekle(&manager, &bekleyen).await, DownloadStatus::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oturum_sonrasi_liste_diskten_geri_yukleniyor() {
    // Bu testin sınadığı senaryo: kullanıcı indirme yarıdayken uygulamayı
    // kapatıyor, sonra yeniden açıyor. Yeni yönetici eski listeyi bilmiyor;
    // tek dayanağı diskteki `.muiget` dosyası.
    const BOYUT: usize = 512 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;
    let url = sunucu.url("/oturum.bin");

    let dir = tempfile::tempdir().unwrap();

    // --- Birinci oturum: başlat, yarıda duraklat, yöneticiyi at ---
    let inen_once = {
        let manager = DownloadManager::new(test_config(4)).unwrap();
        let id = manager.start(url.clone(), dir.path().to_path_buf()).unwrap();

        tokio::time::sleep(Duration::from_millis(120)).await;
        manager.pause(&id).unwrap();
        assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Paused);

        let snapshot = manager.get(&id).unwrap();
        assert!(snapshot.downloaded > 0, "duraklatmadan önce veri inmemiş");
        assert!(snapshot.downloaded < BOYUT as u64, "indirme erken bitmiş");
        snapshot.downloaded
    };

    // --- İkinci oturum: taze yönetici, yalnızca klasörü tarıyor ---
    let manager = DownloadManager::new(test_config(4)).unwrap();
    assert!(manager.list().is_empty(), "yeni yönetici boş başlamalı");

    assert_eq!(manager.restore(dir.path()).await, 1, "yarım indirme bulunamadı");

    let geri_yuklenen = manager.list()[0].clone();
    assert_eq!(geri_yuklenen.status, DownloadStatus::Paused);
    assert_eq!(geri_yuklenen.url, url);
    assert_eq!(geri_yuklenen.total_size, BOYUT as u64);
    assert_eq!(
        geri_yuklenen.downloaded, inen_once,
        "geri yüklenen ilerleme diskteki metayla aynı olmalı"
    );

    // Ve devam edince baştan değil kaldığı yerden inip doğru dosyayı üretmeli.
    manager.resume(&geri_yuklenen.id).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &geri_yuklenen.id).await;
    let snapshot = manager.get(&geri_yuklenen.id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(yazilan == icerik, "oturumlar arası devam eden dosya bozuk");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn geri_yuklenen_indirme_ikinci_kez_eklenemiyor() {
    // Geri yükleme listeye bir kayıt koyduktan sonra kullanıcı aynı bağlantıyı
    // yapıştırırsa: `benzersiz_yol` aynı yarım dosyayı tanıyıp aynı yolu döner
    // ve iki süpervizör aynı dosyaya yazmaya başlardı.
    const BOYUT: usize = 512 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;
    let url = sunucu.url("/tekrar.bin");

    let dir = tempfile::tempdir().unwrap();

    // Yarıda kalmış bir indirme bırak.
    {
        let manager = DownloadManager::new(test_config(4)).unwrap();
        let id = manager.start(url.clone(), dir.path().to_path_buf()).unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        manager.pause(&id).unwrap();
        assert_eq!(tamamlanmayi_bekle(&manager, &id).await, DownloadStatus::Paused);
    }

    let manager = DownloadManager::new(test_config(4)).unwrap();
    assert_eq!(manager.restore(dir.path()).await, 1);
    let geri_yuklenen = manager.list()[0].clone();

    // Aynı bağlantıyı elle ekle: kayıt oluşuyor ama süpervizör reddetmeli.
    let ikinci = manager.start(url.clone(), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &ikinci).await;

    assert_eq!(durum, DownloadStatus::Failed, "aynı dosyaya ikinci indirme başlamamalı");
    let hata = manager.get(&ikinci).unwrap().error.unwrap_or_default();
    assert!(hata.contains("zaten listede"), "anlaşılmayan hata mesajı: {hata}");

    // Geri yüklenen kayıt bozulmamış olmalı ve normal şekilde sürebilmeli.
    assert_eq!(manager.get(&geri_yuklenen.id).unwrap().status, DownloadStatus::Paused);
    manager.resume(&geri_yuklenen.id).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &geri_yuklenen.id).await;
    let snapshot = manager.get(&geri_yuklenen.id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(yazilan == icerik, "dosya bozuk — iki kayıt aynı dosyaya yazmış olabilir");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tumunu_duraklat_ve_tumunu_surdur() {
    const BOYUT: usize = 128 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    // Sınır 1: biri çalışırken diğer ikisi kuyrukta. "Tümünü duraklat"
    // kuyruktakilere de dokunmalı, yoksa öndeki durunca sıradaki başlardı.
    let mut config = test_config(2);
    config.max_concurrent_downloads = 1;
    let manager = DownloadManager::new(config).unwrap();

    let idler: Vec<String> = (0..3)
        .map(|i| {
            manager
                .start(sunucu.url(&format!("/toplu-{i}.bin")), dir.path().to_path_buf())
                .unwrap()
        })
        .collect();

    tokio::time::sleep(Duration::from_millis(120)).await;

    let duraklatilan = manager.pause_all();
    assert_eq!(duraklatilan, 3, "çalışan ve kuyrukta bekleyenlerin hepsi duraklatılmalı");

    for id in &idler {
        assert_eq!(tamamlanmayi_bekle(&manager, id).await, DownloadStatus::Paused);
    }

    // Duraklamış hâlde kalmalı: kuyruk kendiliğinden hiçbirini başlatmamalı.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        manager.list().iter().all(|d| d.status == DownloadStatus::Paused),
        "duraklatmadan sonra bir indirme kendiliğinden başladı"
    );

    let surdurulen = manager.resume_all();
    assert_eq!(surdurulen, 3);

    for id in &idler {
        let durum = tamamlanmayi_bekle(&manager, id).await;
        let snapshot = manager.get(id).unwrap();
        assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);
        let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
        assert!(yazilan == icerik, "{} bozuk indi", snapshot.file_name);
    }
}

