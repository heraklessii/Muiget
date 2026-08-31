//! Parçaları indirip **sırayla** tek dosyaya yazan boru hattı.
//!
//! ## Neden `download::worker` yeniden kullanılmadı
//!
//! Orada bir worker "tek dosyanın şu byte aralığını, şu offsete yaz" diyor;
//! sıra önemli değil çünkü herkes kendi yerine yazıyor (sparse dosya, karar #3).
//! Burada ise yüzlerce **ayrı** dosya var ve çıktı dosyasındaki yerleri ancak
//! kendilerinden öncekilerin boyu bilinince belli oluyor. Yani paralel inmeli
//! ama sıralı yazılmalı.
//!
//! Çözüm `futures_util`in `buffered(N)`i: N parça aynı anda iniyor, sonuçlar
//! **manifest sırasında** teslim ediliyor. Bellek N parçayla sınırlı (tipik bir
//! HLS parçası birkaç MB), ayrı bir sıralama tamponu ya da geçici dosya
//! gerekmiyor.
//!
//! ## Devam etme
//!
//! Yazma sıralı olduğu için devam noktası tek bir sayıdan ibaret: kaç parça
//! tamamlandı. Dosya, o parçaların toplam boyuna **kırpılarak** açılıyor —
//! uygulama meta yazmadan çöktüyse dosyada fazladan kalmış olabilecek son
//! parça böyle atılıyor. Kırpmasaydık aynı parça iki kez yazılır ve video
//! sessizce bozulurdu.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{stream, StreamExt};
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::crypt::{self, KeyStore};
use super::{MediaSegment, MediaTrack};
use crate::download::throttle::{host_of, HostLimiter, RateLimiter};
// Yeniden deneme kuralı motorun kökünde: tek dosya indiren worker ile aynı
// sınıflandırmayı kullanıyoruz, yoksa iki kopya zamanla ayrışırdı.
use crate::download::{yeniden_denenebilir, DownloadError, Result};

#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// Aynı anda kaç parça insin. Host kotası ayrıca geçerli.
    pub concurrency: usize,
    pub max_retries: u32,
    pub read_timeout: Duration,
    pub retry_base_delay: Duration,
    pub max_retry_delay: Duration,
}

impl Default for FetchConfig {
    fn default() -> Self {
        FetchConfig {
            concurrency: 6,
            max_retries: 5,
            read_timeout: Duration::from_secs(30),
            retry_base_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(30),
        }
    }
}

/// Olayın hangi parçadan geldiği.
///
/// Olaylar sınırsız bir kanaldan akıyor: gönderen bir sonraki aşamaya geçtiğinde
/// önceki aşamanın son olayları hâlâ kanalda bekliyor olabiliyor. Aşamayı
/// dışarıdan bir bayrakla takip etmek bu yüzden yanlıştı — videonun son
/// parçaları ses sayacına yazılabiliyordu. Rolü artık olayın kendisi taşıyor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackRole {
    Video,
    Audio,
    Subtitle,
}

/// Boru hattının dışarı bildirdiği olaylar.
#[derive(Debug, Clone)]
pub enum FetchEvent {
    /// Ağdan ham byte geldi. **Yalnızca hız ölçümü** için: yeniden denenen bir
    /// parçanın byte'ları burada iki kez sayılıyor, ilerleme çubuğunun dayanağı
    /// bu değil.
    Bytes(u64),
    /// Bir parça indi, çözüldü ve diske yazıldı. İlerlemenin tek doğru kaynağı.
    SegmentWritten { role: TrackRole, index: usize, bytes: u64, written: u64 },
    Retrying { role: TrackRole, index: usize, attempt: u32, error: String },
}

