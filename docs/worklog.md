# Worklog — Muiget

Her oturumda yapılan işler burada kronolojik (en yeni en üstte) tutulur.
Yeni bir oturuma başlarken sadece en üstteki girdiyi okumak yeterli olmalı.

Format: Tarih, yapılanlar, kararlar, sıradaki adım.

---

## 2026-08-29 (6. oturum) — İlk Commit, CI ve İlk Yayın Build'i

**İlk commit atıldı.** Dal `master` → `main` olarak yeniden adlandırıldı
(GitHub varsayılanı; boş repoda risksiz). 80 dosya, 20.104 satır. Commit öncesi
staged liste tek tek gözden geçirildi: hassas dosya yok, en büyük dosya 277 KB
(`icon.icns`).

`.gitattributes` eklendi: depoda satır sonları LF, ikili dosyalar dönüşüm dışı.
Olmasaydı Windows'ta yazılan dosyalar Linux'tan katkı verildiğinde "tüm dosya
değişti" diye görünürdü. Kilit dosyaları `linguist-generated` işaretlendi ki
GitHub dil istatistiklerini şişirmesin.

**CI kuruldu** (`.github/workflows/ci.yml`) — teknik borç listesindeki madde:
- ubuntu: `npm ci` + `npm run build`
- windows: `cargo test` + `cargo clippy --all-targets -- -D warnings`

Rust işi arayüzü de derliyor, çünkü `generate_context!` derleme anında
`tauri.conf.json`'daki `frontendDist` (yani `dist/`) klasörünü arıyor; o olmadan
`cargo test` bile başlamıyor. Windows seçildi: projenin birincil hedefi orası ve
Tauri'nin Linux'ta istediği webkit2gtk/gtk paketlerini kurmak gerekmiyor.

**İlk yayın build'i alındı** (`npm run tauri build`, 2 dk 45 sn):

| Çıktı | Boyut |
|---|---|
| `muiget.exe` | 14,1 MB |
| `Muiget_0.1.0_x64_en-US.msi` | 4,9 MB |
| `Muiget_0.1.0_x64-setup.exe` (NSIS) | 3,4 MB |

Tauri, WiX 3.14'ü kendisi indirip MSI'ı üretti. README'deki "Electron'a göre çok
daha küçük binary" iddiası artık ölçülmüş rakamlarla yazıyor; ölçülmemiş RAM
iddiası ise çıkarıldı.

**Duman testi:** yayın binary'si `--native-host` kipinde çalıştırıldı (bu kip
pencere açmıyor), stdin EOF'ta temiz çıktı — çıkış kodu 0. Yani paketlenen
binary yükleniyor ve native messaging yolu ayakta.

**Yapılamayan:** GitHub reposu oluşturulamadı — `gh` CLI kurulu değil ve ortamda
token yok. İlker repoyu github.com'dan açacak (public), URL gelince remote
eklenip push edilecek. `Cargo.toml`, `README.md` ve `NOTICE` içindeki
`github.com/ilker/muiget` yer tutucusu gerçek kullanıcı adına göre güncellenmeli.

**Uygulamanın kendisi hâlâ gerçek pencerede denenmedi.** Build üretiliyor ve
binary yükleniyor ama arayüz yalnızca tarayıcı önizlemesinde görüldü. IDM hız
karşılaştırması, Chrome köprü denemesi ve OS bildirimi de aynı kefede.

---

## 2026-08-29 (5. oturum) — Arayüz: Araç Çubuğu, Kısayollar, OS Bildirimi

Faz 3'ün işaretsiz kalan üç maddesi kapandı, üstüne toplu eylem eklendi.

**Araç çubuğu.** Filtre sekmeleri üst çubuktan alınıp kendi satırına taşındı;
yanına arama, sıralama ve toplu eylemler kondu. Üst çubuk artık yalnızca marka,
"Yeni indirme", tema ve ayarlar taşıyor. Hepsi tek satıra sığmıyordu ve dar
pencerede taşıyordu — araç çubuğu `flex-wrap` ile 800 px altında ikinci satıra
iniyor, kesilmiyor.

