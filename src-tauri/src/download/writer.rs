//! Sparse dosya ayırma ve segment yazıcısı (karar #3).
//!
//! Ayrı `.part0`, `.part1`... dosyaları **yok**. Hedef dosya baştan tam
//! boyutuna ayrılıyor ve her worker kendi offsetine yazıyor. Kazanç: birleştirme
//! adımı ortadan kalkıyor (büyük dosyada bir tam okuma + bir tam yazma tasarrufu)
//! ve resume basitleşiyor — parça dosyalarını senkron tutma sorunu kalmıyor.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::Result;

/// İndirme sürerken kullanılan geçici uzantı.
///
/// Yarım dosyanın nihai adını taşıması iki soruna yol açıyor: kullanıcı bozuk
/// dosyayı açmayı deniyor, ve medya tarayıcıları yarım videoyu kütüphaneye
/// ekliyor. `.mgpart` bunu görünür kılıyor; indirme bitince dosya nihai adına
/// taşınıyor.
pub const PART_EXTENSION: &str = "mgpart";

/// `dosya.zip` → `dosya.zip.mgpart`
pub fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(PART_EXTENSION);
    target.with_file_name(name)
}

/// Hedef dosyayı tam boyutuna ayırır.
///
/// `set_len` çoğu modern dosya sisteminde (NTFS, ext4, APFS) **sparse** ayırma
/// yapar: disk blokları gerçekten yazılana kadar tüketilmiyor, ama dosyanın
/// herhangi bir offsetine seek edip yazmak anında mümkün oluyor. Segmentlerin
/// dosyanın ortasından yazmaya başlayabilmesi buna dayanıyor.
///
/// Dosya zaten varsa boyutu ayarlanır ve içeriği **korunur** — resume tam olarak
/// bunu gerektiriyor.
pub async fn allocate(path: &Path, size: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let file = OpenOptions::new().create(true).write(true).truncate(false).open(path).await?;
    file.set_len(size).await?;
    file.sync_all().await?;
    Ok(())
}

/// Tek bir segmentin yazıcısı.
///
/// Her worker'ın **kendi** dosya tanıtıcısı var. Alternatif — tek tanıtıcıyı
/// mutex arkasında paylaşmak — her chunk'ta kilit ve seek demek olurdu; paralel
/// yazmanın anlamı kalmazdı. Ayrı tanıtıcılarla çekirdek zaten kendi offsetine
/// yazan işlemleri birbirinden bağımsız yürütüyor.
pub struct SegmentWriter {
    file: File,
    /// Sıradaki yazmanın gideceği mutlak konum.
    position: u64,
    /// Son `flush`tan bu yana yazılan byte — periyodik flush eşiği için.
    since_flush: u64,
}

