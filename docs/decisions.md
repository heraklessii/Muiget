# Mimari Kararlar (Decisions Log)

Bu dosya, projede alınan önemli teknik kararları kronolojik sırayla, gerekçesiyle
birlikte tutar. Yeni bir karar alındığında dosyanın **sonuna** eklenir, eski
kararlar silinmez (fikir değiştirilirse "Durum: Değiştirildi → bkz. #N" notu
düşülür).

Format: her karar bir başlık, Durum, Bağlam, Karar, Gerekçe, Alternatifler içerir.

---

## #1 — Masaüstü Kabuk: Tauri v2

**Durum:** Kabul edildi

**Bağlam:** Uygulama hem Windows'ta (İlker'in ana ortamı, Muitoon sunucusu da
Windows Server üzerinde) hem de mümkünse macOS/Linux'ta çalışmalı. Electron
yaygın ama RAM/binary boyutu ağır.

**Karar:** Tauri v2 kullanılacak. Rust backend + web tabanlı (React/Vite) frontend.

**Gerekçe:**
- Electron'a göre çok daha düşük bellek kullanımı ve küçük binary boyutu
- Rust native performans — segment worker'ları için ideal (async, güvenli bellek)
- İlker'in daha önceki live wallpaper projesinde Tauri v2 deneyimi zaten var
- librqbit projesi (torrent motoru adayı) zaten kendi masaüstü uygulamasını
  Tauri ile yapmış — entegrasyon riski düşük

**Alternatifler:**
- Electron: Daha olgun ekosistem ama ağır, Node.js native modül bağımlılığı
  torrent kütüphaneleri için sorun çıkarabilir
- Saf Qt/GTK (C++): Performans iyi ama geliştirme hızı düşük, İlker'in web
  tabanlı UI deneyimiyle uyumsuz

---

## #2 — Torrent Motoru: librqbit

**Durum:** Kabul edildi

**Bağlam:** Torrent desteği gerekiyor. Sıfırdan BitTorrent protokolü yazmak
(peer protokolü, DHT, bencode, piece seçimi vb.) aylar sürer ve proje kapsamını
aşar.

**Karar:** `librqbit` (Rust crate, Apache-2.0) kullanılacak.

**Gerekçe:**
- Apache-2.0 lisanslı — proje lisansımızla birebir uyumlu, ek NOTICE yükümlülüğü
  dışında sorun yok
- Saf Rust, FFI/C++ bağımlılığı yok (libtorrent'e FFI'dan çok daha temiz)
- DHT, magnet link, HTTP API desteği hazır
- Zaten bir Tauri masaüstü uygulamasında (rqbit'in kendi projesi) kullanılmış,
  entegrasyon paterni kanıtlanmış
- Aktif bakım altında, 17K+ aylık indirme (crates.io)

**Alternatifler:**
- `cratetorrent`: Daha az olgun, bakımı belirsiz
- C++ `libtorrent`'e Rust FFI: Daha olgun/battle-tested ama build karmaşıklığı
  (Windows'ta özellikle) ve unsafe FFI yüzeyi yüksek
- Kendi yazımı: Kapsam dışı, zaman maliyeti çok yüksek

**Bilinen sınırlamalar (librqbit):** Yalnızca BitTorrent V1 destekliyor (V2 yok),
ağ değişiminde (ör. wifi'den ethernet'e geçiş) otomatik yeniden bağlanma yok.
İlerleyen fazlarda bu sınırlar yeniden değerlendirilecek.

---

## #3 — Segmentasyon Stratejisi: HTTP Range + Sparse File

**Durum:** Kabul edildi

**Bağlam:** IDM'in hız avantajının kaynağı çoklu paralel bağlantı. Bunu meşru
şekilde (limit/kota aşma olmadan, salt HTTP protokolü seviyesinde) uygulamak
gerekiyor.

**Karar:**
1. İndirme başlamadan önce `HEAD` isteği atılır → `Accept-Ranges: bytes` ve
   `Content-Length` kontrol edilir.
2. Destekleniyorsa dosya varsayılan 8 parçaya bölünür (`docs/project_overview.md`
   → "Çekirdek İndirme Motoru").
3. Hedef dosya **baştan tam boyutuna** `set_len` ile ayrılır (sparse file).
4. Her segment kendi async task'ında, dosyanın kendi offsetine `seek+write`
   yapar. Ayrı geçici parça dosyaları ve sonradan birleştirme adımı **yok**.

**Gerekçe:** Ayrı parça dosyalarıyla çalışıp sonradan birleştirmek ekstra disk
I/O (bir kere parça olarak yaz, bir kere birleştirirken oku+yaz) demek. Doğrudan
offsete yazmak bu maliyeti ortadan kaldırır ve resume'u basitleştirir (parça
dosyalarının senkronizasyonu sorunu yok).

**Alternatifler:**
- Ayrı `.part0`, `.part1`... dosyaları + birleştirme: Basit ama disk I/O israfı
- Bellekte tutup tek seferde yazma: Büyük dosyalarda RAM patlar, kapsam dışı

---

## #4 — Resume Mekanizması: `.muiget` JSON Meta Dosyası

**Durum:** Kabul edildi

**Bağlam:** Uygulama kapanabilir/çökebilir; kullanıcı kaldığı yerden devam
edebilmeli, sıfırdan başlamamalı.

**Karar:** Her indirme için `<hedef_dosya_adı>.muiget` adında bir JSON dosyası
tutulur. İçeriği: indirme ID'si, URL, sunucu yetenekleri (ETag, Last-Modified,
boyut), her segmentin durumu ve indirilen byte sayısı.