/// Bir indirmenin tüm parçalarında ortak olan bağlam.
pub struct FetchContext {
    pub client: Client,
    /// Her isteğe eklenen başlıklar (`Referer`, `Cookie`, `Authorization`…).
    /// Anahtar isteği de bunları taşıyor: korumalı bir yayında anahtar da
    /// korumalı oluyor.
    pub headers: Vec<(String, String)>,
    pub rate: Arc<RateLimiter>,
    pub hosts: Arc<HostLimiter>,
    pub keys: Arc<KeyStore>,
    pub cancel: CancellationToken,
    pub config: FetchConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackStats {
    pub bytes: u64,
    /// Bu çağrıda **yeni** yazılan parça sayısı.
    pub segments: usize,
}

/// Bir parçanın tüm segmentlerini indirip `output` dosyasına yazar.
///
/// `start_index` ve `resume_bytes` birlikte devam noktasını belirliyor:
/// dosya `resume_bytes`a kırpılıp oradan yazmaya devam ediliyor.
pub async fn download_track(
    ctx: &FetchContext,
    track: &MediaTrack,
    role: TrackRole,
    output: &Path,
    start_index: usize,
    resume_bytes: u64,
    events: &mpsc::UnboundedSender<FetchEvent>,
) -> Result<TrackStats> {
    if let Some(ust) = output.parent() {
        tokio::fs::create_dir_all(ust).await?;
    }

    let mut dosya = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(output)
        .await?;
    dosya.set_len(resume_bytes).await?;
    dosya.seek(std::io::SeekFrom::Start(resume_bytes)).await?;

    let mut yazilan = resume_bytes;
    let mut sayac = 0usize;

    // fMP4'te başlangıç parçası dosyanın ilk byte'ları olmak zorunda; onsuz
    // dosyada kodek bilgisi yok ve hiçbir oynatıcı açamıyor.
    if start_index == 0 {
        if let Some(init) = &track.init {
            let veri = parcayi_al(ctx, init, role, 0, events).await?;
            dosya.write_all(&veri).await?;
            yazilan += veri.len() as u64;
        }
    }

    let kalan = &track.segments[start_index.min(track.segments.len())..];
    // Akış, parçanın **kendisini** değil sırasını taşıyor. Kapanışa referans
    // vermek, derleyicinin "her ömür için geçerli" (HRTB) bir kapanış istemesine
    // yol açıyor ve bu fonksiyon `tokio::spawn` içinde çağrıldığında kural
    // sağlanamıyordu. `usize` taşımak ömrü kapanışın dışında sabitliyor.
    let mut akis = stream::iter(0..kalan.len())
        .map(|ofset| {
            let index = start_index + ofset;
            async move {
                parcayi_al(ctx, &kalan[ofset], role, index, events)
                    .await
                    .map(|veri| (index, veri))
            }
        })
        .buffered(ctx.config.concurrency.max(1));

    let sonuc = loop {
        let sonraki = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => break Err(DownloadError::Cancelled),
            n = akis.next() => n,
        };

        let Some(parca) = sonraki else { break Ok(()) };
        match parca {
            Ok((index, veri)) => {
                if let Err(e) = dosya.write_all(&veri).await {
                    break Err(DownloadError::Io(e));
                }
                yazilan += veri.len() as u64;
                sayac += 1;
                let _ = events.send(FetchEvent::SegmentWritten {
                    role,
                    index,
                    bytes: veri.len() as u64,
                    written: yazilan,
                });
            }
            Err(e) => break Err(e),
        }
    };

    // Hata ya da iptal olsa da yazılanları diske geçir: devam noktasının
    // dayanağı dosyanın gerçek boyu.
    drop(akis);
    dosya.flush().await?;
    drop(dosya);

    sonuc?;
    Ok(TrackStats { bytes: yazilan, segments: sayac })
}