**Arama.** Dosya adı ve adreste geçiyor. Türkçe duyarlı: `toLocaleLowerCase('tr')`
kullanılıyor, çünkü `"İ".toLowerCase()` araya birleşik bir nokta koyup
"istanbul" ile eşleşmiyor. Tarayıcıda `İstanbul-raporu-2026.pdf` kaydına karşı
denendi, "istanbul" yazınca eşleşiyor. Sonuç yoksa boş durum sebebi doğru
söylüyor ("Eşleşen indirme yok") ve aramayı temizleme düğmesi veriyor.

**Sıralama.** Eklenme (yeni/eski), ad (A→Z, Türkçe collation), boyut, ilerleme.
Eşitlik bozucu olarak liste sırası taşınıyor: `createdAt` saniye çözünürlüğünde
ve arka arkaya eklenen indirmeler eşit çıkabiliyor.

**Klavye kısayolları** (`hooks/useHotkeys.ts`): Ctrl+N yeni indirme, Ctrl+,
ayarlar, Ctrl+F ve `/` arama, Esc aramayı temizler (ikinci Esc odağı bırakır).
İki kural: yazarken Ctrl'süz kısayollar tetiklenmiyor (arama kutusuna "n" yazmak
diyalog açmasın), diyalog açıkken hiçbiri dinlenmiyor (Esc ile kapanan diyaloğun
üstüne yenisi açılmasın). İkisi de tarayıcıda denendi.

**OS bildirimi** (`tauri-plugin-notification`, `lib/notify.ts`): pencere odakta
değilken işletim sistemi bildirimi, odaktayken uygulama içi toast — ikisi birden
göstermek ekrana bakan kullanıcıya aynı şeyi iki kez söylemek olurdu. İzin
açılışta değil ilk ihtiyaç anında isteniyor. Başarısız indirmeler de artık
duyuruluyor; eskiden yalnızca satırda görünüyordu.

Duyuru mantığı yeniden yazıldı: ölçüt "şu an bu durumda mı" değil **"bu duruma
yeni mi girdi"**. Motor her tick'te aynı durumu yayınlıyor, eski ölçüt bildirimi
yarım saniyede bir tekrarlardı. Son durum indirme başına saklanıyor, böylece
başarısız olup yeniden denenen ve yine başarısız olan bir indirme ikinci kez de
duyuruluyor. Açılıştaki liste referans alınıp duyurulmuyor: geri yüklenen bir
kayıt için "indirildi" demek yanlış olurdu.

**Toplu eylem:** "Tümünü duraklat" / "Tümünü sürdür". Motorda tek geçişte
(`pause_all` / `resume_all`) yapılıyor — arayüzün tek tek çağırması hem N tur
demek olurdu hem de araya biten bir indirme girdiğinde kuyruktan yeni bir tanesi
başlayıp duraklatılmadan kalabilirdi. Uçtan uca test tam bu senaryoyu kuruyor
(sınır 1, üç indirme, biri çalışıyor ikisi kuyrukta).

**Durum çubuğu** artık "2 aktif · 1 sırada" diyor. "Aktif" kuyruktakileri de
sayıyor ve eşzamanlılık sınırı varken bu yanıltıcıydı: 3 aktif görünüp yalnızca
biri iniyor olabiliyordu.

**Doğrulama:**
- 128 birim + 12 uçtan uca test geçiyor, `cargo clippy --all-targets` temiz.
- Arayüz tarayıcıda gerçek bileşenler + gerçek CSS ile denendi (Tauri köprüsü
  sahte veriyle taklit edildi, geçici sayfa sonra silindi): arama/sıralama
  sonuçları tek tek doğrulandı, dört kısayolun dördü de çalışıyor, toplu eylem
  düğmeleri komutu çağırıp bildirim veriyor, araç çubuğu 640/760/800/1000/1038
  px'te taşmıyor ve dar pencerede düzgün sarıyor, koyu ve açık tema ekran
  görüntüsüyle görüldü.

**Bilinen sınır:** OS bildirimi gerçek pencerede denenmedi — tarayıcı
önizlemesinde Tauri eklentisi yok. Windows'ta toast bildirimi uygulamanın
kurulu kimliğine bağlı; `npm run tauri dev` ile bir kez denenmeli. Başarısız
olursa uygulama içi toast'a düşüyor, yani kötü senaryo sessiz kalmak değil.

