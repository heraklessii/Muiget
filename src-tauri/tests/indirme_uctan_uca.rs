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
    /// `Authorization: Basic YWxpOmdpemxp` (ali:gizli) istiyor, yoksa 401.
    KimlikIster,
    /// YouTube SABR taklidi: her isteğe **200** ve `application/vnd.yt-ump`.
    ///
    /// Ölçülen davranışın birebir kopyası (karar #33): `Content-Length` yok,
    /// `Accept-Ranges` yok, gövdede medya değil `sabr.malformed_config` hata
    /// yapısı var. Kip şart, çünkü tehlikeli olan sunucunun **hata
    /// dönmemesi**: motor bunu boyutu bilinmeyen normal bir indirme sanıp
    /// çöp gövdeyi dosya diye yazıyor ve "tamamlandı" diyordu.
    SabrUmp,
}

/// [`SunucuKipi::KimlikIster`] kipinin beklediği başlık değeri — `ali:gizli`.
const BEKLENEN_KIMLIK: &str = "Basic YWxpOmdpemxp";

impl SunucuKipi {
    fn range_destekli(self) -> bool {
        matches!(self, SunucuKipi::RangeDestekli | SunucuKipi::Yavas | SunucuKipi::KimlikIster)
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

    // Korumalı sunucu: kimlik başlığı yoksa gövde hiç gönderilmiyor.
    if kip == SunucuKipi::KimlikIster {
        let kimlik_var = istek
            .lines()
            .filter_map(|l| l.split_once(':'))
            .any(|(ad, deger)| {
                // Şema adı büyük/küçük harfe duyarsız, base64 yükü değil.
                ad.trim().eq_ignore_ascii_case("authorization")
                    && deger.trim() == BEKLENEN_KIMLIK
            });

        if !kimlik_var {
            let yanit = "HTTP/1.1 401 Unauthorized\r\n\
                 Content-Length: 0\r\n\
                 WWW-Authenticate: Basic realm=\"test\"\r\n\
                 Connection: close\r\n\r\n";
            stream.write_all(yanit.as_bytes()).await?;
            stream.flush().await?;
            return stream.shutdown().await;
        }
    }

    // SABR: istek ne olursa olsun (HEAD de dahil) aynı kontrol akışı dönüyor.
    if kip == SunucuKipi::SabrUmp {
        let govde: &[u8] = b"\x2c\x1d\n\x15sabr.malformed_config\x10\x02";
        let basliklar = "HTTP/1.1 200 OK\r\n\
             Content-Type: application/vnd.yt-ump\r\n\
             Connection: close\r\n\r\n";
        stream.write_all(basliklar.as_bytes()).await?;
        if !head_mi {
            stream.write_all(govde).await?;
        }
        stream.flush().await?;
        return stream.shutdown().await;
    }

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

/// İndirme gerçekten akmaya başlayana kadar bekler ve inen byte sayısını döner.
///
/// Sabit `sleep(80ms)` + "bu arada veri inmiştir" varsayımının yerine geçti.
/// O varsayım GitHub'ın Windows runner'ında tutmadı ve
/// `oturum_sonrasi_liste_diskten_geri_yukleniyor` "duraklatmadan önce veri
/// inmemiş" diye düştü — `docs/tasks.md`'de önceden not edilmiş risk.
///
/// Süreyi büyütmek yanlış çözüm olurdu: eşik ne kadar büyütülse daha yavaş bir
/// makinede yine yetmeyebilir, hızlı makinede ise her koşu boşuna beklerdi.
/// Burada beklenen olayın **kendisi** bekleniyor; hızlı makinede birkaç
/// milisaniyede dönüyor, yavaşta gerektiği kadar bekliyor. Ayrıca indirmeyi
/// mümkün olan en erken anda yakaladığı için "duraklatma bitmeden yetişmeli"
/// tarafı da güvenceye giriyor.
async fn veri_akmaya_baslayinca(manager: &DownloadManager, id: &str) -> u64 {
    let son = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = manager.get(id).expect("indirme kaydı kayboldu");
        if snapshot.downloaded > 0 {
            return snapshot.downloaded;
        }
        assert!(
            snapshot.status.is_active(),
            "indirme tek byte inmeden bitti: durum {:?}, hata {:?}",
            snapshot.status,
            snapshot.error
        );
        assert!(
            tokio::time::Instant::now() < son,
            "20 saniyede tek byte inmedi: durum {:?}",
            snapshot.status
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
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
    // sürüyor. İlk byte iner inmez duraklatıldığı için indirme kesin olarak
    // akarken yakalanıyor.
    const BOYUT: usize = 512 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::Yavas).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();
    let id = manager.start(sunucu.url("/duraklat.bin"), dir.path().to_path_buf()).unwrap();

    veri_akmaya_baslayinca(&manager, &id).await;
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

    veri_akmaya_baslayinca(&manager, &id).await;
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

/// SABR/UMP yanıtı **başarı sayılmamalı**.
///
/// Bu, İlker'in "YouTube indirme çalışmıyor" bildiriminin altından çıkan asıl
/// kusurun gerileme testi (karar #33). Kritik nokta sunucunun hata dönmemesi:
/// 200 + gövde geliyor, motor da bunu boyutu bilinmeyen normal bir indirme
/// sanıp birkaç yüz byte'lık kontrol akışını dosya diye yazıyor ve
/// "tamamlandı" diyordu. Test hem durumu hem de **dosya bırakılmadığını**
/// sınıyor: oynatılamayan bir dosyayı diskte bırakmak, hiç indirmemekten kötü.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sabr_ump_yaniti_basari_sayilmiyor() {
    let sunucu = TestSunucusu::baslat(Vec::new(), SunucuKipi::SabrUmp).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();
    let id = manager
        .start(sunucu.url("/videoplayback?itag=137"), dir.path().to_path_buf())
        .unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    assert_eq!(durum, DownloadStatus::Failed, "UMP kontrol akışı başarı sayıldı");

    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(
        hata.contains("UMP") || hata.to_lowercase().contains("ump"),
        "hata mesajı sebebi söylemiyor: {hata}"
    );

    let kalanlar: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|g| g.ok().map(|g| g.file_name()))
        .collect();
    assert!(kalanlar.is_empty(), "başarısız indirme dosya bıraktı: {kalanlar:?}");
}

/// SABR adresi ağa hiç çıkmadan reddediliyor.
///
/// Adres `sabr=1` taşıyorsa sonucu zaten biliyoruz; kullanıcıyı bir istek
/// turu bekletmenin ve mesajı "medya değil"e düşürmenin anlamı yok. Sunucu
/// bilerek **kapalı** bir porta bakıyor: istek atılsaydı bağlantı hatası
/// alırdık, SABR mesajı değil.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sabr_adresi_istek_atmadan_reddediliyor() {
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();