/// Altyazı parçalarını indirip **bellekte** döner.
///
/// Videodan farklı akmasının sebebi birleştirmenin metin düzeyinde olması:
/// parçalar art arda yazılamıyor, ayrıştırılıp tek bir WebVTT belgesine
/// dönüştürülüyor (bkz. [`super::vtt`]). Bellek riski yok — bir saatlik filmin
/// altyazısı birkaç yüz KB.
pub async fn fetch_subtitle_parts(
    ctx: &FetchContext,
    track: &MediaTrack,
    events: &mpsc::UnboundedSender<FetchEvent>,
) -> Result<Vec<Vec<u8>>> {
    let toplam = track.segments.len();
    let mut akis = stream::iter(0..toplam)
        .map(|index| async move {
            parcayi_al(ctx, &track.segments[index], TrackRole::Subtitle, index, events)
                .await
                .map(|veri| (index, veri))
        })
        .buffered(ctx.config.concurrency.max(1));

    let mut parcalar: Vec<Vec<u8>> = Vec::with_capacity(toplam);
    loop {
        let sonraki = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => return Err(DownloadError::Cancelled),
            n = akis.next() => n,
        };
        let Some(sonuc) = sonraki else { break };
        let (index, veri) = sonuc?;
        let boyut = veri.len() as u64;
        parcalar.push(veri);
        let _ = events.send(FetchEvent::SegmentWritten {
            role: TrackRole::Subtitle,
            index,
            bytes: boyut,
            written: 0,
        });
    }
    Ok(parcalar)
}

/// Tek bir parçayı indirir; hata alırsa üstel geri çekilmeyle yeniden dener.
async fn parcayi_al(
    ctx: &FetchContext,
    segment: &MediaSegment,
    role: TrackRole,
    index: usize,
    events: &mpsc::UnboundedSender<FetchEvent>,
) -> Result<Vec<u8>> {
    let mut deneme = 0u32;

    loop {
        if ctx.cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }

        match tek_deneme(ctx, segment, events).await {
            Ok(veri) => return Ok(veri),
            Err(e) if !yeniden_denenebilir(&e) => return Err(e),
            Err(e) => {
                deneme += 1;
                if deneme > ctx.config.max_retries {
                    return Err(DownloadError::Other(format!(
                        "parça {index}: {} deneme sonrası vazgeçildi: {e}",
                        ctx.config.max_retries
                    )));
                }
                let _ = events.send(FetchEvent::Retrying {
                    role,
                    index,
                    attempt: deneme,
                    error: e.to_string(),
                });

                tokio::select! {
                    _ = tokio::time::sleep(geri_cekilme(&ctx.config, deneme)) => {}
                    _ = ctx.cancel.cancelled() => return Err(DownloadError::Cancelled),
                }
            }
        }
    }
}