**Sıradaki adım:** değişmedi — gerçek dünya doğrulaması (IDM hız karşılaştırması
+ gerçek Chrome ile köprü denemesi), sonra Faz 4 (torrent).

---

## 2026-08-29 (4. oturum) — Oturumlar Arası Liste, Kuyruk ve İki Gerçek Hata

Sıradaki listesinin ilk maddesi ve teknik borçtan iki madde kapandı.

**Oturumlar arası indirme listesi.** Uygulama kapanıp açılınca liste boş
geliyordu; `.muiget` dosyaları diskte duruyor ama kimse bakmıyordu. Artık
açılışta indirme klasörü taranıyor ve yarım indirmeler **duraklatılmış**
kayıtlar olarak listeye dönüyor.

- `resume::scan_directory()` — klasördeki `.muiget` dosyalarını topluyor,
  bozuk JSON'u atlıyor, `.mgpart`ı kaybolmuş öksüz metayı silip geçiyor,
  sonucu `createdAt`e göre sıralıyor (liste sırası oturumlar arası korunuyor).
- `DownloadManager::restore()` — bulunanları listeye ekliyor. Aynı klasör iki
  kez taranırsa yinelenen kayıt oluşmuyor (hem kimlik hem hedef yol
  karşılaştırılıyor).
- `lib.rs` bunu pencere açılmadan **önce** senkron çağırıyor: arka planda
  kalsaydı liste bir an boş görünüp sonra dolardı.
- Ayrı bir liste veritabanı bilinçli olarak yok — gerekçe karar #15.

**`DownloadOptions` kalıcılığı** (teknik borç). Uzantıdan gelen `Referer` ve
diğer başlıklar artık `.muiget` metasına yazılıyor. Yazılmasaydı yukarıdaki
geri yükleme yarım işe yarardı: indirme listeye dönerdi ama devam edince
başlıksız gidip 403 alırdı. Eski meta dosyaları alan eksikken de okunuyor
(`#[serde(default)]`), testi var.