impl SegmentWriter {
    /// Dosyayı açar ve imleci `offset`e konumlandırır.
    pub async fn open(path: &Path, offset: u64) -> Result<Self> {
        let mut file = OpenOptions::new().write(true).open(path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(SegmentWriter { file, position: offset, since_flush: 0 })
    }

    /// Chunk'ı yazar ve imleci ilerletir.
    ///
    /// Ek `seek` yok: worker kendi aralığını **sıralı** indiriyor, dolayısıyla
    /// dosya imleci zaten doğru yerde. Seek sadece açılışta ve resume'da gerekli.
    pub async fn write_chunk(&mut self, buf: &[u8]) -> Result<()> {
        self.file.write_all(buf).await?;
        self.position += buf.len() as u64;
        self.since_flush += buf.len() as u64;
        Ok(())
    }

    /// Belirli bir eşiği aşınca tamponu diske indirir.
    ///
    /// Her chunk'ta flush çağırmak indirme hızını diske bağlar. Hiç
    /// çağırmamaksa çökme anında işletim sistemi tamponundaki veriyi kaybettirir
    /// ve `.muiget` meta dosyası diskteki gerçekten ileride kalır — resume bozuk
    /// bir dosyayı "tamam" sanır. Eşik ikisinin ortası.
    pub async fn flush_if_needed(&mut self, threshold: u64) -> Result<()> {
        if self.since_flush >= threshold {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.file.flush().await?;
        self.file.sync_data().await?;
        self.since_flush = 0;
        Ok(())
    }

    pub fn position(&self) -> u64 {
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    async fn oku(path: &Path) -> Vec<u8> {
        let mut buf = Vec::new();
        File::open(path).await.unwrap().read_to_end(&mut buf).await.unwrap();
        buf
    }

    #[test]
    fn part_uzantisi_ekleniyor() {
        let p = part_path(Path::new("/indirmeler/film.mkv"));
        assert_eq!(p.file_name().unwrap(), "film.mkv.mgpart");
        assert_eq!(p.parent().unwrap(), Path::new("/indirmeler"));
    }

    #[tokio::test]
    async fn allocate_dosyayi_tam_boyutuna_ayiriyor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hedef.bin");

        allocate(&path, 4096).await.unwrap();

        let meta = tokio::fs::metadata(&path).await.unwrap();
        assert_eq!(meta.len(), 4096);
    }

    #[tokio::test]
    async fn allocate_olmayan_dizini_olusturuyor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("yeni").join("alt").join("hedef.bin");

        allocate(&path, 128).await.unwrap();

        assert!(path.exists());
        assert_eq!(tokio::fs::metadata(&path).await.unwrap().len(), 128);
    }

    #[tokio::test]
    async fn allocate_mevcut_icerigi_korumali() {
        // Resume senaryosu: yarım dosya var, yeniden allocate çağrılıyor.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("yarim.bin");

        allocate(&path, 16).await.unwrap();
        let mut w = SegmentWriter::open(&path, 0).await.unwrap();
        w.write_chunk(b"MUIGET").await.unwrap();
        w.flush().await.unwrap();

        allocate(&path, 16).await.unwrap();

        assert_eq!(&oku(&path).await[..6], b"MUIGET");
    }

    /// Asıl mesele bu: farklı offsetlere paralel yazan worker'lar birbirinin
    /// verisini ezmemeli ve dosya doğru sırada birleşmeli.
    #[tokio::test]
    async fn paralel_segmentler_dogru_offsetlere_yaziyor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paralel.bin");

        // 4 segment × 256 byte. Her segment kendi indeksinin byte'ıyla dolduruluyor.
        const SEG: usize = 256;
        allocate(&path, (SEG * 4) as u64).await.unwrap();

        let mut tasks = Vec::new();
        for index in 0..4u8 {
            let path = path.clone();
            tasks.push(tokio::spawn(async move {
                let offset = index as u64 * SEG as u64;
                let mut w = SegmentWriter::open(&path, offset).await.unwrap();
                // Parça parça yaz — gerçek indirmede chunk'lar da böyle geliyor.
                for _ in 0..4 {
                    w.write_chunk(&[index; SEG / 4]).await.unwrap();
                }
                w.flush().await.unwrap();
                w.position()
            }));
        }

        for (index, task) in tasks.into_iter().enumerate() {
            let son_konum = task.await.unwrap();
            assert_eq!(son_konum, (index + 1) as u64 * SEG as u64);
        }

        let icerik = oku(&path).await;
        assert_eq!(icerik.len(), SEG * 4);
        for index in 0..4u8 {
            let dilim = &icerik[index as usize * SEG..(index as usize + 1) * SEG];
            assert!(
                dilim.iter().all(|&b| b == index),
                "segment {index} kendi aralığını doldurmamış"
            );
        }
    }

    #[tokio::test]
    async fn resume_offsetinden_devam_ediyor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resume.bin");
        allocate(&path, 10).await.unwrap();

        // İlk oturum: ilk 4 byte.
        let mut w = SegmentWriter::open(&path, 0).await.unwrap();
        w.write_chunk(b"AAAA").await.unwrap();
        w.flush().await.unwrap();
        drop(w);

        // İkinci oturum: kaldığı yerden.
        let mut w = SegmentWriter::open(&path, 4).await.unwrap();
        assert_eq!(w.position(), 4);
        w.write_chunk(b"BBBBBB").await.unwrap();
        w.flush().await.unwrap();

        assert_eq!(oku(&path).await, b"AAAABBBBBB");
    }

    #[tokio::test]
    async fn flush_esik_asilinca_tetikleniyor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flush.bin");
        allocate(&path, 100).await.unwrap();

        let mut w = SegmentWriter::open(&path, 0).await.unwrap();
        w.write_chunk(&[1u8; 10]).await.unwrap();
        assert_eq!(w.since_flush, 10);

        // Eşik altında: sayaç sıfırlanmamalı.
        w.flush_if_needed(64).await.unwrap();
        assert_eq!(w.since_flush, 10);

        w.write_chunk(&[1u8; 60]).await.unwrap();
        w.flush_if_needed(64).await.unwrap();
        assert_eq!(w.since_flush, 0, "eşik aşılınca flush olmalı");
    }
}