    let id = manager
        .start(
            "https://rr2---sn-test.googlevideo.com/videoplayback?expire=1&sabr=1&n=abc".to_string(),
            dir.path().to_path_buf(),
        )
        .unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    assert_eq!(durum, DownloadStatus::Failed);

    let hata = manager.get(&id).unwrap().error.unwrap_or_default();
    assert!(hata.contains("SABR"), "hata mesajı SABR demiyor: {hata}");
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
    //
    // Buradaki sabit bekleyiş `veri_akmaya_baslayinca` ile değiştirilemez ve
    // kırılgan da değil: sınanan şey bir olayın **olmaması**. Yavaş bir makinede
    // en fazla yanlışlıkla geçer, yanlışlıkla düşmez.
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

        veri_akmaya_baslayinca(&manager, &id).await;
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
        veri_akmaya_baslayinca(&manager, &id).await;
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

    // İlki akmaya başlayınca hepsi başlamış sayılır: kuyruk `pump()` ile tek
    // geçişte dolduruluyor, üçü de aynı anda kayda giriyor.
    veri_akmaya_baslayinca(&manager, &idler[0]).await;

    let duraklatilan = manager.pause_all();
    assert_eq!(duraklatilan, 3, "çalışan ve kuyrukta bekleyenlerin hepsi duraklatılmalı");