async fn tek_deneme(
    ctx: &FetchContext,
    segment: &MediaSegment,
    events: &mpsc::UnboundedSender<FetchEvent>,
) -> Result<Vec<u8>> {
    // Host kotası: bu fonksiyondan çıkarken (hata dâhil) bırakılıyor.
    let _permit = ctx.hosts.acquire(&host_of(&segment.url)).await;

    let mut istek = ctx.client.get(&segment.url);
    for (ad, deger) in &ctx.headers {
        istek = istek.header(ad, deger);
    }
    if let Some(aralik) = segment.range {
        istek = istek.header(RANGE, aralik.header());
    }

    let yanit = istek.send().await?;
    let durum = yanit.status();
    if !durum.is_success() {
        return Err(DownloadError::HttpStatus { status: durum.as_u16() });
    }
    // Aralık istendiği hâlde sunucu dosyanın tamamını gönderdiyse yazılan parça
    // yanlış olurdu — tek dosyaya paketlenmiş fMP4 akışlarda bu sessiz bozulma
    // demek.
    if segment.range.is_some() && durum != StatusCode::PARTIAL_CONTENT {
        return Err(DownloadError::RangeIgnored { segment: segment.sequence as usize });
    }

    let mut govde: Vec<u8> = Vec::with_capacity(yanit.content_length().unwrap_or(0) as usize);
    let mut akis = yanit.bytes_stream();

    // Hız olayları biriktiriliyor, chunk başına gönderilmiyor: eşikler ve
    // gerekçesi `download::worker`da (aynı sorun, aynı sayılar). Burada ayrı
    // bir tip yerine iki yerel değişken var, çünkü tek çıkış noktası bu
    // fonksiyonun sonu — worker'daki yedi `return` gibi bir dağınıklık yok.
    let mut birikmis = 0u64;
    let mut son_gonderim = std::time::Instant::now();

    loop {
        let sonraki = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => return Err(DownloadError::Cancelled),
            chunk = tokio::time::timeout(ctx.config.read_timeout, akis.next()) => chunk,
        };

        match sonraki {
            Ok(Some(Ok(chunk))) => {
                if chunk.is_empty() {
                    continue;
                }
                ctx.rate.consume(chunk.len() as u64).await;
                birikmis += chunk.len() as u64;
                if birikmis >= crate::download::worker::ILERLEME_ESIGI_BYTE
                    || son_gonderim.elapsed() >= crate::download::worker::ILERLEME_ESIGI_SURE
                {
                    let _ = events.send(FetchEvent::Bytes(birikmis));
                    birikmis = 0;
                    son_gonderim = std::time::Instant::now();
                }
                govde.extend_from_slice(&chunk);
            }
            Ok(Some(Err(e))) => return Err(DownloadError::Network(e)),
            Ok(None) => break,
            Err(_) => {
                return Err(DownloadError::Other(format!(
                    "{} saniyedir veri gelmiyor",
                    ctx.config.read_timeout.as_secs()
                )))
            }
        }
    }

    // Kalan birikmiş byte'lar: hız göstergesinin parçanın son kırıntısını da
    // görmesi için. Hata yollarında gönderilmiyor — o byte'lar zaten
    // yeniden denemede baştan sayılacak.
    if birikmis > 0 {
        let _ = events.send(FetchEvent::Bytes(birikmis));
    }

    if let Some(anahtar_bilgisi) = &segment.key {
        let anahtar = ctx
            .keys
            .get(&ctx.client, &anahtar_bilgisi.uri, &ctx.headers)
            .await?;
        let iv = anahtar_bilgisi
            .iv
            .unwrap_or_else(|| crypt::iv_from_sequence(segment.sequence));
        crypt::decrypt_aes128_cbc(&anahtar, &iv, &mut govde)?;
    }

    Ok(govde)
}

fn geri_cekilme(config: &FetchConfig, deneme: u32) -> Duration {
    let carpan = 2u32.saturating_pow(deneme.saturating_sub(1).min(16));
    config
        .retry_base_delay
        .saturating_mul(carpan)
        .min(config.max_retry_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gecici_hatalar_yeniden_deneniyor() {
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 500 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 503 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 429 }));
        assert!(yeniden_denenebilir(&DownloadError::HttpStatus { status: 408 }));
        assert!(yeniden_denenebilir(&DownloadError::Other("zaman aşımı".into())));
    }

    #[test]
    fn kalici_hatalar_yeniden_denenmiyor() {
        assert!(!yeniden_denenebilir(&DownloadError::HttpStatus { status: 404 }));
        assert!(!yeniden_denenebilir(&DownloadError::HttpStatus { status: 403 }));
        assert!(!yeniden_denenebilir(&DownloadError::Cancelled));
        assert!(!yeniden_denenebilir(&DownloadError::Drm("x".into())));
        // Yanlış anahtar her denemede aynı sonucu verir.
        assert!(!yeniden_denenebilir(&DownloadError::Manifest("dolgu geçersiz".into())));
    }

    #[test]
    fn geri_cekilme_ikiye_katlanip_tavana_dayaniyor() {
        let c = FetchConfig {
            retry_base_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(4),
            ..FetchConfig::default()
        };
        assert_eq!(geri_cekilme(&c, 1), Duration::from_millis(500));
        assert_eq!(geri_cekilme(&c, 2), Duration::from_secs(1));
        assert_eq!(geri_cekilme(&c, 3), Duration::from_secs(2));
        assert_eq!(geri_cekilme(&c, 4), Duration::from_secs(4));
        // Tavan aşılmıyor ve taşma olmuyor.
        assert_eq!(geri_cekilme(&c, 40), Duration::from_secs(4));
    }
}
