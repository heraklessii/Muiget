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