**Kuyruk yönetimi** (teknik borç). `maxConcurrentDownloads` ayarı geldi
(varsayılan 3, 0 = sınırsız). `start`/`resume` artık süpervizörü doğrudan
başlatmıyor; isteği kaydın `pending` alanına yazıp tek bir `pump()`'a
bırakıyorlar (karar #16). Fazlası `Sırada` rozetiyle bekliyor, sırası gelince
kendiliğinden başlıyor.

**Testlerin yakaladığı gerçek hatalar:**

1. **Kuyrukta bekleyen indirme devam ettirilince klasörün üstüne yazıyordu.**
   Yeni bir indirme eklendiğinde yalnızca hedef *klasör* biliniyor; dosya adı
   sunucu yoklandıktan sonra belli oluyor. Süpervizörü hiç çalışmamış bir kayıt
   duraklatılıp devam ettirilince `resume`, klasör yolunu dosya yolu sanıyordu.
   Kuyruktan önce bu durum ender, kuyrukla birlikte sıradan hâle geldi.
   `EntryState.resolved` bayrağı ikisini ayırıyor.
   (`kuyrukta_bekleyen_indirme_duraklatilabiliyor` yakaladı.)

2. **İki kayıt aynı dosyaya yazabiliyordu.** `benzersiz_yol`, aynı URL'nin
   yarım indirmesini bilerek tanıyıp aynı yolu döndürüyor — resume buna
   dayanıyor. Ama artık o yarım indirme listede *duruyor* ve kullanıcı
   bağlantıyı ikinci kez yapıştırırsa iki süpervizör aynı dosyaya yazardı.
   Süpervizör artık taze indirmelerde hedefi başka bir kaydın kullanıp
   kullanmadığına bakıyor ve anlaşılır bir hatayla reddediyor.

**Arayüz:**
- Ayarlar → Genel: **Klasörü tara** düğmesi (indirme klasörü değişince ya da
  dosyalar elle taşınınca yeniden açılışı beklemeden taratmak için) ve
  **Açılışta yarım indirmeleri sürdür** anahtarı (varsayılan kapalı).
- Ayarlar → Bağlantı: **Aynı anda indirme** alanı.
- `Sırada` rozeti (kesikli kenar, sessiz renk) — kullanıcı "neden başlamadı?"
  diye sormasın diye.

**Doğrulama:**
- 128 birim + 11 uçtan uca test geçiyor, `cargo clippy --all-targets` temiz.
- Yeni uçtan uca testler yerel sunucuya karşı: eşzamanlılık sınırı hiç
  aşılmıyor (indirme sürerken 10 ms'de bir örnekleniyor), kuyrukta bekleyen
  duraklatılabiliyor, taze bir yönetici yalnızca klasörü tarayarak yarım
  indirmeyi geri yükleyip byte-byte doğru tamamlıyor.
- Arayüz gerçek bileşenler + gerçek CSS ile tarayıcıda denendi (Tauri köprüsü
  sahte veriyle taklit edildi, geçici sayfa sonra silindi): yeni ayar alanları
  taşma yapmıyor, tarama akışı uçtan uca çalışıyor, `Sırada` rozeti koyu ve
  açık temada okunur.

**Kararlar:** `docs/decisions.md` #15 (liste taramayla, ayrı veritabanı yok) ve
#16 (kuyruk yöneticide, tek pompa).

**Bilinçli sınır:** Açılışta yalnızca ayarlardaki indirme klasörü, yalnızca bir
seviye taranıyor. Başka klasöre inen indirmeler için ayarlardaki tarama düğmesi
var; her hedef klasörü kalıcı izlemek, kaçındığımız defteri geri getirirdi.

**Sıradaki adım:** `docs/tasks.md` → "Sıradaki". Öncelik artık **gerçek dünya
doğrulaması**: Faz 1'in IDM hız karşılaştırması ve Faz 5'in gerçek Chrome ile
köprü denemesi. İkisi de kod değil, sahada deneme işi. Sonra Faz 4 (torrent).

**İlker'e kalan (kod dışı):** GitHub reposu ve ilk commit. Hâlâ hiç commit yok.

---

## 2026-08-29 (3. oturum) — Faz 1, 2, 3 ve 5: Motor, Arayüz, Uzantı

Tek oturumda dört faz. Faz 4 (torrent) bilinçli olarak atlandı — gerekçe aşağıda.

**Tasarım dili çıkarıldı:** İlker'in Muita (Electron) ve Muitoon (web)
projelerinin CSS'i incelendi. İkisi de aynı dili konuşuyor: teal `#2dd4bf`
vurgu, `#0f1115` zemin, `#181b22` panel, Outfit fontu, `data-theme` ile tema.
Muiget de aynı aileye alındı (karar #10). Outfit dosyaları Muita'dan kopyalandı
ve `NOTICE`'a OFL-1.1 bildirimi eklendi.

**Faz 1 — segmentasyon motoru** (`src-tauri/src/download/`):
- `http.rs` — iki aşamalı yoklama: HEAD, sonra `GET Range: bytes=0-0`. İkinci
  aşama şart: CDN arkasındaki bazı sunucular HEAD'e `Accept-Ranges` koymuyor
  ama GET'te Range'i uyguluyor.
- `segmenter.rs` — saf plan fonksiyonları, artan byte'lar baştaki segmentlere
  dağıtılıyor
- `writer.rs` — sparse dosya + offsete yazan `SegmentWriter`, `.mgpart` uzantısı
- `resume.rs` — atomik `.muiget` JSON, `Freshness` üçlemesi
  (Fresh / Unverifiable / Stale)
- `worker.rs` — Range isteği, retry + üstel geri çekilme, `200 OK`'i ölümcül
  sayma (sunucu Range'i yok saydıysa dosyanın ortasına yazmak onu bozar)
- `manager.rs` — orkestrasyon, duraklat/devam/iptal

**Faz 2 — adaptif:** `speed.rs` (EWMA), `throttle.rs` (token bucket + host
semaforu + zaman kuralları), manager'da work stealing.

**Faz 3 — arayüz:** Mui paletiyle tam bir kabuk — indirme listesi, segment
şeridi, sparkline, ekleme/ayarlar diyalogları, sistem tepsisi, koyu/açık tema.

**Faz 5 — Chrome uzantısı:** native messaging protokolü (`extension_bridge/`),
MV3 uzantı (`extension/`), sağ tık menüsü, sayfa taraması, indirme devralma.

**Kararlar:** `docs/decisions.md` #10–#14. Özet: Mui tasarım dili, state
kütüphanesi yok, bölme rezervasyon kilidi, tek binary native host, opt-in çerez.

**Testlerin yakaladığı gerçek hatalar** (hepsi düzeltildi):
1. `find_param`'da `?` operatörü, `=` içermeyen ilk parçada (`attachment`) erken
   dönüyordu → `Content-Disposition`'dan dosya adı hiç okunamıyordu.
2. **Adaptif bölmede yarış koşulu.** Uçtan uca test 524.288 byte'lık dosyada
   528.483 byte saydı. Yönetici bölme noktasını hesaplarken worker o noktayı
   geçebiliyordu; aynı byte'lar iki kez sayılıyordu. Çözüm: segment başına
   rezervasyon kilidi (karar #12).
3. Resume, mevcut yarım dosyaya devam edeceğine `devam (1).bin` açıyordu —
   `benzersiz_yol` artık aynı URL'nin metasını tanıyor.
4. `read_exact`, "kanal temiz kapandı" ile "uzunluk öneki yarım kaldı"
   durumlarına aynı hatayı veriyordu; önek elle okunuyor artık.
5. **Arayüzde:** boyutlandırılmamış satır içi SVG'ler 300×150 çiziliyordu ve
   uyarı ikonu sayfayı ele geçiriyordu. Tarayıcı önizlemesinde görüldü —
   ekrana bakmadan fark edilecek bir hata değildi.

**Doğrulama:**
- 116 birim + 7 uçtan uca test geçiyor (`cargo test`)
- Uçtan uca testler elle yazılmış yerel bir HTTP sunucusuna karşı çalışıyor:
  segmentli indirme byte-byte doğrulanıyor, resume/bayat meta/duraklat/iptal
  senaryoları dâhil
- `npm run build` ve `cargo build` temiz; `muiget.exe` üretiliyor
- Arayüz gerçek CSS ile tarayıcıda koyu ve açık temada görsel olarak denendi

**Faz 4 (torrent) neden atlandı:** librqbit ~100 yeni bağımlılık getiriyor ve
gerçek bir swarm'a karşı denenmeden doğru çalıştığı söylenemez. HTTP motorunu
yerel bir sunucuyla test edebiliyoruz; torrent'te "derleniyor" yeterli kanıt
değil. Kendi başına bir oturumu hak ediyor.

**Sıradaki adım:** `docs/tasks.md` → "Sıradaki". Öncelik: (1) oturumlar arası
indirme listesi — uygulama kapanınca yarım indirmeler listeden kayboluyor,
(2) gerçek dünya doğrulaması (IDM hız karşılaştırması + gerçek Chrome ile
köprü denemesi), (3) Faz 4.

**İlker'e kalan (kod dışı):** GitHub reposu ve ilk commit. Hâlâ hiç commit yok.

---

## 2026-08-29 (2. oturum) — Faz 0 Tamamlandı: Lisans Dosyaları + Tauri İskeleti

**Yapılanlar:**
- `LICENSE` — Apache 2.0 tam metni, telif satırı "Copyright 2026 Muiget Contributors".
- `NOTICE` — librqbit (Apache-2.0), Tauri, Tokio, reqwest, Serde, React, Vite,
  TypeScript bildirimleri. Dosyanın sonunda açık bir uyarı var: bu liste
  transitif bağımlılık ağacı **değil**, yayın öncesi `cargo-about` +
  `license-checker` ile yeniden üretilmeli (görev `docs/tasks.md`'de).
- `README.md` — Türkçe, kullanıcıya dönük. "Ne Yapar" bölümünün hemen ardından
  "Ne Yapmaz" bölümü konuldu: limit/kota aşma ve premium servis bypass'ının
  asla eklenmeyeceği README'nin en üst seviyesinde net yazıyor.
- `.gitignore` — `target/`, `node_modules/`, `dist/`, `src-tauri/gen/`, `.env*`
  ve çalışma zamanı `*.muiget` dosyaları.
- **Gerçek proje iskeleti kuruldu:**
  - Frontend elle yazıldı (`npm create vite` yerine): `index.html`,
    `vite.config.ts` (port 1420, strictPort, `src-tauri` watch ignore),
    `tsconfig.json`, `src/main.tsx`, `src/App.tsx`, `src/styles.css`.
  - `npx tauri init --ci` ile `src-tauri/` üretildi; ardından elle düzeltildi:
    crate adı `app` → `muiget`, lib adı `app_lib` → `muiget_lib`,
    `license = "Apache-2.0"`, identifier `com.tauri.dev` → `com.muiget.app`
    (placeholder identifier bundle'ı bloklardı), pencere 800x600 → 1000x680.
  - `app_version` adında tek bir Tauri komutu eklendi; `App.tsx` bunu çağırıp
    Rust ↔ React köprüsünün ayakta olduğunu ekranda gösteriyor.
- **Doğrulama:** `npm run build` ✅ (18 modül, 192 kB bundle) ve `cargo check` ✅
  (33 sn, hatasız). İskelet gerçekten derleniyor, boş klasör değil.

**Kararlar:** `docs/decisions.md` #9 (paket yöneticisi ve iskelet kurulum yöntemi).

**Karşılaşılan sorunlar:**
- TypeScript 7, `tsconfig.node.json`'ı composite referans olarak kabul etmedi
  (`TS6310: may not disable emit`). Çözüm: ayrı node tsconfig'i kaldırıldı, tek
  `tsconfig.json` hem `src`'yi hem `vite.config.ts`'i kapsıyor.
- `TS2882: side-effect import of './styles.css'` → `src/vite-env.d.ts` eklendi.

**Sıradaki adım:** Faz 1 — `src-tauri/Cargo.toml`'a `tokio`/`reqwest`/`thiserror`,
`src-tauri/src/download/` modülü, sonra `probe()` ve `plan_segments()` (testli).

**İlker'e kalan (kod dışı):** GitHub reposunun oluşturulması ve ilk commit.
Şu an `master` branch'inde hiç commit yok, remote tanımlı değil.

**Açık sorular (önceki oturumdan devam):** Frontend state yönetimi (Zustand vs
Context, Faz 3), istatistik verisi saklama (JSON vs SQLite, Faz 5+), virus
tarama entegrasyonu (Faz 6).

---

## 2026-08-29 — Faz 0: Dokümantasyon Kurulumu

**Yapılanlar:**
- Proje adı ve kapsamı netleşti: Muiget — Apache 2.0, Tauri v2, Rust backend,
  librqbit ile torrent, Chrome MV3 extension.
- İlk sohbette mimari tartışıldı: segmentasyon (HTTP Range), sparse file yazma,
  resume mekanizması (.muiget meta dosyası), adaptif segment bölme, torrent
  motoru seçimi (librqbit — Apache 2.0, saf Rust, Tauri deneyimi var).
- Deneme amaçlı bir kod iskeleti yazıldı (segmenter.rs, writer.rs, resume.rs,
  worker.rs, manager.rs) — **ardından bilinçli olarak silindi**. Proje sahibi
  kod yazımından önce sağlam bir dokümantasyon temeli istedi.
- Kurulan dokümanlar: `CLAUDE.md`, `docs/project_overview.md`,
  `docs/decisions.md` (#1-#8), bu dosya, `docs/tasks.md`.
- Ek özellik fikirleri konuşuldu (bkz. `docs/project_overview.md` → Ana Özellik
  Grupları): bant genişliği zamanlayıcısı, torrent sequential download, plugin
  sistemi, lokal istatistik dashboard'u, hash gösterimi, opsiyonel virus tarama.

**Kararlar:** `docs/decisions.md` #1-#8'e bakınız. Özet: Tauri v2 + Rust +
librqbit + React/Vite frontend + Native Messaging extension köprüsü.

**Sıradaki adım:** `docs/tasks.md` → Faz 0'ın kalan maddeleri (LICENSE, NOTICE,
README, boş proje iskeletinin `cargo tauri init` ile gerçek şekilde kurulması)
tamamlanınca Faz 1 (segmentasyon motoru — gerçek kod, testlerle) başlayacak.

**Açık sorular / karara bağlanmamış:**
- Frontend state yönetimi: Zustand mı, React Context mi? (Faz 3'te karar verilecek)
- İstatistik dashboard'u için veri saklama: JSON dosya mı, SQLite mi? (Faz 5+)
- Virus tarama entegrasyonu: Windows Defender API mı, VirusTotal API mı, ikisi
  de opsiyonel mi sunulacak? (Faz 6, henüz düşünce aşamasında)