    for id in &idler {
        assert_eq!(tamamlanmayi_bekle(&manager, id).await, DownloadStatus::Paused);
    }

    // Duraklamış hâlde kalmalı: kuyruk kendiliğinden hiçbirini başlatmamalı.
    // Sabit bekleyiş burada da bilinçli — bkz. yukarıdaki not.
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


#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uzantidan_gelen_dosya_adi_sunucununkini_eziyor() {
    // Tarayıcı dosya adını yönlendirme zinciri ve kendi kurallarıyla çözüyor;
    // sunucunun ham adından daha doğru oluyor. `DownloadOptions::file_name`
    // bunun için var ama yoklama sonucu üzerine yazdığı için tutulmuyordu:
    // uzantıdan gelen her indirme sunucunun verdiği adla kaydediliyordu.
    const BOYUT: usize = 64 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(2)).unwrap();

    let secenekler = muiget_lib::download::DownloadOptions {
        headers: Vec::new(),
        file_name: Some("tarayicinin-cozdugu-ad.bin".into()),
    };

    // Sunucu bu adresi "sunucu-adi.bin" diye adlandırıyor (URL'den türetiliyor).
    let id = manager
        .start_with(sunucu.url("/sunucu-adi.bin"), dir.path().to_path_buf(), secenekler)
        .unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    assert_eq!(
        snapshot.file_name, "tarayicinin-cozdugu-ad.bin",
        "arayüzde uzantının verdiği ad görünmeli"
    );
    let hedef = PathBuf::from(&snapshot.target_path);
    assert_eq!(
        hedef.file_name().unwrap(),
        "tarayicinin-cozdugu-ad.bin",
        "dosya diske uzantının verdiği adla yazılmalı"
    );
    assert!(hedef.exists(), "dosya beklenen yolda yok");

    let yazilan = tokio::fs::read(&hedef).await.unwrap();
    assert!(yazilan == icerik);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kategori_acikken_dosya_alt_klasore_iniyor() {
    const BOYUT: usize = 128 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let config = ManagerConfig { categorize: true, ..test_config(4) };
    let manager = DownloadManager::new(config).unwrap();

    let id = manager.start(sunucu.url("/film.mkv"), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let hedef = PathBuf::from(&snapshot.target_path);
    assert_eq!(
        hedef.parent().and_then(|p| p.file_name()).and_then(|a| a.to_str()),
        Some("Video"),
        "mkv dosyası Video klasörüne inmeliydi: {}",
        hedef.display()
    );

    // Klasörleme dosyanın kendisini bozmamalı.
    let yazilan = tokio::fs::read(&hedef).await.unwrap();
    assert!(yazilan == icerik, "dosya içeriği bozuk");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kategori_acikken_taninmayan_tur_kokte_kaliyor() {
    const BOYUT: usize = 64 * 1024;
    let sunucu = TestSunucusu::baslat(beklenen_icerik(BOYUT), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let config = ManagerConfig { categorize: true, ..test_config(2) };
    let manager = DownloadManager::new(config).unwrap();

    let id = manager.start(sunucu.url("/veri.xyzzy"), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    // Tanınmayan tür için "Diğer" klasörü açmıyoruz: dosyayı bir kat daha
    // derine gömmek, aramayı zorlaştırmaktan başka bir şey yapmazdı.
    let hedef = PathBuf::from(&snapshot.target_path);
    assert_eq!(hedef.parent(), Some(dir.path()), "kökte kalmalıydı: {}", hedef.display());
}

/// Adrese gömülü `kullanıcı:parola@` gerçekten korumalı bir sunucudan dosya
/// indiriyor mu (karar #20)?
///
/// Birim test kimliğin ayrıldığını gösteriyor; buradaki soru başlığın
/// **her segment isteğine** eklenip eklenmediği. Tek bir segmentte unutulsa
/// 401 gelir ve dosya yarım kalırdı.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adrese_gomulu_kimlikle_indirme_calisiyor() {
    const BOYUT: usize = 256 * 1024;
    let icerik = beklenen_icerik(BOYUT);
    let sunucu = TestSunucusu::baslat(icerik.clone(), SunucuKipi::KimlikIster).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();

    // `http://ali:gizli@127.0.0.1:PORT/korumali.bin`
    let url = sunucu.url("/korumali.bin").replace("http://", "http://ali:gizli@");
    let id = manager.start(url, dir.path().to_path_buf()).unwrap();

    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);
    assert!(snapshot.segments.len() > 1, "korumalı sunucu da segmentlenebilmeli");

    let yazilan = tokio::fs::read(&snapshot.target_path).await.unwrap();
    assert!(yazilan == icerik, "dosya içeriği bozuk");

    // Parola listede, log'da ve metada görünmemeli.
    assert!(!snapshot.url.contains("gizli"), "parola arayüze sızdı: {}", snapshot.url);
    assert!(!snapshot.url.contains('@'), "kimlik bölümü adreste kaldı: {}", snapshot.url);
}

/// Aynı sunucu, kimliksiz adres: indirme başarısız olmalı.
///
/// Bir önceki testin gerçekten kimlik yüzünden geçtiğini kanıtlıyor; sunucu
/// her isteği kabul etseydi o test de geçerdi.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kimliksiz_istek_401_aliyor() {
    let sunucu = TestSunucusu::baslat(beklenen_icerik(64 * 1024), SunucuKipi::KimlikIster).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(2)).unwrap();

    let id = manager.start(sunucu.url("/korumali.bin"), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;

    assert_eq!(durum, DownloadStatus::Failed, "kimliksiz indirme başarılı olmamalı");
}

/// İnen dosyanın özeti gerçekten dosyanın özeti mi (karar #21)?
///
/// Birim testler `checksum::compute`'u bilinen vektörlerle sınıyor; buradaki
/// soru zincirin tamamı: sunucudan inen içerik → diskteki dosya → özet.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inen_dosyanin_ozeti_bilinen_vektorle_ayni() {
    use muiget_lib::download::checksum::{compute, Algorithm};

    // İçerik "abc": SHA-256'sı RFC/NIST örneklerinden bilinen bir değer.
    let sunucu = TestSunucusu::baslat(b"abc".to_vec(), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(4)).unwrap();

    let id = manager.start(sunucu.url("/abc.bin"), dir.path().to_path_buf()).unwrap();
    let durum = tamamlanmayi_bekle(&manager, &id).await;
    let snapshot = manager.get(&id).unwrap();
    assert_eq!(durum, DownloadStatus::Completed, "hata: {:?}", snapshot.error);

    let ozet = compute(&PathBuf::from(&snapshot.target_path), Algorithm::Sha256).await.unwrap();
    assert_eq!(ozet, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

/// Aynı adres ikinci kez eklenince kopya olarak bulunuyor mu (karar #22)?
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ayni_adres_kopya_olarak_bulunuyor() {
    let sunucu = TestSunucusu::baslat(beklenen_icerik(32 * 1024), SunucuKipi::RangeDestekli).await;

    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(test_config(2)).unwrap();

    let url = sunucu.url("/kopya.bin");
    assert!(manager.find_by_url(&url).is_none(), "boş listede kopya olmamalı");

    let id = manager.start(url.clone(), dir.path().to_path_buf()).unwrap();
    tamamlanmayi_bekle(&manager, &id).await;

    let kopya = manager.find_by_url(&url).expect("aynı adres kopya sayılmalı");
    assert_eq!(kopya.id, id);

    // Kimlikli yazım da aynı indirmeyi bulmalı: URL metada parolasız duruyor.
    let kimlikli = url.replace("http://", "http://ali:gizli@");
    assert_eq!(
        manager.find_by_url(&kimlikli).map(|s| s.id),
        Some(id),
        "kimlik bilgisi kopya tespitini bozmamalı"
    );

    // Başka bir adres kopya değil.
    assert!(manager.find_by_url(&sunucu.url("/baska.bin")).is_none());
}
