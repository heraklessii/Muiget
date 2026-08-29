# Tasks — Muiget

Görevler fazlara bölünmüştür. Bir faz bitmeden bir sonrakine geçilmez (küçük
istisnalar `docs/worklog.md`'de not düşülerek yapılabilir). Kutucuk işaretleme:
`[ ]` yapılmadı, `[x]` tamamlandı, `[~]` kısmen/devam ediyor.

---

## Faz 0 — Kurulum ve Dokümantasyon ✅

- [x] Proje adı ve kapsamı netleştirildi (Muiget)
- [x] Teknoloji yığını kararlaştırıldı (Tauri v2, Rust, librqbit, React/Vite)
- [x] `CLAUDE.md` oluşturuldu
- [x] `docs/project_overview.md` oluşturuldu
- [x] `docs/decisions.md` oluşturuldu (#1-#16)
- [x] `docs/worklog.md` oluşturuldu
- [x] `docs/tasks.md` oluşturuldu (bu dosya)
- [x] `LICENSE` dosyası eklendi (Apache 2.0 tam metni)
- [x] `NOTICE` dosyası eklendi
- [x] `README.md` yazıldı
- [x] `.gitignore` eklendi
- [x] Tauri proje iskeleti kuruldu, `npm run build` + `cargo check` temiz
- [x] İlk commit atıldı (`main` dalı, 80 dosya)
- [ ] GitHub reposu oluşturulacak ve remote bağlanacak **(İlker yapacak —
      `gh` CLI kurulu olmadığı için oturumdan oluşturulamadı)**
- [ ] Repo URL'si belli olunca `Cargo.toml`, `README.md` ve `NOTICE` içindeki
      `github.com/ilker/muiget` yer tutucusu düzeltilecek; README'ye CI rozeti
      eklenecek
- [~] `NOTICE` gerçek bağımlılık ağacından yeniden üretilecek — şu an yalnızca
      doğrudan bağımlılıkları listeliyor. **Yayın öncesi zorunlu:**
      `cargo about generate` + `npx license-checker --production`

## Faz 1 — Segmentasyon Motoru ✅

- [x] `Cargo.toml` bağımlılıkları: tokio, reqwest (rustls), futures-util,
      thiserror, tokio-util, uuid, chrono
- [x] `probe()`: HEAD + Range yoklaması ile sunucu yeteneklerini öğrenme
      (`Accept-Ranges`, `Content-Length`, `ETag`, `Last-Modified`,
      `Content-Disposition` dosya adı)
- [x] `plan_segments()`: segment planı + küçük dosyalarda aşırı parçalanmayı
      önleme + artan byte'ları baştaki segmentlere dağıtma
- [x] Sparse file allocation (`set_len`)
- [x] `SegmentWriter`: offsete seek edip doğrudan yazma, eşikli flush
- [x] Tek segment worker: Range isteği + stream + retry/backoff
- [x] `.muiget` resume meta dosyası: atomik kaydetme/yükleme,
      ETag/Last-Modified ile tazelik kontrolü
- [x] Manager: paralel başlatma, ilerleme toplama, duraklat/devam/iptal
- [x] Birim testleri: segment planlama, sparse write offset doğruluğu,
      resume round-trip, dosya adı temizleme, hata sınıflandırma
- [x] Uçtan uca testler: yerel HTTP sunucusuna karşı segmentli indirme, resume,
      bayat meta, duraklat/devam, iptal, Range desteklemeyen sunucu
- [ ] **Manuel test:** gerçek bir büyük dosyayı (ISO/zip) indirip IDM ile hız
      karşılaştırması — otomatik testler doğruluğu gösteriyor, hızı değil

## Faz 2 — Adaptif Optimizasyon ✅

- [x] Segment bazlı hız ölçümü (EWMA, 3 saniye yarı-ömür)
- [x] Yavaş segmenti tespit edip ikiye bölme ("work stealing" ilk sürüm) —
      ölçüt hız değil **tahmini bitiş süresi**
- [x] Bölme yarış koşulunun kilitle çözülmesi (karar #12)
- [x] Bant genişliği sınırlama (token bucket, çalışma anında değiştirilebilir)
- [x] Eşzamanlı indirme sayısı sınırı + kuyruk (karar #16)
- [x] Zaman bazlı hız kuralları (ör. "02:00-08:00 sınırsız")
- [x] Aynı host'a açılan toplam bağlantı sayısını sınırlama (semafor)
- [ ] Segment sayısını indirme sürerken dinamik artırma/azaltma (tam adaptif
      algoritma — ilk sürüm yalnızca boşalan slotu değerlendiriyor)

## Faz 3 — Tauri UI (React/Vite) ✅

- [x] Temel pencere: indirme listesi, ilerleme çubuğu, segment şeridi
- [x] Durum çubuğunda canlı hız grafiği (sparkline, son 1 dakika)
- [x] Yeni indirme diyaloğu: URL yapıştır → otomatik sunucu yoklaması →
      dosya adı/boyut/çoklu bağlantı önizlemesi
- [x] Pause/Resume/Cancel/Kaldır/Klasörde göster
- [x] Ayarlar diyaloğu: segment sayısı, host kotası, yeniden deneme, adaptif
      bölme, genel hız sınırı, zaman kuralları, indirme klasörü
- [x] Sistem tepsisi: menü, sol tıkla pencereyi getir, ipucunda canlı hız,
      kapatınca tepsiye inme (ayardan kapatılabilir)
- [x] Koyu/açık tema (`data-theme`), Mui ailesi paleti, Outfit fontu (gömülü)
- [x] Oturumlar arası liste: `Sırada` rozeti, "Klasörü tara" düğmesi,
      "Açılışta yarım indirmeleri sürdür" anahtarı
- [x] State yönetimi kararı verildi → `docs/decisions.md` #11 (kütüphane yok)
- [x] İşletim sistemi bildirimi (`tauri-plugin-notification`). Pencere odakta
      değilken OS bildirimi, odaktayken uygulama içi toast — ikisi birden
      gösterilmiyor. Başarısız indirmeler de duyuruluyor.
- [x] Klavye kısayolları: Ctrl+N yeni indirme, Ctrl+, ayarlar, Ctrl+F ve `/`
      arama, Esc aramayı temizler. Diyalog açıkken devre dışı.
- [x] İndirme listesinde arama (dosya adı + adres, Türkçe duyarlı) ve sıralama
      (eklenme/ad/boyut/ilerleme)
- [x] Toplu eylem: tümünü duraklat / tümünü sürdür (motorda tek geçişte)
- [ ] Sürükle-bırak ile bağlantı ekleme (tarayıcıdan sürüklenen linki yakalamak)
- [ ] Liste satırında sağ tık menüsü (kopyala, yeniden indir, klasörde göster)

## Faz 4 — Torrent Entegrasyonu

> **Bilinçli olarak ertelendi.** librqbit yaklaşık yüz yeni bağımlılık
> getiriyor ve gerçek bir swarm'a karşı denenmeden doğru çalıştığı söylenemez.
> Yerel HTTP sunucusuyla test edilebilen indirme motorunun aksine, burada
> "yazdım ve derleniyor" yeterli bir kanıt değil.

- [ ] librqbit `Session` kurulumu, Tauri komutlarına bağlama
- [ ] Magnet link ile indirme başlatma
- [ ] `.torrent` dosyası ile indirme başlatma
- [ ] Torrent ilerleme/peer bilgisi UI'a yansıtma (mevcut `DownloadSnapshot`
      soyutlamasının torrent'i de taşıyıp taşıyamayacağına karar verilecek)
- [ ] Sequential download modu (izlerken indirme)
- [ ] Seed oranı/süresi ayarları
- [ ] `NOTICE` güncellemesi (librqbit ve alt bağımlılıkları)

## Faz 5 — Chrome Extension ✅ (uçtan uca manuel doğrulama bekliyor)

- [x] Manifest V3 iskeleti
- [x] Native messaging protokolü (4 byte önek + JSON), boyut sınırı ve
      "temiz EOF / yarım önek" ayrımı testli
- [x] Native messaging host kaydı: manifest yazma + Windows registry (Chrome
      ve Edge), ayarlardan tetikleniyor
- [x] Köprü mimarisi: tek binary, `--native-host` kipi, `--add` ile tek örneğe
      aktarım (karar #13)
- [x] Sağ tık context menu → "Muiget ile indir"
- [x] Sayfa taraması: popup açılınca medya/arşiv bağlantılarını listeler
      (kalıcı content script yok — gizlilik)
- [x] Chrome indirmelerini devralma (opsiyonel izin, varsayılan kapalı)
- [x] `Referer`/`User-Agent` her zaman, `Cookie` opt-in (karar #14)
- [x] Uzantı kimliği doğrulama (32 karakter, a–p) — hem Rust hem arayüz tarafında
- [ ] **Gerçek Chrome ile uçtan uca deneme** — protokol birim testli ama
      Chrome'un host'u gerçekten başlattığı senaryo elle denenmedi
- [ ] Firefox uyarlaması (native messaging protokolü aynı, manifest farklı)
- [ ] Chrome Web Store yayını

## Faz 6 — Medya Özel ve Güven Katmanı

- [ ] HLS/DASH (m3u8) indirme + ffmpeg ile mp4 birleştirme
- [ ] yt-dlp entegrasyonu (opsiyonel dış bağımlılık olarak)
- [ ] İndirilen dosya için checksum gösterimi (MD5/SHA256, UI'da görünür)
- [ ] Opsiyonel virus tarama tetikleme (Windows Defender API veya VirusTotal API,
      varsayılan kapalı, kullanıcı açıkça etkinleştirmeli)

## Faz 7 — Topluluk ve Genişletilebilirlik

- [ ] Plugin sistemi tasarımı: site-özel indirme kuralları için arayüz
- [ ] Plugin örneği: en az bir gerçek site için topluluk katkısı şablonu
- [ ] Lokal istatistik dashboard'u (toplam indirilen veri, yoğun saatler —
      tamamen offline, telemetri yok)
- [ ] Katkı rehberi (`CONTRIBUTING.md`)

## Fazlar Arası / Teknik Borç

Kod yazarken ortaya çıkan, bir faza tam oturmayan işler:

- [x] **Oturumlar arası indirme listesi.** Açılışta indirme klasörü taranıyor,
      yarım indirmeler duraklatılmış kayıt olarak listeye dönüyor (karar #15).
      Ayarlardaki "Klasörü tara" düğmesi başka klasörler için.
- [x] **`DownloadOptions` kalıcılığı.** Başlıklar `ResumeMeta`'ya yazılıyor;
      alan eksik olan eski metalar da okunuyor.
- [x] **Kuyruk yönetimi.** `maxConcurrentDownloads` (varsayılan 3, 0 =
      sınırsız), tek `pump()` üzerinden (karar #16).
- [ ] **Geri yükleme yalnızca tek klasör, tek seviye.** Alt klasörlere inen ya
      da indirme klasörü dışına kaydedilen indirmeler açılışta gelmiyor.
      Bilinçli sınır (karar #15); şikâyet gelirse "son kullanılan klasörler"
      listesi düşünülebilir.
- [x] CI kurulumu (`.github/workflows/ci.yml`): ubuntu'da `npm run build`,
      windows'ta `cargo test` + `cargo clippy -D warnings`. Rust işi arayüzü de
      derliyor, çünkü `generate_context!` derleme anında `dist/` klasörünü arıyor.
- [ ] **CI'da zamanlamaya bağlı test riski.** `duraklat_ve_devam_et_dosyayi_bozmuyor`
      ve benzerleri 80–120 ms bekleyip indirmeyi akarken yakalıyor. Yerelde
      kararlı ama yavaş bir runner'da "duraklatmadan önce hiç veri inmemiş"
      diye patlayabilir. İlk yeşil koşudan sonra birkaç çalıştırma izlenmeli;
      titrerse eşikler yavaş sunucunun parça aralığına göre büyütülmeli.
- [x] `cargo clippy --all-targets` temiz (CI'a eklenmesi bekliyor)
- [ ] Uygulama ikonu (şu an varsayılan Tauri ikonları kullanılıyor)

## Sıradaki (Şu Anki Öncelik)

1. **Gerçek dünya doğrulaması** — kod değil, sahada deneme işi ve artık en
   büyük bilinmeyen:
   - Büyük bir dosyayı (ISO/zip) indirip IDM ile hız karşılaştırması. Otomatik
     testler doğruluğu gösteriyor, hızı değil.
   - Gerçek Chrome ile köprü denemesi. Native messaging protokolü birim testli
     ama Chrome'un host'u gerçekten başlattığı senaryo hiç denenmedi.
2. **Faz 4 (torrent)** — en büyük yeni yüzey; yukarıdaki bitmeden
   başlanmamalı.
3. **Kalan arayüz işleri** — sürükle-bırak ile bağlantı ekleme ve satır sağ tık
   menüsü (Faz 3'ün işaretsiz maddeleri). Küçük ve bağımsız; arada yapılabilir.