**Gerekçe:** JSON insan-okunur, debug kolay, ekstra veritabanı bağımlılığı
gerektirmiyor. `ETag`/`Last-Modified` karşılaştırması sayesinde sunucudaki
dosya değiştiyse (resume dosyası eskimişse) sıfırdan başlanacağı anlaşılabilir
(bu kontrol mantığı Faz 1'de eklenecek, henüz iskelet planında not olarak duruyor).

**Alternatifler:**
- SQLite: Çoklu indirme yönetimi için daha güçlü ama tek dosya JSON kadar basit
  değil; indirme sayısı arttıkça (ilerleyen faz) SQLite'a geçiş değerlendirilebilir
- Binary format: Debug edilebilirlik kaybı, kazanç marjinal

---

## #5 — Adaptif Segment Yeniden Dağıtımı ("Work Stealing")

**Durum:** Kabul edildi (ilk basit sürüm)

**Bağlam:** Bazı segmentler diğerlerinden çok daha yavaş inebilir (sunucu
tarafı throttling, ağ yolu farkı). Sabit segment sayısıyla devam etmek toplam
hızı düşürür.

**Karar:** İlk sürümde basit strateji: bir segment sürekli hata veriyorsa ya
da diğerlerine göre belirgin yavaşsa, kalan byte aralığı ikiye bölünüp yeni bir
worker'a devredilir.

**Gerekçe:** Basit ve anlaşılır bir başlangıç. Tam "iş çalma" (work-stealing
queue, boşta kalan worker'ların otomatik iş alması) daha karmaşık ve şimdilik
gereksiz — ilk sürümde çalışan bir MVP'ye öncelik veriliyor.

**İleride değerlendirilecek:** Segment sayısını dosya indirilirken dinamik
olarak artırıp azaltan tam adaptif algoritma (Faz 2+).

---

## #6 — Chrome Extension ↔ Masaüstü Köprüsü: Native Messaging

**Durum:** Kabul edildi

**Bağlam:** Extension, yakaladığı indirme linklerini masaüstü uygulamasına
güvenli şekilde iletmeli. Chrome extension'ları doğrudan yerel dosya sistemine
ya da rastgele portlara erişemez (MV3 kısıtlamaları).

**Karar:** Chrome'un resmi **Native Messaging** API'si kullanılacak — extension,
manifest'te tanımlı bir "native host" ile stdin/stdout üzerinden length-prefixed
JSON mesajlaşır. Masaüstü uygulaması bu native host'u kendi kurulumunda register eder.

**Gerekçe:** Chrome'un desteklediği tek resmi, güvenli, kullanıcı onayı
gerektiren yerel IPC yöntemi. Yerel bir HTTP sunucusu açmaktan (port çakışması,
CORS, güvenlik yüzeyi riski) daha temiz.

**Alternatifler:**
- Lokal HTTP sunucusu (localhost:PORT): Daha esnek ama güvenlik yüzeyi daha
  geniş (herhangi bir sekme bu porta istek atabilir), port çakışma riski var
- WebSocket: Benzer riskler, native messaging'in sağladığı OS-seviyesi izolasyonu
  sağlamıyor

---

## #7 — Lisans: Apache License 2.0

**Durum:** Kabul edildi

**Bağlam:** Proje bilinçli olarak açık kaynak ve IDM'in kapalı/ücretli modeline
alternatif olarak konumlandırılıyor.

**Karar:** Hem ana uygulama hem Chrome extension Apache 2.0 ile lisanslanacak.

**Gerekçe:**
- Ticari kullanım dahil geniş izin veriyor (topluluk büyümesini teşvik eder)
- Patent hibesi içeriyor (MIT'te yok) — üçüncü taraf katkılarında ekstra güvence
- librqbit de Apache-2.0, lisans uyumluluğu sorunsuz

**Not:** `NOTICE` dosyasında librqbit ve diğer üçüncü parti Apache/MIT
bağımlılıkları listelenecek (Faz 0 görevi, `docs/tasks.md`'de var).

---

## #8 — Dokümantasyon-Önce Yaklaşım (Bu Fazın Kararı)

**Durum:** Kabul edildi

**Bağlam:** İlker, kod yazımından önce projenin net bir CLAUDE.md, karar
geçmişi, iş günlüğü ve görev listesiyle başlamasını istedi.

**Karar:** Faz 0 tamamen dokümantasyona ayrılıyor: `CLAUDE.md`,
`docs/project_overview.md`, `docs/decisions.md`, `docs/worklog.md`,
`docs/tasks.md`. Kod yazımı bu dosyalar netleşmeden başlamayacak.

**Gerekçe:** Solo geliştirici + AI asistan ile çalışırken oturumlar arası
bağlam kaybını önlemenin en ucuz yolu iyi doküman. İlk sohbette segmenter/writer/
resume/worker/manager modülleri taslak olarak yazılmıştı ama bu kararla
bilinçli olarak silindi — önce plan, sonra kod.

---

## #9 — Paket Yöneticisi ve İskelet Kurulum Yöntemi

**Durum:** Kabul edildi

**Bağlam:** Faz 0'ın son adımı gerçek proje iskeletini kurmaktı. İki alt karar
gerekti: (a) Tauri CLI nereden gelecek, (b) frontend nasıl scaffold edilecek.

**Karar:**
1. **Paket yöneticisi: npm.** Ekstra bir araç kurulumu gerektirmiyor, Node 24
   ile geliyor.
2. **Tauri CLI, `cargo install tauri-cli` yerine `@tauri-apps/cli` npm
   devDependency'si olarak.** Komutlar `npx tauri ...` / `npm run tauri ...`
   üzerinden çalışıyor.
3. **Frontend elle yazıldı, `npm create vite` kullanılmadı.**

**Gerekçe:**
- npm paketi hazır derlenmiş binary indiriyor (saniyeler); `cargo install
  tauri-cli` kaynaktan derliyor (dakikalar). Ayrıca CLI sürümü `package.json`'da
  sabitlendiği için her katkıcı aynı sürümü alıyor — global `cargo install`'da
  bu garanti yok.
- `npm create vite` boş olmayan bir dizinde "mevcut dosyaları sil?" diye
  soruyor; bu dizinde zaten `CLAUDE.md` ve `docs/` vardı. Elle yazmak hem bu
  riski ortadan kaldırdı hem de `vite.config.ts`'e `src-tauri` watch ignore ve
  strictPort gibi Tauri'ye özel ayarların baştan doğru girmesini sağladı.

**Uygulanan düzeltmeler (tauri init çıktısı üzerinde):**
- Crate adı `app` → `muiget`, lib adı `app_lib` → `muiget_lib`
- `license = "Apache-2.0"`, gerçek açıklama ve repository alanı
- Bundle identifier `com.tauri.dev` → `com.muiget.app` — placeholder identifier
  ile `tauri build` çalışmıyor, bunu şimdiden düzeltmek ileride sürpriz olmasını
  önlüyor

**Alternatifler:**
- pnpm/yarn: Daha hızlı/disk dostu ama sistemde kurulu değil, ek kurulum adımı
  ve katkıcılar için ek gereksinim demek. İhtiyaç doğarsa geçilebilir.
- `cargo install tauri-cli`: Node bağımlılığı olmayan bir dünyada mantıklı ama
  frontend zaten Node gerektiriyor, kazancı yok.

**Not:** TypeScript 7 ile `tsconfig.node.json` composite referansı çalışmıyor
(`TS6310`), bu yüzden tek bir `tsconfig.json` hem `src`'yi hem `vite.config.ts`'i
kapsıyor.

---

## #10 — Tasarım Dili: Mui Ailesi Ortak Paleti

**Durum:** Kabul edildi

**Bağlam:** Muiget'in arayüzü sıfırdan tasarlanacaktı. İlker'in iki mevcut
projesi var: **Muita** (Electron masaüstü uygulaması) ve **Muitoon** (web).
İkisi de aynı görsel dili konuşuyor.

**Karar:** Muiget de aynı dili kullanacak:

| Jeton | Koyu | Açık |
|---|---|---|
| Vurgu | `#2dd4bf` (teal) | `#0d9488` |
| Zemin | `#0f1115` | `#eef1f5` |
| Panel | `#181b22` | `#ffffff` |
| Metin | `#e8eaed` | `#12151a` |

Yazı tipi **Outfit** (değişken, `latin` + `latin-ext` alt kümeleri), tema
geçişi `<html data-theme>` üzerinden. Jeton adları Muita'nınkiyle hizalandı
(`--bg-panel`, `--accent-soft`, `--on-accent`...).

**Gerekçe:**
- Aynı geliştiricinin üç ürünü aynı aileden görünüyor; kullanıcı Muita'dan
  Muiget'e geçince yabancılık çekmiyor.
- Palet zaten hem koyu hem açık temada okunurluk açısından denenmiş; sıfırdan
  bir palet üretip aynı olgunluğa gelmek boşa emek olurdu.
- Outfit dosyaları **gömülü** (`src/assets/fonts/`), CDN yok: bir indirme
  yöneticisinin kendi arayüzü için ağa çıkması saçma olurdu ve uygulama
  tamamen çevrimdışı açılabilmeli.

**Alternatifler:**
- Hazır bir bileşen kütüphanesi (MUI, shadcn): hızlı başlangıç ama paket boyutu
  ve "her uygulama gibi görünme" bedeli; Tauri'nin küçük binary vaadiyle çelişir.
- Yeni bir palet: kimlik açısından gereksiz ayrışma.

**Not:** Outfit **OFL-1.1** lisanslı, Apache 2.0 değil. `NOTICE` dosyasında ayrı
bir "BUNDLED FONTS" bölümünde belirtiliyor.

---

## #11 — Frontend State Yönetimi: Kütüphane Yok

**Durum:** Kabul edildi (`docs/worklog.md`'deki "Zustand mı Context mi" açık
sorusunun cevabı)

**Bağlam:** Faz 3 planında "state yönetimi kararı verilecek (Zustand vs
Context)" maddesi vardı.

**Karar:** Ne Zustand ne Context. Durum React'in kendi `useState`/`useMemo`'su
ve tek bir özel hook (`useDownloads`) ile yönetiliyor.

**Gerekçe:** Asıl kavrayış şu: **tek gerçek kaynak Rust motoru.** Arayüz
kendi durumunu tutmuyor, motorun yayınladığı anlık görüntüleri gösteriyor.
Global bir store, sunucudan gelen veriyi bir kez daha kopyalamaktan ibaret
olurdu — senkron tutulacak ikinci bir gerçek yaratmak, çözdüğünden fazla
sorun çıkarır.

Pratikte paylaşılan tek şey indirme listesi ve ayarlar; ikisi de `App`'te
duruyor ve iki seviye aşağı geçiliyor. Prop drilling sorunu oluşacak kadar
derin bir ağaç yok.

**Alternatifler:**
- Zustand: 1 KB'lık bir kütüphane için bile gerekçe yok; durum zaten uzakta.
- Context: gereksiz yeniden çizim riski, kazancı yok.

**Yeniden değerlendirme koşulu:** Torrent (Faz 4) ve istatistik paneli (Faz 7)
eklendiğinde ağaç derinleşirse bu karar yeniden ele alınacak.

---

## #12 — Segment Bölmede Rezervasyon Kilidi

**Durum:** Kabul edildi

**Bağlam:** Karar #5'teki adaptif bölme ilk uygulamasında ilerleme %100'ü
aşıyordu: uçtan uca test 524.288 byte'lık dosyada 528.483 byte saydı.

**Sebep bir yarış koşuluydu:** yönetici kurbanın `cursor`unu okuyup bölme
noktasını hesaplarken worker yazmaya devam ediyor. Yönetici yeni sınırı
yazana kadar worker bölme noktasını geçmiş olabiliyor ve aynı byte'lar hem
kurbanda hem çalınan segmentte sayılıyor.

**Karar:** Her segmentin bir `split_lock`u (`Mutex<()>`) var. Kilit altında
yapılan iş:
- **Worker:** sınırı oku → yazacağı byte kadarını `downloaded`a ekle (rezerve et)
- **Yönetici:** `cursor`u oku → bölme noktasını hesapla → yeni sınırı yaz

Disk yazması kilidin **dışında**: rezervasyon senkron ve nanosaniyeler sürüyor,
yönetici pratikte hiç beklemiyor. Yazma başarısız olursa rezervasyon geri
alınıyor (`fetch_sub`), yoksa `downloaded` diskteki gerçekten ileride kalır ve
yeniden deneme dosyada delik bırakır.

**Gerekçe:** Atomik değişkenler tek başına yetmiyor çünkü korunması gereken şey
tek bir değer değil, **iki değer arasındaki ilişki** (sınır ve imleç). Kilit
kapsamı bunun için mümkün olan en dar hâlde tutuldu.

**Alternatifler:**
- Bölmeyi tamamen worker'a yaptırmak: yönetici çalınan aralığı hemen
  öğrenemezdi, yeni worker'ı başlatamazdı.
- Bölmeden vazgeçmek: karar #5'in tüm kazancını çöpe atardı.

---

## #13 — Native Host, Ayrı Bir Daemon Değil, Aynı Binary + Tek Örnek

**Durum:** Kabul edildi

**Bağlam:** Karar #6 native messaging'i seçmişti ama bir soru açıktı: Chrome'un
başlattığı köprü süreci, indirmeyi **çalışan** uygulamaya nasıl iletecek?
Chrome host'u kendisi başlatıyor ve o süreç uygulamanın kendisi değil.

**Karar:** Ayrı bir host binary'si yok. Aynı `muiget` çalıştırılabiliri iki kipte
çalışıyor:

```text
Chrome ──(stdio)──> muiget --native-host ──(argv)──> çalışan Muiget penceresi
```

`--native-host` ile başlatıldığında pencere açılmıyor, yalnızca stdio döngüsü
işliyor. Gelen her istek `muiget --add <base64-json>` olarak yeniden
çağrılıyor; `tauri-plugin-single-instance` bu argümanı zaten açık olan pencereye
taşıyor ve ikinci süreç kapanıyor. Uygulama kapalıysa bu süreç ilk örnek olup
kendi argümanını işliyor.

**Düzeltme (29 Ağu 2026):** Köprü kipini yalnızca `--native-host` bayrağı
açıyordu, ama Chrome bu bayrağı hiçbir zaman geçirmiyor: native messaging
manifestinde argüman alanı yok, komut
`muiget.exe chrome-extension://<id>/ --parent-window=<handle>` biçiminde
çalıştırılıyor. Yani gerçek Chrome her denemede stdio köprüsü yerine boş bir
pencere açardı — birim testler protokolü doğruladığı için hata sahada
denenmeden görünmüyordu. Kip artık `native_host::is_host_invocation()` ile
belirleniyor: `chrome-extension://` önekli argüman **ya da** elle verilen
`--native-host` köprüyü açıyor; `--add` her zaman pencereye gidiyor.

**Gerekçe:**
- Tek binary: dağıtımda ikinci bir dosya, ikinci bir sürüm uyumu sorunu yok.
- Köprü süreci **durumsuz ve kısa ömürlü**; MV3 service worker'ı zaten uykuya
  daldığı için kalıcı bir port açmanın anlamı yoktu.
- Yerel port açılmıyor — karar #6'daki güvenlik gerekçesi korunuyor.
- Base64: URL'ler boşluk, `&`, tırnak içerebiliyor; ham JSON'u komut satırına
  koymak platformlar arasında farklı şekillerde bozulurdu.

**Alternatifler:**
- Ayrı host binary + yerel soket: iki dosya, iki sürüm, ek IPC yüzeyi.
- Köprünün indirmeyi kendisinin yapması: uygulama açık değilken indirme
  ilerlemesi hiçbir yerde görünmezdi.

---

## #14 — Uzantıdan Gelen Başlıklar: Referer Evet, Çerez Opt-in

**Durum:** Kabul edildi

**Bağlam:** Tarayıcıdan yakalanan bir bağlantı çıplak URL olarak indirilince
çoğu sitede 403 dönüyor: sunucu `Referer` bekliyor, bazen oturum çerezi
bekliyor. Motorun bu başlıkları taşıyabilmesi gerekti.

**Karar:**
- İndirme motoruna indirme başına `headers: Vec<(String, String)>` eklendi
  (`DownloadOptions`). Yoklama (`probe`) da **aynı** başlıkları gönderiyor —
  yoksa yoklama 403 alıp indirme daha başlamadan yanlış sonuçla biterdi.
- Uzantı `Referer` ve `User-Agent`'ı **her zaman**, `Cookie`'yi **yalnızca
  kullanıcı açıkça açtıysa** gönderiyor. Çerez izni Chrome'da isteğe bağlı
  (`optional_permissions`) ve varsayılan kapalı.

**Gerekçe:** `Referer` zaten sayfanın adresi — tarayıcı normal indirmede de
gönderiyor, gizlilik açısından yeni bir şey vermiyoruz. Çerez ise oturum
kimliği taşıyor; uygulamanın dışına çıkarmayı varsayılan yapmak, kullanıcının
haberi olmadan kimlik bilgisini taşımak olurdu.

**Alternatifler:**
- Hiç başlık taşımamak: uzantı pratikte pek çok sitede işe yaramazdı.
- Her şeyi varsayılan açık göndermek: projenin telemetri/gizlilik duruşuyla
  çelişirdi.

---

## #15 — Oturumlar Arası Liste: Ayrı Veritabanı Değil, `.muiget` Dosyalarını Taramak

**Durum:** Kabul edildi

**Bağlam:** Uygulama kapanıp açılınca indirme listesi boş geliyordu. Yarım
dosyalar ve `.muiget` metaları diskte duruyordu ama kimse onlara bakmıyordu:
kullanıcının gözünde yarım indirme "kaybolmuş" oluyordu.

**Karar:** Açılışta indirme klasörü taranıyor (`resume::scan_directory`),
bulunan `.muiget` dosyaları listeye **duraklatılmış** kayıt olarak yükleniyor.
Ayrı bir "indirme listesi" veritabanı ya da `downloads.json` **yok**.

**Gerekçe:** İki ayrı gerçeklik kaynağı tutmak, ikisinin ayrışması demek.
Kullanıcı yarım dosyayı elle silerse listede hayalet kayıt kalırdı; dosyayı
başka klasöre taşırsa liste yanlış yolu gösterirdi. Meta dosyası zaten
indirilen dosyanın **yanında** duruyor: dosya nereye giderse durum da onunla
gidiyor, dosya silinirse durum da siliniyor. Tarama, bu tek kaynağı okumaktan
ibaret.

**Sonuçları / bilinçli sınırlar:**
- Yalnızca ayarlardaki indirme klasörü, yalnızca bir seviye taranıyor. Başka
  klasöre inen indirmeler açılışta gelmiyor; ayarlardaki **Klasörü tara**
  düğmesi bunun için var. Her hedef klasörü kalıcı olarak izlemek, kaçındığımız
  defteri geri getirirdi.
- `.mgpart` dosyası kaybolmuş meta öksüz sayılıp siliniyor. Listede "yarısı
  inmiş" gösterip devam edince sıfırdan başlamak, kullanıcıyı yanıltırdı.
- Geri yüklenen indirmeler **kendiliğinden başlamıyor**. Uygulamayı açar açmaz
  bağlantının dolması, kullanıcının istemeden başına gelen bir şey olurdu.
  `resumeOnStart` ayarı isteyene bunu veriyor, varsayılan kapalı.
- `DownloadOptions` (uzantıdan gelen `Referer` vb.) metaya yazılıyor. Yazılmasa
  geri yüklenen indirme başlıksız gidip 403 alırdı — karar #14'ün kalıcı hâli.

**Alternatifler:**
- `downloads.json` gibi merkezî bir liste: yukarıdaki ayrışma sorunu.
- SQLite: aynı sorun + yeni bağımlılık, kazancı yok.

---

## #16 — Kuyruk: Eşzamanlı İndirme Sınırı Yöneticide, Tek Bir Pompada

**Durum:** Kabul edildi

**Bağlam:** Kaç indirme başlatılırsa hepsi aynı anda çalışıyordu. On dosyayı
birden indirmek toplam süreyi kısaltmıyor — bant genişliği bölünüyor ve
**hiçbiri** erken bitmiyor. Kullanıcı genelde ilk dosyayı bekliyor.

**Karar:** `ManagerConfig.max_concurrent_downloads` (varsayılan 3, `0` =
sınırsız). `start` ve `resume` süpervizörü doğrudan başlatmıyor; isteği kaydın
`pending` alanına yazıp `pump()`'a bırakıyorlar. Süpervizör biten her indirmede
`pump()`'ı yeniden çağırıyor.

**Gerekçe:** Sınırın tek bir yerde uygulanması şart. `start` ve `resume` kendi
başlarına karar verseydi ikisi aynı anda çalıştığında sınırı birlikte aşarlardı.
`pump` bir mutex arkasında: aynı boş slotu iki çağrı birden göremiyor.

Bir slotun "dolu" sayılması için kaydın aktif olması **ve** artık beklemiyor
olması gerekiyor. `pending` alındıktan sonra süpervizör durumu `Probing` yapana
kadar kayıt hâlâ `Queued` görünüyor; o aralıkta da slotu tutması gerek, yoksa
`pump` aynı slotu ikinci kez dağıtırdı.

**Yan etkisi — ortaya çıkan bir hata:** Kuyruk gelince "süpervizörü hiç
çalışmamış indirme" sıradan bir durum oldu. Böyle bir kaydın `target` alanı
hâlâ hedef **klasörü** gösteriyor (dosya adı sunucu yoklanınca belli oluyor),
ve devam etmek onu dosya yolu sanıp klasörün üstüne yazmaya çalışıyordu.
`EntryState.resolved` bayrağı bunu ayırıyor. Uçtan uca test yakaladı.

**Alternatifler:**
- Semafor: `pause` edilen bir indirmenin izni geri vermesi ve sıradakini
  seçmek için yine bir sıra tutmak gerekirdi; `pump` daha az parça.
- Sınırı arayüzde uygulamak: köprüden (uzantıdan) gelen indirmeler sınırı
  hiç görmezdi.

---

## #17 — Host Kotası İndirmeler Arasında Bölüşülüyor, İzin Sırasında Değil

**Durum:** Kabul edildi

**Bağlam:** Karar #2'nin host kotası (`max_connections_per_host`, varsayılan 8)
sunucuyu koruyordu ama indirmeler arasında adalet sağlamıyordu. Aynı siteden üç
dosya başlatılınca gerçekte şu oluyordu: ilk indirme sekiz iznin hepsini alıyor,
diğer ikisi **sıfır byte'ta** bekliyor ve ancak ilki bitmeye yaklaşınca
başlıyordu. Arayüz dürüst davranıp "Bağlantı bekleniyor" yazıyordu ama kullanıcı
açısından ikinci ve üçüncü indirme takılmış görünüyordu.

Sebep, iznin **segment ömrü boyunca** tutulması: bir worker izni aldığında
segmenti bitene kadar bırakmıyor. Segmentler kabaca eşit büyüklükte olduğu için
hepsi indirmenin sonunda birlikte serbest kalıyor.

**Karar:** Adalet izin dağıtımında değil **segment planında** uygulanıyor.
`HostLimiter` artık host başına kaç *indirme* olduğunu da biliyor
(`register()` → RAII kayıt) ve `fair_share()` her indirmeye düşen payı veriyor:

```text
pay = max(1, max_per_host / o hosttaki indirme sayısı)
```

Süpervizör başlarken kaydını yapıp `config.segments`i payla sınırlıyor. Üç
indirme × 2 segment = 6 bağlantı: kota aşılmıyor ve kimse aç kalmıyor.

**Rebalans bedava geliyor:** bir indirme bitince kaydı düşüyor, kalanların payı
büyüyor ve adaptif bölme (`try_steal`, karar #5) zaten boşalan slotu
değerlendiriyor. Yani ayrı bir yeniden dağıtım mekanizması yazılmadı; var olan
"work stealing" makinesi pay kontrolüyle rebalanser'a dönüştü.

**Gerekçe:**
- İzni chunk başına almak (adaletli kuyruk) her chunk'ta bağlantıyı bırakmak
  demek olurdu — TCP+TLS el sıkışmasını sürekli tekrarlamak.
- Payın sıfır olmasına izin verilmiyor (`max(1, …)`): sıfır pay, indirmenin hiç
  başlayamaması demekti.
- Sürmekte olan indirmenin segmentleri yeni bir indirme geldiğinde **kesilmiyor**.
  Kesmek anlık adalet verirdi ama yarım TCP bağlantılarını çöpe atardı; birkaç
  saniyelik gecikme bundan ucuz.

**Alternatifler:**
- Global bir bağlantı zamanlayıcısı: doğru çözüm ama motorun her katmanına
  dokunurdu; kazanç aynı, karmaşıklık kat kat fazla.
- Kotayı büyütmek: sunucuyu korumak için konulan sınırı adalet için gevşetmek
  olurdu — yanlış düğme.

---

## #18 — Kategori Klasörleri: Gömülü Eşleme, Varsayılan Kapalı

**Durum:** Kabul edildi

**Bağlam:** IDM'in en görünür davranışlarından biri inen dosyayı türüne göre
`Video`, `Müzik`, `Belgeler`… alt klasörlerine ayırması. İki soru vardı:
kurallar kullanıcı tarafından düzenlenebilsin mi, ve varsayılan açık mı?

**Karar:** Eşleme koda gömülü (`download/category.rs`), özellik ayarlardan tek
anahtarla açılıp kapanıyor ve **varsayılan kapalı**.

**Gerekçe:**
- Kural düzenleyicisi bu aşamada çözdüğünden çok soru doğururdu: çakışan
  kurallar, sıra, büyük/küçük harf, kullanıcının bozduğu eşlemeyi kurtarma.
  Gömülü liste bir dosyada duruyor ve katkıya açık.
- Varsayılan kapalı, çünkü bir sürüm yükseltmesinin indirmelerin **nereye
  düştüğünü** sessizce değiştirmesi, kullanıcıya dosyasını kaybettirmek gibi
  gelirdi. Açmak tek tıklama; geri almak dosya aramak demek.
- Tanınmayan uzantı için "Diğer" klasörü **yok**: dosyayı bir kat daha derine
  gömmek, aramayı kolaylaştırmıyor.
- Sürmekte olan indirmeler taşınmıyor. Hedef yol metada yazılı (karar #3) ve
  dosyayı altından çekmek resume'u bozardı.

**Karar #15 ile etkileşimi:** oturumlar arası liste `.muiget` taramasıyla geri
geliyordu ve tarama tek seviyeydi. Kategori açıkken yarım indirmeler alt
klasörde olacağı için tarama, **yalnızca bilinen kategori klasörlerine** de
bakıyor. Serbest özyineleme hâlâ yok: o klasörlere dosyayı bu uygulama koyuyor,
gerisi kullanıcının alanı.

---

## #19 — Vekil Sunucu: Tek Alan, Şemasız Adrese `http://` Ekle, Geçersizi Boşalt

**Durum:** Kabul edildi

**Bağlam:** Kurumsal ağların çoğu doğrudan çıkışa izin vermiyor. Proxy desteği
olmayan bir indirme yöneticisi o ağlarda hiç çalışmıyor; IDM'de yıllardır var.

**Karar:** `ManagerConfig.proxy` tek bir dizge. Boş = doğrudan bağlantı.
`reqwest`'e `socks` özelliği eklendi, böylece `socks5://` de kabul ediliyor.
Ayar `settings::normalize_proxy`'den geçiyor: şema yoksa `http://` ekleniyor,
desteklenmeyen şema **boşaltılıyor**.

**Gerekçe:**
- HTTP ve SOCKS için ayrı alanlar açmak, kullanıcıya bizim iç ayrımımızı
  ezberletmek olurdu. Adres zaten şemayı taşıyor.
- Şemasız yazım (`10.0.0.1:8080`) insanların vekil adresini not etme biçimi;
  bunu hata saymak gereksiz bir tökezleme noktası.
- Geçersiz şema **boşaltılıyor, hata verilmiyor**: bozuk bir vekille istemci hiç
  kurulamıyor ve o hâlde uygulama tek bir dosya bile indiremezdi. Sessizce
  doğrudan bağlanmak, çalışmayan bir yapılandırmadan iyi — durum log'a yazılıyor.

**Sınır:** `reqwest::Proxy::all` şemayı istemci kurulurken doğrulamıyor
(`ftp://` sessizce kabul edilip ilk istekte patlıyor), bu yüzden asıl süzgeç
ayar katmanında. `download/http.rs`'teki test bu sınırı kayda geçiriyor.

**Ayar değişiminde geçiş:** yeni vekil yalnızca yeni bağlantılara uygulanıyor;
akan indirmeler eski istemciyle devam ediyor. Yarım bir indirmeyi ayar değişti
diye kesmek, kullanıcının beklemediği bir kayıp olurdu.

---

## #20 — Adresteki Kimlik Bilgisi Motorun Kapısında Ayrılıyor

**Durum:** Kabul edildi

**Bağlam:** `https://kullanıcı:parola@site/dosya.zip` biçimi yaygın; korumalı
dizinler ve bazı NAS/paylaşım sunucuları bunu bekliyor. `reqwest` bu bilgiyi
kendiliğinden `Authorization` başlığına çevirmiyor, yani adres olduğu gibi
gönderilse 401 alınıyordu.

**Karar:** `http::split_credentials` adresi ikiye ayırıyor;
`DownloadManager::start_with` kimliği `Authorization: Basic …` başlığı olarak
`DownloadOptions.headers`'a ekliyor ve **temizlenmiş** adresi kaydediyor.

**Gerekçe:**
- Kimlik URL'de kalsaydı listede, log'da ve `.muiget` metasında **parola olarak
  düz metin** dururdu. Ayırma motorun en dış kapısında yapılıyor; içeride hiçbir
  katman parolalı adresi görmüyor.
- Mevcut başlık boru hattı yeniden kullanılıyor (karar #14): başlıklar zaten
  metaya yazılıyor ve her segment isteğine ekleniyor. Ayrı bir "kimlik" kavramı
  eklemek aynı işi ikinci kez yapmak olurdu.
- Uzantıdan gelen `Authorization` başlığı önceliklidir: tarayıcının oturumu,
  adrese elle yazılmış kimlikten daha güncel.
- Parolada `@` geçebildiği için son `@` sınır kabul ediliyor; kullanıcı adı ve
  parola yüzde kodlamasından çözülüyor.

**Sınır:** yalnızca HTTP Basic. Digest ve form tabanlı oturum açma yok; ikisi de
sunucuyla tur atmayı gerektiriyor ve şu an talep eden bir senaryo yok.

**Doğrulama:** uçtan uca test, kimlik isteyen (yoksa 401 dönen) bir sunucuya
karşı segmentli indirme yapıyor. Başlık tek bir segmentte unutulsaydı test
düşerdi. İkinci bir test kimliksiz isteğin gerçekten 401 aldığını gösteriyor —
yoksa ilk test sunucu her isteği kabul ettiği için de geçebilirdi.

---

## #21 — Checksum İstek Üzerine, Otomatik Değil

**Durum:** Kabul edildi

**Bağlam:** İndirilen dosyanın SHA-256/MD5 özeti, yayımlanmış değerle
karşılaştırmak için gerekiyor (`docs/project_overview.md` → Güven katmanı).

**Karar:** Özet **her indirmede otomatik hesaplanmıyor**; satır sağ tık
menüsünden ve `file_checksum` komutundan isteniyor. Yalnızca tamamlanmış
indirmelerde çalışıyor.

**Gerekçe:**
- 8 GB'lık bir dosyayı hash'lemek diski baştan sona bir kez daha okumak demek.
  Kullanıcıların çoğu bu değere hiç bakmıyor; herkese bu bedeli ödetmek yanlış.
  IDM de otomatik hesaplamıyor.
- Yarım dosyanın özeti anlamsız ve **zararlı**: kullanıcı onu sitedeki değerle
  karşılaştırıp "indirme bozuk" sanardı. Bu yüzden komut tamamlanmamış
  indirmede hata veriyor.
- MD5 çakışmaya açık, imza doğrulamada kullanılmamalı — yine de duruyor, çünkü
  indirme sitelerinin çoğu hâlâ yalnızca MD5 yayımlıyor.
- Hesaplama akış hâlinde ve her blok sonrası `yield_now()` ile: büyük dosyada
  tek bir görev çekirdeği doldurup arayüzü dondurmasın.

---

## #22 — Kopya İndirme: Uyarı, Engel Değil

**Durum:** Kabul edildi

**Bağlam:** Aynı adres iki kez eklendiğinde ne olmalı? IDM soruyor.

**Karar:** `find_duplicate` listede aynı adresi arıyor; yeni indirme diyaloğu
bulursa **uyarı** gösteriyor, düğmeyi kilitlemiyor. Pano izleme (karar #24)
zaten listede olan bir adresi hiç önermiyor.

**Gerekçe:**
- Engellemek yanlış olurdu: aynı dosyayı bilerek yeniden indirmek meşru bir
  istek (bozuk indi, dosya silindi, sürüm değişti).
- Karşılaştırma kimlik ayıklandıktan sonra yapılıyor (karar #20): aynı dosya bir
  kez parolalı bir kez parolasız yapıştırıldığında ikisi de aynı indirme.
- İptal edilenler sayılmıyor — kullanıcı onu bilerek durdurdu, yeniden denemek
  isteyebilir.

---

## #23 — Sürüm Kontrolü Var, İmzalı Otomatik Güncelleyici Yok

**Durum:** Kabul edildi

**Bağlam:** v0.1.2'yi kuran kullanıcı v0.1.3'ten nasıl haberdar olacak? Bugüne
kadarki cevap "GitHub'a baksın"dı; gerçek kullanıcı için bu, güncellenmeyen bir
uygulama demek.

**Karar:** Uygulama açılışta GitHub'ın yayın listesine bakıp yeni sürüm varsa
bildirim gösteriyor; indirmeyi kullanıcının tarayıcısına bırakıyor. Tauri'nin
`updater` eklentisi **kurulmadı**.

**Gerekçe:**
- Updater bir imza anahtar çifti ve her yayında imzalanan bir `latest.json`
  istiyor. Anahtar İlker'in elinde olmadan yarım kurulan bir updater, uygulamayı
  hiç güncellenemez hâle getirirdi.
- Bu, uygulamanın kendiliğinden yaptığı **tek** dış istek. Kullanıcı verisi
  taşımıyor ve ayardan kapatılabiliyor; `project_overview.md`'deki "telemetri
  yok" sözüyle çelişmemesi için bu sınır ayar metnine de yazıldı.

**Sahada bulunan hata:** ilk uygulama `/releases/latest` uç noktasını
kullanıyordu. O uç nokta ön sürümleri atlıyor ve Muiget'in **bütün** yayınları
`prerelease: true` — yani 404 dönüyordu, özellik her seferinde sessizce
başarısız olurdu. Gerçek API'ye bakılmasaydı görülmezdi; 8. oturumun
`--native-host` hatasıyla aynı sınıf. Artık `/releases?per_page=10` çekiliyor,
taslaklar eleniyor ve en yüksek sürüm **liste sırasına güvenmeden** seçiliyor
(GitHub listeyi tarihe göre sıralıyor, sürüme göre değil).

---

## #24 — Pano İzleme: Rust Tarafında, Dar Süzgeçle, Varsayılan Kapalı

**Durum:** Kabul edildi

**Bağlam:** IDM'in en çok kullanılan davranışlarından biri: bağlantıyı
kopyalayınca "indireyim mi?" diye sorması. Uzantı kurmadan çalışan tek yol bu.

**Karar:** Pano **Rust tarafında** saniyede bir okunuyor
(`tauri-plugin-clipboard-manager`), `clipboard::indirilebilir_baglanti`
süzgecinden geçiyor ve eşleşme arayüze olay olarak gidiyor. Varsayılan kapalı.

**Gerekçe:**
- **Neden arayüzde değil:** webview'in `navigator.clipboard` okuması pencerenin
  odakta olmasını gerektiriyor. Oysa bu özelliğin bütün anlamı, kullanıcı
  *tarayıcıdayken* kopyaladığı adresi yakalamak. Rust tarafı pencere tepside
  bile çalışıyor.
- **Neden varsayılan kapalı:** panoyu sürekli okumak, kullanıcının kopyaladığı
  her şeyi (parola yöneticisinden gelen bir parolayı da) uygulamanın görmesi
  demek. Böyle bir yeteneğin sessizce açık gelmesi bu projede yanlış olurdu.
  Ayar metni ne yaptığını açıkça yazıyor.
- **Neden dar süzgeç:** yalnızca tek satırlık, `http(s)` şemalı ve yolunun
  sonunda **tanınan bir dosya uzantısı** (karar #18'in tablosu) olan adresler.
  Her URL'yi yakalamak, bir haber sayfasının adresi kopyalandığında da sormak
  demekti; ikinci kez rahatsız eden bir özellik kapatılır.
- **Neden kendiliğinden indirmiyor:** öneri bildirim olarak çıkıyor, düğmeye
  basılırsa yeni indirme kutusu dolu açılıyor. Kopyalanan her adresi sessizce
  indirmeye başlamak, kullanıcının istemediği dosyaları diske yazmak olurdu.
- Açılışta panoda duran içerik yok sayılıyor: uygulama açıldığında panodaki şey
  kullanıcının o an kopyaladığı bir adres değil.
- Arayüze pano izni **verilmedi** (`capabilities/default.json` değişmedi):
  okuma yalnızca Rust tarafında, tek bir yerde.

**Sahada doğrulandı:** uygulama çalışırken pano dışarıdan üç kez değiştirildi.
`…/test-dosyasi.zip` yakalandı; bir GitHub sayfa adresi ve düz metin bir parola
**yakalanmadı** — süzgecin gerçekten dar olduğu, "parolam log'a düşer mi"
sorusunun cevabıyla birlikte görüldü.

---

## #25 — HLS/DASH: Parçalar Bizim Kodumuzda, ffmpeg İsteğe Bağlı

**Durum:** Kabul edildi

**Bağlam:** IDM'i bugün satan özellik video yakalama. Sıradan bir dosyayı
indirmek artık her tarayıcıda var; IDM'i kuran insanların çoğu bir sayfadaki
videoyu almak için kuruyor. Muiget'in motoru, arayüzü ve uzantısı hazırdı ama
akış videosu (HLS `.m3u8`, DASH `.mpd`) hiç desteklenmiyordu.

Akış, motorun mevcut mantığına oturmuyor: orada "tek dosyanın N byte aralığı"
var (karar #3), burada "N ayrı dosyanın tamamı". `docs/tasks.md` bu yüzden ilk
kararı açıkça soruyordu: **ffmpeg dış bağımlılık olarak mı gelecek, yoksa
birleştirme kendi kodumuzda mı yapılacak?**

**Karar:** İkisi de. Manifest ayrıştırma, parça indirme, `AES-128` çözme ve
parçaların uç uca eklenmesi tamamen bu projenin kodu (`src/media/`). ffmpeg
**isteğe bağlı** ve yalnızca iki iş için çağrılıyor:

1. MPEG-TS parçalarını `.mp4` kabına taşımak (`-c copy`, yeniden kodlama yok),
2. **ayrı inen ses ile videoyu birleştirmek**.

**Gerekçe:**

- **Neden ffmpeg gömülmedi:** ffmpeg ikilisi ~70 MB ve LGPL/GPL matrisi
  Apache-2.0 bir projenin dağıtımını karmaşıklaştırıyor. Kurulum paketini on
  katına çıkarmak, kullanıcıların çoğunun hiç ihtiyaç duymadığı bir yetenek
  için ağır bir bedel.
- **Neden zorunlu da değil:** parçaları uç uca eklemek zaten oynatılabilir bir
  dosya veriyor — MPEG-TS parçaları geçerli bir `.ts`, fMP4 parçaları (init
  parçasıyla birlikte) geçerli bir `.mp4`. Yani ffmpeg'i şart koşmak,
  kurulumu olmayan kullanıcıya çalışabilecek bir indirmeyi reddetmek olurdu.
- **Neden ayrı ses ffmpeg'siz reddediliyor:** ses ve video ayrı iniyorsa
  (DASH'in tamamı, HLS'in `#EXT-X-MEDIA` kullanan yayınları) birleştirme
  kaçınılmaz. ffmpeg yoksa indirme **hiç başlamıyor** ve sebebi yazılıyor.
  Alternatif — sessiz bir video dosyası teslim etmek — kullanıcının ancak
  izlemeye başlayınca fark edeceği bir hata olurdu. Kontrol tek byte inmeden
  yapılıyor; yüzlerce parçayı indirip sonunda birleştirememek bant genişliğini
  çöpe atmak demekti. Kaçış yolu var: diyalogdan "yalnızca görüntü".
- **Neden `.ts` de `.mp4`e çevriliyor:** ffmpeg varken. `.ts` dosyaları
  Windows'ta çift tıklanınca çoğu zaman açılmıyor ve telefona atılamıyor.
  Dönüşüm yeniden kodlama değil, saniyeler süren bir kap değişimi.

**AES-128 destekleniyor, DRM desteklenmiyor.** HLS'in `METHOD=AES-128` kipinde
anahtar, manifestin gösterdiği adresten **herkese açık** veriliyor; tarayıcıdaki
oynatıcı da tam olarak bunu yapıyor. Burada atlatılan bir koruma yok ve
desteklenmemesi hâlinde sıradan bir video sitesinin yarısı inmezdi. Buna
karşılık `SAMPLE-AES` (FairPlay), Widevine ve PlayReady **ayrıştırma anında**
reddediliyor — orada anahtar bir lisans sunucusundan cihaz kimliğiyle alınıyor
ve onu aşmak `CLAUDE.md`'deki kapsam sınırının dışına çıkmak olurdu. Ret
mesajı sebebi açıkça söylüyor; sessizce başarısız olmuyor.

**Canlı yayın reddediliyor.** `#EXT-X-ENDLIST` yoksa (ya da MPD
`type="dynamic"` ise) akışın sonu belli değil, dolayısıyla indirmenin de sonu
olmaz. Kayıt bambaşka bir özellik; "indirme" gibi görünen ama hiç bitmeyen bir
satır kullanıcıya bozuk bir uygulama izlenimi verirdi.

**Paralel indir, sıralı yaz.** Parçalar `futures_util`in `buffered(N)`i ile
paralel iniyor ama sonuçlar **manifest sırasında** teslim ediliyor; çıktı
dosyası sıralı büyüyor. Ayrı bir sıralama tamponu ya da geçici dosya
gerekmiyor, bellek N parçayla sınırlı. `download::worker`in sparse yazma
yaklaşımı burada kullanılamazdı: bir parçanın dosyadaki yeri ancak kendinden
öncekilerin boyu bilinince belli oluyor.

**Devam etme tek sayıya indi.** Yazma sıralı olduğu için devam noktası "kaç
parça tamamlandı" + "dosya kaç byte". Devam ederken dosya o boya **kırpılıyor**:
uygulama meta yazmadan çöktüyse fazladan kalmış son parça böyle atılıyor.
Kırpmasaydık aynı parça iki kez yazılır ve video sessizce bozulurdu.

**Bağımlılık eklendi:** `aes` + `cbc` (RustCrypto, saf Rust, sistem bağımlılığı
yok, proje zaten aynı ailenin `sha2`/`md-5`ini kullanıyor). AES'i elle yazmak
gözden geçirilmemiş bir blok şifre bırakırdı. XML okuyucu ise **eklenmedi**:
DASH'in ihtiyacı olan yüzey dar ve tek kullanıcısı için `quick-xml` ağacını
getirmek bu projenin ölçütüne uymuyordu (`media/xml.rs`, ~250 satır, testli).

**Ayarlarda bir yol yazılıysa yalnızca o deneniyor.** ffmpeg araması boşken
uygulamanın yanına, sonra `PATH`e bakıyor; kullanıcı bir yol yazdıysa
PATH'tekine sessizce düşmüyor. Yazdığı ffmpeg çalışmıyorsa bunu görmesi
gerekiyor — başka bir sürümün arkasında saklanması değil. (Yan fayda: testler
"ffmpeg yok" durumunu makineden bağımsız kurabiliyor.)

**Ne test edildi:** 16 uçtan uca test yerel bir HTTP sunucusuna karşı koşuyor —
sıralı birleştirme, master playlistten kalite seçimi, `AES-128` çözme (fixture
gerçekten şifreleniyor), fMP4 init parçası, duraklat/devam ederken parçaların
**yeniden indirilmemesi**, canlı ve DRM reddi, eksik parçada başarısızlık.
ffmpeg entegrasyonu sahte bir ffmpeg betiğiyle sınanıyor: gerçek ffmpeg her
makinede yok ve olsa bile testi onun sürümüne bağlamak sonucu değişken yapardı;
sınanan şey ffmpeg değil, etrafındaki bulma/çağırma/taşıma/temizleme zinciri.

**Gerçek pencerede doğrulandı:** yerelde beş parçalık bir VOD playlisti
kaldırılıp uygulamaya köprünün kullandığı `--add` argümanıyla verildi. Sunucu
günlüğü parçaların **paralel** istendiğini gösteriyor (`s4`, `s3`ten önce
geldi); çıktı dosyasının SHA-256'sı beklenen birleşimle birebir aynı. Yani
"paralel indir, sırayla yaz" sözü test ortamında değil uygulamanın kendisinde
tutuyor. ffmpeg bu makinede kurulu olmadığı için dosya `.ts` kaldı — beklenen.

---

## #26 — Uzantıda Video Yakalama: `webRequest`, İsteğe Bağlı ve Kapalı

**Durum:** Kabul edildi

**Bağlam:** Uzantının sayfa taraması DOM'a bakıyor (karar: kalıcı content
script yok). HLS/DASH manifestinin adresi ise sayfanın HTML'inde **hiç
geçmiyor** — oynatıcı JavaScript'i çalışırken isteniyor. Yani DOM taraması bu
videoları tanım gereği bulamaz.

**Karar:** `webRequest` **isteğe bağlı** izin olarak eklendi (varsayılan
kapalı). Açıldığında arka plan `.m3u8`/`.mpd` isteklerini görüp sekme başına
`storage.session`de biriktiriyor; popup o listeyi gösteriyor.

**Gerekçe:**
- **Neden isteğe bağlı:** izin verildiğinde uzantı gezilen her sayfanın ağ
  isteklerinin adreslerini görüyor. Bu ağır bir yetki ve kurulumda sessizce
  istenmemeli — aynı ölçüt `downloads` ve `cookies` izinlerinde de uygulandı.
  Chrome'un izin kutusu zaten kullanıcıya ne verdiğini söylüyor; anahtar da
  aynı cümleyi yazıyor.
- **Neden `storage.session`:** liste tarayıcı kapanınca siliniyor ve diske
  yazılmıyor. Sekme kapanınca ya da sayfa değişince o sekmenin kaydı düşüyor;
  önceki sayfanın videosunu göstermek kullanıcıyı yanıltırdı.
- **Süzgeçten geçmeyen hiçbir adres kaydedilmiyor.** Dinleyici her isteği
  görüyor ama yalnızca manifest desenine uyanlar saklanıyor.
- **Rozet sekmeye özel** (`setBadgeText({tabId})`): indirme gönderiminde
  kullanılan geçici genel rozetlerle çakışmıyor.
- Uzantı dosya adı **göndermiyor**: bir manifestin adı (`master.m3u8`) diskte
  `master.mp4` olurdu. Masaüstü tarafı adı manifest adresinden türetiyor ve
  `master`/`index` gibi anlamsız gövdeleri atlıyor.

---

## 27. YouTube (manifestsiz) yakalama — derleme bayrağının arkasında

**Bağlam:** Kullanıcı YouTube'da uzantının hiçbir şey bulamadığını bildirdi.
Sebep hata değildi: [karar #26](#26)'nın süzgeci yalnızca `.m3u8`/`.mpd`
arıyor, YouTube ise normal videolarda manifest kullanmıyor. Oynatıcı, sayfaya
gömülü `streamingData` listesinden aldığı `googlevideo.com/videoplayback`
adreslerini byte aralıklarıyla çekiyor; ağdan geçen bir manifest yok.

**Karar:** Manifest süzgecinin yanına **doğrudan medya** kuralları eklendi
(ilki YouTube). Yakalama, `background.js`'teki tek bir sabite bağlı:
`DOGRUDAN_MEDYA_YAKALAMA`. GitHub'dan inen pakette `true`, Chrome Web Store'a
gidecek derlemede `false`.

**Gerekçe:**

- **Neden bayrak:** Chrome Web Store geliştirici politikası, telifli içeriğin
  indirilmesini "kolaylaştıran" uzantıları yasaklıyor ve YouTube'u adıyla
  sayıyor — *uzantının nasıl kurgulandığına bakmaksızın*. "Uzantı yalnızca
  adresi bulur, indirmeyi masaüstü yapar" ayrımı (ki Muiget'in mimarisi tam bu)
  kurtarmıyor. IDM'in mağazadaki uzantısında YouTube bu yüzden kapalı.
  `docs/tasks.md`'deki "Chrome Web Store yayını" hedefiyle YouTube desteği
  doğrudan çakışıyordu; bayrak ikisini de açık tutuyor. Kaynak açık olduğu
  için gizlemenin anlamı yok, kapatmak da tek satır.

- **Sınırın neresinde durduğu:** burada imza çözülmüyor, şifre kırılmıyor,
  DRM'e dokunulmuyor. `videoplayback` adresindeki `n`/`sig` alanlarını
  tarayıcının kendi oynatıcısı üretiyor ve adresi zaten istiyor; uzantı yalnızca
  o adresi görüyor. Alternatif yol — YouTube'un `base.js`'inden deşifre
  fonksiyonunu çıkarıp çalıştırmak — **reddedildi**: o gerçekten bir teknik
  koruma aşma olurdu, üstelik YouTube onu düzenli değiştirdiği için sürekli
  kırılan bir bakım yükü getirirdi.
  README'deki "hiçbir kullanım şartını ihlal etmez" cümlesi bu kararla birlikte
  düzeltildi; artık ToS sorumluluğunun kullanıcıda olduğunu açıkça yazıyor.

- **Adres normalize ediliyor:** `range`, `rn`, `rbuf` gibi parçaya özel alanlar
  siliniyor. Silinmezse indirilen adres tek bir parçayı verir, tam dosyayı
  değil.

- **Tekilleştirme adrese değil `itag`e göre:** aynı akışın her parça isteği
  farklı bir adres üretiyor (`rn` sayacı artıyor). Adrese bakılsaydı 12'lik
  liste tek videonun parçalarıyla dolar, kullanıcı ses izini hiç göremezdi.

- **Sessiz akış işaretleniyor:** uyarlanır yayında video ve ses ayrı iniyor;
  video itag'ı tek başına indirilirse sessiz dosya çıkıyor. [Karar #25](#25)
  bunu açıkça yasakladığı için popup, düğmeye basılmadan önce "sessiz — ses
  ayrı iniyor" uyarısını gösteriyor. Eski birleşik itag'lar (18, 22…) hem video
  hem ses taşıdığı için işaretlenmiyor.

**Açık kalan:** video+ses çiftini otomatik eşleyip ffmpeg ile birleştirmek
henüz yok. Bugün kullanıcı iki akışı ayrı indirip elle birleştiriyor; ses tek
başına indirildiğinde ([karar #28](#28)) zaten doğrudan kullanılabilir dosya
çıkıyor.

---

## 28. Ses indirmede varsayılan biçim: kayıpsız çıkarma, MP3 seçenek

**Bağlam:** "MP3 desteği de ekleyelim" talebi. Kullanıcıların çoğu video
sitelerinden ses almak için harici sitelere gidiyor.

**Karar:** `mux.rs`'e iki yeni kip eklendi. Varsayılan `AudioCopy`: ses izi
`-vn -c:a copy` ile **olduğu gibi** çıkarılıyor (`.m4a`/`.opus`, kaynağın
kodekine göre). `AudioMp3 { kbps }` ise `libmp3lame` ile yeniden kodluyor ve
yalnızca kullanıcı isterse çalışıyor.

**Gerekçe:**
- Akış videosunun sesi zaten AAC ya da Opus — yani çoktan kayıplı
  sıkıştırılmış. MP3'e çevirmek **ikinci bir kayıplı geçiş** demek: kalite
  düşüyor ve işlem dakikalar sürüyor. Kabı değiştirip içeriğe dokunmamak hem
  anında bitiyor hem kayıpsız.
- Bu, `build_args`'ın en baştaki `-c copy` ilkesiyle de tutarlı: "bir indirme
  yöneticisi kullanıcının medyasını yeniden kodlamaz." MP3 bu ilkeden bilinçli
  bir sapma ve o yüzden varsayılan değil, seçenek.
- MP3 yine de duruyor çünkü hâlâ her cihazda çalan tek biçim.
- **Çıktı uzantısı `codecs` alanından türetiliyor** (`audio_extension`). Kabı
  yanlış seçmek `-c copy`yi çalışmaz hale getiriyor: AAC'yi `.opus` diye
  yazmaya kalkınca ffmpeg akışı ogg'a koyamayıp hata veriyor.
- `+faststart` yalnızca MP4 ailesinde ekleniyor; `.mp3`/`.opus` çıktısında
  ffmpeg onu geçersiz seçenek sayıp duruyor.

---

## 29. Altyazı: varsayılan açık, metin düzeyinde birleştirme, videoyu asla düşürmez

**Bağlam:** Faz 6'nın açık kalan maddesi. HLS master playlistindeki
`#EXT-X-MEDIA:TYPE=SUBTITLES` ve DASH'in `contentType="text"` AdaptationSet'i
şimdiye kadar ayrıştırma anında atlanıyordu. Yabancı dildeki bir yayını
altyazısız indirmek işi yarım bırakıyor ve IDM bunu yapıyor.

**Karar:**

1. Altyazılar ayrı bir liste (`MediaManifest.subtitles`), video/ses seçimine
   hiç karışmıyor.
2. Varsayılan **açık** (`media_subtitles: "auto"`): dil tercihine uyan, o yoksa
   yayının kendi varsayılanı tek altyazı iniyor. `all` hepsini, `off` hiçbirini
   indiriyor.
3. Parçalar **metin düzeyinde** birleştiriliyor (`media/vtt.rs`), uç uca
   eklenmiyor.
4. Çıktı videonun yanına, aynı gövdeyle: `film.mp4` → `film.tr.vtt`.
5. Altyazı **hiçbir koşulda** indirmeyi düşürmüyor.
6. fMP4'e sarılmış altyazılar (`codecs="wvtt"`/`"stpp"`) baştan eleniyor.

**Gerekçe:**

- **Neden varsayılan açık:** kapalı bir özellik görünmeyen bir özellik. Bedel
  gerçekten küçük — bir saatlik filmin altyazısı birkaç yüz KB ve videonun
  yanında, adı açık, ayrı bir dosya. Buna karşılık yabancı dildeki bir videoyu
  altyazısız indirmiş olmak, kullanıcının ancak izlemeye başlayınca fark
  edeceği bir eksiklik. Kapatmak Ayarlar'da tek tık.

- **Neden uç uca eklemek yetmiyor:** her HLS altyazı parçası kendi başına tam
  bir WebVTT belgesi, yani her birinin başında `WEBVTT` satırı var. Art arda
  yazılan bir dosyada oynatıcı ikinci başlığı bir cue sanıp ya orada kesiyor ya
  da dosyayı tamamen reddediyor. Video parçalarında işe yarayan yöntem
  (karar #25) burada işe yaramıyor.

- **Zaman ekseni ilk parçanın haritasına göre hizalanıyor.** HLS altyazısı
  `X-TIMESTAMP-MAP=LOCAL:...,MPEGTS:...` ile MPEG-TS saatine bağlanıyor ve o
  saat tipik olarak 900000'de (10 sn) başlıyor. Mutlak değeri korumak, ffmpeg
  `.mp4`e çevirdiğinde (orada zaman ekseni sıfırlanıyor) altyazıyı 10 saniye
  kaydırırdı. İlk parçanın offsetini taban almak iki çıktıda da doğru sonuç
  veriyor.

- **Parça sınırını aşan cue'lar tekilleştiriliyor:** sağlayıcılar böyle bir
  cue'yu her iki parçaya da yazıyor (oynatıcı hangisinden başlarsa başlasın
  görsün diye). Birleşik dosyada bu, aynı satırın iki kez görünmesi demek.

- **Biçim inen byte'lardan anlaşılıyor**, adresten ya da `mimeType`ten değil:
  sağlayıcılar `.vtt` uzantısının arkasına TTML, `application/mp4` etiketinin
  arkasına düz WebVTT koyabiliyor. Yanlış biçimde yazılan bir altyazı dosyası
  hiçbir oynatıcıda açılmıyor.

- **TTML yalnızca tek parçalıysa yazılıyor.** Her TTML parçası kendi `<tt>` kök
  öğesini taşıyor; ikisini uç uca eklemek geçersiz XML veriyor. Yarım bir
  çözüm, kullanıcıya açılmayan bir dosya vermek olurdu.

- **fMP4'e sarılmış altyazı neden listede yok:** `wvtt`/`stpp` mp4 kutularının
  açılmasını gerektiriyor; bu ayrı bir iş. Listede görünüp inmemeleri
  kullanıcıya yalan söylemek olurdu, o yüzden `describe` çıktısından da
  eleniyorlar.

- **Neden videoyu düşürmüyor:** kullanıcı bir filmi indirdi; altyazı
  sunucusunun 503 vermesi o filmi çöpe atmak için sebep değil. Ne olduğu yine
  de bir uyarıyla söyleniyor — sessizce eksik bir dosya bırakmak da kabul
  edilemezdi. Uyarı, varsa mevcut uyarının (tipik olarak "ffmpeg yok")
  **yanına** ekleniyor: üstüne yazsaydı, ffmpeg'i olmayan herkeste altyazı
  hatası sessizce yutulurdu.

- **Adım nereye kondu:** birleştirmeden *sonra* (ffmpeg düşerse klasörde
  sahipsiz `.vtt` kalmasın), `finalize`dan *önce* (orada durum `Completed`
  oluyor; sonrasına bırakılsaydı "tamamlandı" diyen bir indirmenin altyazısı
  hâlâ iniyor olurdu ve klasörü o anda açan kullanıcı dosyayı bulamazdı).

- **Devam noktası tutulmuyor.** Birkaç yüz KB için `.muiget` metasına ayrı bir
  alan taşımak, onu bozacak bir hata riskine değmiyor; yarım kalan altyazı
  sürdürmede baştan iniyor.

**Kapsam dışı:** DASH'in fMP4 altyazıları, `#EXT-X-DISCONTINUITY` sonrası
zaman ekseni sıfırlanan yayınlar, ve altyazının videoya *gömülmesi*
(soft-mux). Sonuncusu ffmpeg'le mümkün ama ayrı bir `.vtt` dosyası her
oynatıcıda çalışıyor ve kullanıcı isterse silebiliyor.

---

## 30. İlerleme olayları biriktiriliyor, chunk başına gönderilmiyor

**Bağlam:** `download::worker` her yazılan chunk için kanala bir
`WorkerEvent::Progress` gönderiyordu. reqwest'in verdiği chunk tipik olarak
8–64 KB; 100 MB/s'lik bir bağlantıda bu segment başına saniyede binlerce mesaj
demek, sekiz paralel segmentle on binlerce. Aynı desen akış boru hattında da
vardı (`FetchEvent::Bytes`).

**Karar:** Olaylar 256 KB **ya da** 100 ms'de bir gönderiliyor (hangisi önce
dolarsa). `worker.rs`'te bunu bir `IlerlemeBiriktirici` yapıyor; `Drop`
uygulaması sayesinde fonksiyonun hangi `return`ünden çıkılırsa çıkılsın kalan
byte kaybolmuyor.

**Gerekçe:**

- **Neden güvenli:** bu olayların **tek** tüketicisi hız ölçer
  (`SpeedMeter::record`). İlerleme çubuğunun gerçek kaynağı
  `SegmentContext.downloaded` atomiği; o her chunk'ta güncellenmeye devam
  ediyor. Yani biriktirme yalnızca göstergeyi etkileyebilirdi — ve EWMA'nın
  yarı-ömrü 3 saniye olduğu için 100 ms'lik bir pencere ölçümü gözle görülür
  biçimde değiştirmiyor.
- **Neden iki eşik:** yalnızca bayt eşiğine bakan bir kod, 20 KB/s'lik bir
  bağlantıda on saniyede bir güncelleme yapar ve hız göstergesi donmuş görünür.
  Yalnızca süre eşiğine bakmak ise hızlı bağlantıda mesaj sayısını
  sabitlemezdi.

**Ölçülmedi.** Bu bir mesaj sayısı azaltması; gerçek indirme hızına etkisi
sahada ölçülmedi ve "şu kadar hızlandı" diye bir iddia yok. Gerekçe aritmetik:
saniyede on binlerce kanal mesajı ve o kadar görev uyandırması, karşılığında
hiçbir bilgi kazandırmıyordu.

---

## 31. Firefox desteği: tek kaynak, iki manifest, türetilmiş paket

**Bağlam:** Köprü bugüne kadar yalnızca Chromium'u tanıyordu. Edge zaten
çalışıyordu (aynı manifest, aynı `chrome-extension://` kaynağı, registry kaydı
da yazılıyordu); eksik olan Firefox'tu. `docs/tasks.md`'de "ucuz kazanç" diye
duruyordu ve öyle de çıktı — protokol aynı, ayrışan yer yalnızca **manifest ve
başlatma biçimi**.

**Karar:** Firefox destekleniyor. Ayrışan üç nokta tek tek karşılandı:

1. **Köprü manifesti ikiye çıktı.** Chromium `allowed_origins` içinde
   `chrome-extension://<kimlik>/` bekliyor, Firefox `allowed_extensions` içinde
   çıplak kimlik. Windows'ta ikisi de yapılandırma klasörüne yazıldığı için
   dosya adları da ayrıştı (`com.muiget.host.json` /
   `com.muiget.host.firefox.json`); Linux/macOS'ta tarayıcıların sabit
   dizinleri zaten ayrı, dosyanın adı ikisinde de host adı olmak zorunda.
   Registry kökü de ayrı: `HKCU\Software\Mozilla\NativeMessagingHosts`.
2. **Köprü kipi tespiti.** Chrome köprüyü `<exe> chrome-extension://<id>/ …`
   ile başlatıyor; Firefox kaynağı hiç geçirmiyor, yerine **manifest yolunu**
   ve (Firefox 55'ten beri) **eklenti kimliğini** veriyor. `is_host_invocation`
   ikisini de tanıyor. Tanımasaydı Firefox'un her mesaj denemesi köprü yerine
   uygulamanın penceresini açardı ve tek bir indirme bile gelmezdi.
3. **Firefox kimliği kullanıcıdan istenmiyor.** Chrome kimliği uzantının açık
   anahtarının özeti, yani kuruluma göre değişiyor ve sorulmak zorunda.
   Firefox'ta kimliği paketi üretirken biz yazıyoruz (`muiget@muiget.app`), o
   yüzden köprü manifestine kendiliğinden ekleniyor ve ayarlardaki kutu boş
   bırakılabiliyor.

**Uzantı paketleri türetiliyor, elle çoğaltılmıyor.** `tools/uzanti-paketle.js`
`extension/` klasöründen `dist-extension/chrome` ve `dist-extension/firefox`
üretiyor. İkinci bir `manifest.firefox.json` **bilerek yok**: iki manifesti
elle eşit tutmak, her değişiklikte kaçırılabilecek bir adım demekti — uzantının
sürüm numarası tam da bu yüzden üç yayın boyunca geride kalmıştı. Firefox
manifesti Chrome manifestinden türetiliyor; ayrıştıkları yerler betikte tek tek
yazılı (olay sayfası, `browser_specific_settings`, `minimum_chrome_version`in
çıkarılması).

Aynı betik `--magaza` bayrağıyla doğrudan medya yakalamayı kapatıyor
(karar #27). Sabit bulunamazsa betik **hata veriyor**: sessizce geçmek,
YouTube yakalaması açık bir paketi Web Store'a göndermek olurdu.

**`background.js` artık modül değil.** Chrome'da servis çalışanı, Firefox'ta
olay sayfası olarak yükleniyor (Firefox MV3'te arka plan servis çalışanı yok) ve
olay sayfası bağlamında `export` doğrudan bir sözdizimi hatası. Dosyadaki
`export`lar hiçbir yerden `import` edilmiyordu, kaldırıldılar. Tarayıcı API'si
tek bir sabitin arkasında: `const api = globalThis.browser ?? globalThis.chrome`
— Firefox'ta Promise döndüren ad `browser`, Chrome/Edge'de `chrome`.

**En düşük Firefox 128.** `optional_host_permissions` Firefox 128'de geldi.
Daha aşağı inmek, uzantıyı yükleyip video yakalamanın sessizce çalışmadığı bir
tarayıcıya izin vermek olurdu.

**Kabul edilen bedel:** Firefox kimliği uzantının kendi beyanı olduğu için
sahtelenebiliyor — geçici olarak yüklenmiş başka bir eklenti aynı kimliği
yazıp köprüye ulaşabilir. Chrome'da bu mümkün değil (kimlik açık anahtardan
türüyor). Kapı yine de dar: köprü yalnızca `http(s)` adresi kabul ediyor ve
indirme eklemekten başka bir şey yapmıyor. Alternatif — Firefox kullanıcısından
da kimlik istemek — kullanıcının elinde olmayan bir bilgiyi sormak olurdu.

**Üretilmiş paketler depoda tutuluyor.** `dist-extension/chrome` ve
`dist-extension/firefox` commit'leniyor. Normalde derleme çıktısı depoya
girmez; buradaki gerekçe kullanıcı tarafında: uzantı mağazalarda olmadığı için
tek kurulum yolu "klasörü seç" ve o klasörün var olması için insanın Node
kurup bir betik çalıştırması gerekseydi uzantıyı kimse kurmazdı. Ayrışma riski
—çıktının kaynakla uyumsuz kalması— CI'a bağlandı: `npm run uzanti` sonrası
`git diff` boş değilse derleme düşüyor. Aynı paketler her yayına zip olarak
da ekleniyor (`release.yml` → `uzanti` işi).

**Denenmedi:** kod ve testler hazır, gerçek bir Firefox kurulumunda
çalıştırılmadı. Chrome tarafı 8. oturumda gerçek Chrome'la doğrulanmıştı;
Firefox için aynı doğrulama bekliyor.
