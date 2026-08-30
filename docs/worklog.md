# Worklog — Muiget

Her oturumda yapılan işler burada kronolojik (en yeni en üstte) tutulur.
Yeni bir oturuma başlarken sadece en üstteki girdiyi okumak yeterli olmalı.

Format: Tarih, yapılanlar, kararlar, sıradaki adım.

---

## 2026-08-30 (9. oturum) — Proxy, Kimlik Doğrulama, Checksum, Pano İzleme, Sürüm Kontrolü, Çapraz Platform Yayın

İlker "daha neler eklenebilir, IDM ile ne zaman yarışır" diye sordu; ardından
"yapabileceğin her şeyi tek oturumda yap" dedi. Bu oturum o listenin kod
tarafındaki maddelerini kapatıyor.

Önce durum tespiti: motor ve arayüz büyük ölçüde bitmişti, eksik olan şey kod
değil **başka birinin kurup kullanabilmesiydi**. Koda bakınca üç boşluk çıktı —
proxy yok, kimlik doğrulama yok, güncelleme yolu yok — ve hepsi "kurumsal ağda
hiç çalışmaz / korumalı linki indiremez / güncellenmez" demek oluyordu.

### Vekil sunucu desteği (karar #19)

`ManagerConfig.proxy` tek bir dizge; `reqwest`'e `socks` özelliği eklendi, yani
`socks5://` de çalışıyor. Şemasız yazılan `10.0.0.1:8080` `http://` sayılıyor,
desteklenmeyen şema boşaltılıyor.

Boşaltmanın sebebi öğrenilmiş bir davranış: `Client::builder()` bozuk vekille
hiç kurulamıyor ve o hâlde uygulama **tek bir dosya bile** indiremezdi. Bir ayar
alanının yanlış doldurulması uygulamayı işlevsiz bırakmamalı.

Yol boyunca öğrenilen: `reqwest::Proxy::all` şemayı istemci kurulurken
doğrulamıyor — `ftp://` sessizce kabul edilip ilk istekte patlıyor. İlk yazılan
test bunun tersini varsaydığı için düştü; test, gerçeği kayda geçirecek şekilde
yeniden yazıldı (yanlış varsayımı doğrulayan bir testi "düzeltmek" yerine).

### Adresteki kimlik bilgisi (karar #20)

`https://ali:gizli@site/dosya.zip` artık çalışıyor. Kimlik motorun **en dış
kapısında** ayrılıp `Authorization: Basic` başlığına taşınıyor, adres temiz
kaydediliyor. Yoksa parola listede, log'da ve `.muiget` metasında düz metin
dururdu.

Mevcut başlık boru hattı (karar #14) yeniden kullanıldı: başlıklar zaten metaya
yazılıyor ve her segment isteğine ekleniyor. Yeni bir kavram eklemek aynı işi
ikinci kez yapmak olurdu.

Uçtan uca test için test sunucusuna `KimlikIster` kipi eklendi: kimlik yoksa
401. Başlık **tek bir segmentte** unutulsa test düşer. İkinci bir test kimliksiz
isteğin gerçekten 401 aldığını gösteriyor — yoksa ilk test, sunucu her isteği
kabul ettiği için de geçerdi.

### Checksum (karar #21)

`download/checksum.rs`: SHA-256 ve MD5, akış hâlinde, her blok sonrası
`yield_now()`. Satır sağ tık menüsünde "SHA-256 hesapla"; sonuç kopyalanabilir
bildirimle çıkıyor.

Otomatik değil: 8 GB'ı hash'lemek diski baştan sona bir kez daha okumak demek ve
kullanıcıların çoğu bu değere hiç bakmıyor. Yarım dosyada da çalışmıyor — onun
özeti kullanıcıya "indirme bozuk" dedirtirdi.

### Kopya indirme tespiti (karar #22)

`find_duplicate` aynı adresi listede arıyor, yeni indirme diyaloğu uyarı
gösteriyor. **Engel değil**: aynı dosyayı bilerek yeniden indirmek meşru bir
istek. Karşılaştırma kimlik ayıklandıktan sonra yapılıyor, yani parolalı ve
parolasız yazım aynı indirme sayılıyor.

### Pano izleme (karar #24) — IDM'in imza davranışı

Kopyalanan adres indirilebilir bir dosyaya işaret ediyorsa uygulama soruyor.

Rust tarafında yazıldı, arayüzde değil: webview'in pano okuması pencerenin
odakta olmasını gerektiriyor, oysa özelliğin bütün anlamı kullanıcı
**tarayıcıdayken** kopyaladığı adresi yakalamak.

Varsayılan kapalı ve süzgeç dar (tek satır + `http(s)` + tanınan dosya
uzantısı). Panoyu sürekli okumak, kullanıcının kopyaladığı her şeyi görmek
demek; bunun sessizce açık gelmesi bu projede yanlış olurdu.

### Sürüm kontrolü (karar #23) ve orada bulunan hata

Açılışta GitHub'ın yayın listesine bakılıyor, yeni sürüm varsa bildirim çıkıyor.
İmzalı updater kurulmadı: anahtar çifti olmadan yarım kurulan bir updater,
uygulamayı hiç güncellenemez hâle getirirdi.

**İlk uygulama `/releases/latest` kullanıyordu ve bu depoda 404 dönüyor** — o uç
nokta ön sürümleri atlıyor, Muiget'in bütün yayınları ise `prerelease: true`.
Yani özellik her seferinde sessizce başarısız olurdu. Gerçek API'ye bakılmasa
görülmezdi; 8. oturumun `--native-host` hatasıyla aynı sınıf: derleniyor,
testleri geçiyor, çalışmıyor.

Şimdi `/releases?per_page=10` çekiliyor, taslaklar eleniyor ve en yüksek sürüm
**liste sırasına güvenmeden** seçiliyor. Seçim saf bir fonksiyona
(`en_yeni_yayin`) alındı ve gerçek yanıt biçiminden türetilmiş bir örnekle
test edildi — ağa çıkan test yok.

### Çapraz platform yayın

`release.yml` artık matris: Windows + Linux (`.deb`/`.AppImage`) + macOS
(universal). IDM'in Windows dışına çıkamaması Muiget'in kalıcı farkı ve
derlemeyi ertelemek, o platformlarda ilk hatayı görmeyi de erteliyordu.

Yayın notu sınırı açıkça yazıyor: Linux ve macOS paketleri **derleniyor ama
geliştirici tarafından denenmedi**. Ayrıca paketlerin imzasız olduğu ve
SmartScreen/Gatekeeper uyarısı çıkacağı da yazıldı.

CI'a `ubuntu-22.04` üzerinde `cargo check` işi eklendi: Linux derlemesinin ilk
kez bir etiket atıldığında denenmesi, yayını kırılgan bırakırdı.

### Gerçek pencerede doğrulama

Uygulama derlenip çalıştırıldı, pano dışarıdan (PowerShell ile) üç kez
değiştirildi:

| Panoya konan | Sonuç |
|---|---|
| `https://ornek.com/test-dosyasi.zip` | yakalandı |
| `https://github.com/heraklessii/Muiget` | yakalanmadı (dosya değil) |
| düz metin bir parola | yakalanmadı |

Yani hem özellik çalışıyor hem de "panomdaki parola log'a düşer mi" sorusunun
cevabı görülmüş oldu.

Bu arada öğrenilen bir tuzak: `cargo run` ile açılan debug binary arayüzü
`dist/` yerine `devUrl`den (localhost:1420) yüklüyor. Vite çalışmıyorken pencere
boş kalıyor ve **arayüz kodu hiç çalışmıyor**. Rust tarafını böyle denemek
geçerli, arayüz tarafı için `npm run tauri dev` şart.

### CI kırıldı: öngörülen kırılgan test gerçekten düştü

Push'tan sonra CI'ın Windows test işi düştü — ama Linux `cargo check` işi
(bu oturumda eklenen) ve arayüz işi geçti.

Düşen test `oturum_sonrasi_liste_diskten_geri_yukleniyor`, mesaj
**"duraklatmadan önce veri inmemiş"**. Bu `docs/tasks.md`'de kelimesi kelimesine
öngörülmüş bir riskti: birkaç test sabit `sleep(80–120 ms)` atıp "bu arada veri
inmiştir" varsayıyordu. Yavaş runner'da o varsayım tutmadı; yeni testlerin
getirdiği ek yük muhtemelen dengeyi bozdu.

Yerelde sekiz koşuda (yük altında, iki çekirdeğe kısıtlanmış, `--test-threads=2`
ile) tekrar üretilemedi — kırılgan testin tanımı da bu zaten.

**Çözüm eşikleri büyütmek değil.** Ne kadar büyütülse daha yavaş bir makinede
yine yetmeyebilir, hızlı makinede ise her koşu boşuna beklerdi. Bunun yerine
`veri_akmaya_baslayinca()` yardımcısı eklendi: indirme ilk byte'ı alana kadar
(en fazla 20 sn, 10 ms'de bir yoklayarak) bekliyor, sonra duraklatma/iptal
uygulanıyor. Beş çağrı yeri dönüştürüldü. Yan fayda: indirme artık mümkün olan
**en erken** anda yakalanıyor, yani "duraklatma bitmeden yetişmeli" tarafı da
sağlamlaştı.

Geriye kalan iki 200 ms'lik bekleyiş bilerek duruyor: onlar bir olayın
*olmamasını* sınıyor (duraklatılan indirme kendiliğinden başlamamalı) ve
yanlışlıkla geçebilir ama yanlışlıkla düşemez. Koda not olarak yazıldı.

CI loglarını okumak giriş istiyor; hata İlker'in yapıştırmasıyla görüldü.
Public olan check-run annotation'ı yalnızca "exit code 1" diyor.

### Sayılar

198 test (179 birim + 19 uçtan uca), `cargo clippy --all-targets -D warnings`
temiz, `npm run build` temiz. Önceki oturum: 158.

### Sıradaki

Kod tarafında IDM'e yaklaştıran en büyük boşluk artık **HLS/DASH (m3u8) video
indirme** (Faz 6) — bugün IDM'i satan asıl özellik o. Ondan sonra Firefox/Edge
uyarlaması ve Chrome Web Store yayını.

İlker'e kalan, kod dışı ve hâlâ en büyük bilinmeyen: büyük bir dosyayı indirip
IDM ile hız karşılaştırması. Bir akşamlık iş, ölçülene kadar "IDM kadar hızlı"
cümlesi tahmin.

---

## 2026-08-30 (8. oturum) — Köprü Düzeltmesi, v0.1.1 Yayını, Kota Adaleti, IDM Özellikleri

İlker "uzantıyı nasıl kurup test ederim" diye sordu. Kuruluma başlamadan önce,
köprünün Chrome'un gerçekte kullandığı çağrıyla çalışmadığı ortaya çıktı.

### Hata: `--native-host` bayrağı hiçbir zaman gelmiyor

Köprü kipini yalnızca `--native-host` argümanı açıyordu. Chrome bu bayrağı
geçirmiyor: native messaging manifestinde argüman alanı yok ve host şu komutla
başlatılıyor:

```text
muiget.exe chrome-extension://<id>/ --parent-window=<handle>
```

Yani gerçek Chrome her denemede stdio köprüsü yerine **boş bir pencere**
açardı. Birim testler protokolü doğruladığı için hata görünmüyordu — 7.
oturumdaki Tokio çökmesiyle aynı sınıf: "testler geçiyor" ile "çalışıyor"
arasındaki fark.

**Düzeltme:** kip artık `native_host::is_host_invocation()` ile belirleniyor.
`chrome-extension://` önekli argüman **ya da** elle verilen `--native-host`
köprüyü açıyor; `--add` her zaman pencereye gidiyor. Dört birim test eklendi,
karar #13'e düzeltme notu yazıldı.

### Köprü, Chrome olmadan uçtan uca doğrulandı

Host, Chrome'un gerçek argümanlarıyla çalıştırılıp uzunluk önekli mesajlar
gönderildi:

- `ping` → `{"type":"pong","version":"0.1.0"}`; pencere açılmadı, süreç çıktı
- `download` → `{"type":"accepted",...}`; uygulama `--add` ile açıldı, yerel
  sunucudan 2 MB dosya indi, SHA-256 kaynakla **birebir aynı**

Köprü kaydı (manifest + `HKCU` registry) elle yazıldı. Paketlenmemiş uzantının
kimliği Chrome'un algoritmasıyla klasör yolundan hesaplandı — yolun UTF-16LE
baytlarının SHA-256'sı, ilk 16 bayt, hex haneleri `a-p`'ye eşlenerek:
`jmicepilfahlolilhkejmcmcpdhjjokf`.

### Kalan tek adım

Chrome'da **Paketlenmemiş öğe yükle** — bir Windows dosya seçme penceresi,
otomatikleştirilemiyor. Zincirin Chrome'a değen son halkası hâlâ denenmedi.

### Testte görülen risk

Köprü, uygulamayı `--add` ile başlatırken stdout'u miras bırakıyordu ve
borunun EOF'u uygulama kapanana kadar gelmedi. Aynı oturumda kapatıldı —
aşağıdaki "Köprüdeki açık risk kapatıldı" bölümüne bakın.


### v0.1.1 yayınlandı — iş akışı ilk kez uçtan uca çalıştı

Sürüm numarası beş dosyada 0.1.1'e çekildi, `v0.1.1` etiketi itildi ve
`release.yml` bu kez **kendisi** derleyip GitHub Release'ini oluşturdu, iki
kurulum paketini yükledi. v0.1.0'da bu adım izin sorunundan düşmüş ve paket
elle yüklenmişti; o madde artık kapandı.

Kurulu sürüm (`%LOCALAPPDATA%\Muiget`) 0.1.1'e güncellendi ve köprü kaydı debug
binary yerine kurulu exe'ye yönlendirildi.

**Beklenen ama not edilmesi gereken:** CI'ın ürettiği paketin SHA-256'sı yerel
derlemeyle **tutmuyor**. v0.1.0'da tutmasının sebebi paketin elle yüklenen
yerel derleme olmasıydı; CI bağımsız derleyince Rust/NSIS yeniden üretilebilir
çıktı vermiyor. Doğrulanabilir yayın ayrı bir iş.

### Host kotası artık indirmeler arasında bölüşülüyor (karar #17)

Teknik borcun en görünür maddesi kapandı. Aynı siteden üç indirme başlatılınca
ilki sekiz iznin hepsini alıyor, diğer ikisi sıfır byte'ta bekliyordu.

Sebep: izin **segment ömrü boyunca** tutuluyor ve segmentler birlikte bitiyor.
Çözüm izin dağıtımında değil planda: `HostLimiter` host başına indirme sayısını
tutuyor (`register()` → RAII), `fair_share()` payı veriyor, süpervizör segment
planını payla sınırlıyor. Üç indirme × 2 segment = 6 bağlantı.

Rebalans ayrıca yazılmadı: bir indirme bitince kaydı düşüyor, pay büyüyor ve
adaptif bölme (karar #5) boşalan slotu zaten değerlendiriyor.

### Kategori klasörleri (karar #18)

IDM'in imza davranışı: inen dosya türüne göre `Video`, `Müzik`, `Belgeler`,
`Arşivler`, `Programlar`, `Resimler` alt klasörlerine ayrılıyor. Eşleme koda
gömülü, ayarlardan tek anahtarla açılıyor, **varsayılan kapalı** — bir sürüm
yükseltmesinin dosyaların nereye düştüğünü sessizce değiştirmesi doğru olmazdı.

Karar #15 ile etkileşimi atlanmadı: oturumlar arası liste `.muiget` taramasıyla
geri geliyor ve tarama tek seviyeydi. Artık **yalnızca bilinen kategori
klasörlerine** de bakıyor; serbest özyineleme hâlâ yok. İki testi var (kategori
klasörü bulunuyor, rastgele alt klasöre inilmiyor).

### Arayüz: sağ tık menüsü, sürükle-bırak, toplu ekleme

- **Sağ tık menüsü** (`ContextMenu.tsx`): bağlantıyı/dosya adını kopyala,
  duraklat/devam, klasörde göster, yeniden indir, listeden kaldır. Menü duruma
  göre kısalıyor — sönük ama duran maddeler yerine hiç göstermemek.
  Konum açıldıktan sonra ölçülüp pencereye sığdırılıyor.
- **Sürükle-bırak**: `dragDropEnabled: false` yapıldı, yoksa Tauri'nin kendi
  dosya-bırakma yakalayıcısı webview'in HTML5 olaylarını yutuyordu. Bırakılan
  adres doğrudan indirilmiyor, yeni indirme kutusunu dolduruyor.
- **Toplu ekleme**: kutuya birden çok adres yapıştırılınca hepsi kuyruğa
  alınıyor, tek yenileme turuyla. Toplu ekleme yoklamayı atlıyor.

Arayüz tarayıcı panelinde koyu ve açık temada doğrulandı. Gerçek pencerede
denenmedi — sürüklemenin webview davranışı orada görülür.

### Köprüdeki açık risk kapatıldı

Köprü, uygulamayı `--add` ile başlatırken stdout'u miras bırakıyordu; artık
`Stdio::null()`. Chrome'un okuduğu boru, uygulama penceresi yüzünden açık
kalmıyor.

### Sayılar

158 test (143 birim + 15 uçtan uca), `cargo clippy --all-targets -D warnings`
temiz, `npm run build` temiz.

### Sıradaki

Chrome'la ilk gerçek deneme (İlker'in iki tıklaması) ve yeni arayüz
özelliklerinin gerçek pencerede denenmesi. Sonra Faz 4 (torrent) ya da motor
derinliği (dinamik segment sayısı, checksum, pano izleme) — bkz. `tasks.md`.

---

## 2026-08-29 (7. oturum) — Çökme Düzeltildi, Uygulama İlk Kez Gerçekten Çalıştırıldı

İlker "yeni indirme yapmaya kalkınca uygulama çöküyor" dedi. Sebep bulundu,
düzeltildi ve uygulama ilk kez gerçek penceresinde uçtan uca çalıştırıldı.

### Çökmenin sebebi

```
panicked at src/download/manager.rs:778:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

`tokio::spawn` çağıran thread'in **ortam** çalışma zamanı bağlamına bakıyor ve
bağlam yoksa panikliyor. Tauri'nin senkron komutları (`start_download`,
`resume_download`, `resume_all_downloads`), tek-örnek eklentisinin geri
çağırması ve kurulum kancası — hiçbiri o bağlamda değil. Yani "İndir"e basmak
motoru düşürüyordu.

Bu hata en baştan beri koddaydı; uygulama bugüne kadar hiç gerçek penceresinde
çalıştırılmadığı için görünmemişti. Otomatik testler yakalayamazdı: hepsi
`#[tokio::test]` içinde, yani her zaman bağlam **vardı**.

**Düzeltme:** yönetici artık ortam bağlamına güvenmiyor, kurulumda bir
`tokio::runtime::Handle` alıp saklıyor ve tüm görevleri onun üzerinden
başlatıyor. `DownloadManager::new` bağlam yoksa panik yerine anlaşılır bir hata
dönüyor. `lib.rs` motoru `block_on` içinde kuruyor.

İki regresyon testi eklendi; ilki bilerek `block_on` **dışından** çağırıyor ve
düzeltmeden önce panikliyordu.

### İkinci hata: uzantının verdiği dosya adı yok sayılıyordu

`DownloadOptions::file_name` "sunucudan gelen adı ezmek için" diye
belgelenmişti ama `supervise` yoklama sonucundaki adı kullanıyordu. Yani
tarayıcıdan gelen her indirme, tarayıcının çözdüğü ad yerine sunucunun ham
adıyla kaydediliyordu. Düzeltildi, uçtan uca testi var.

### Uygulama ilk kez gerçekten çalıştırıldı

Yayın binary'si `--add` köprüsüyle (çökmenin yaşandığı yol) yerel bir test
sunucusuna karşı çalıştırıldı:

- 8 MB dosya, 8 segmente bölündü, hepsi paralel indi (sunucu logunda sekiz
  ayrı `206 Partial Content`)
- İnen dosyanın SHA-256'sı kaynakla **birebir aynı**
- `.mgpart` ve `.muiget` ara dosyaları temizlendi
- Arayüz gerçek Tauri webview'inde ilk kez görüldü — tarayıcı önizlemesinde
  görünenle aynı çalışıyor

### Gerçek çalıştırmanın ortaya çıkardığı üçüncü sorun

Aynı siteden üç indirme başlatılınca ilki host kotasının (8 bağlantı) tamamını
alıyor, diğer ikisi sıfır byte'ta bekliyor. Sonuç doğru — kota aşılmıyor ve
indirmeler sırayla tamamlanıyor — ama arayüz "İniyor · %0 · —" yazdığı için
takılmış görünüyordu. Arayüz artık bu durumda **"Bağlantı bekleniyor"** diyor
ve anlamsız hız/süre alanlarını göstermiyor. Kotayı indirmeler arasında
paylaştırmak `docs/tasks.md`'ye yazıldı.

### Uygulama ikonu

Varsayılan Tauri ikonu gitti. Yeni ikon `tools/ikon-uret.js` ile üretiliyor:
harici görüntü kütüphanesi yok, Node'un `zlib`'iyle PNG elle yazılıyor, kenar
yumuşatma 4 kat supersampling. Biçim arayüzdeki `IconDownload` ile aynı, renk
Mui ailesinin teal'i. `npx tauri icon` platform boyutlarını üretti; mobil
klasörleri silindi (proje masaüstü hedefliyor).

### Yayın altyapısı

- `.github/workflows/release.yml` — `v*` etiketi itilince `tauri-action` ile
  Windows kurulum paketlerini derleyip GitHub Release'ine yüklüyor. Ön sürüm
  olarak işaretleniyor: 0.1.0 sahada geniş çapta denenmedi.

  İlk koşuşta derleme başarılı oldu ama sürüm oluşturma adımı
  `Resource not accessible by integration` ile düştü. Sebep depo ayarıydı:
  `default_workflow_permissions` `read`ti ve bu, iş akışı dosyasındaki
  `permissions: contents: write` isteğini tavanlıyor. (İlginç ayrıntı: aynı
  izinle `gh-pages` dalına `git push` çalışıyordu — kısıt yalnızca REST API
  tarafında hissediliyor.) Ayar `write` yapıldı; her iş akışı zaten kendi
  ihtiyacı kadar izni açıkça bildiriyor, yani gevşeme iş akışı düzeyinde
  sınırlı kalıyor. `can_approve_pull_request_reviews` kapalı bırakıldı.

  v0.1.0 bu yüzden **elle** yayınlandı (yerel derlemedeki paketler yüklendi;
  indirilen dosyanın SHA-256'sı yerel paketle birebir aynı doğrulandı).
  İş akışının sürüm oluşturma adımı bir sonraki etikette sınanacak.
- `.github/workflows/pages.yml` — `site/` klasörünü GitHub Pages'e yayınlıyor.
  Ekran görüntüsü, ikon ve fontlar depodaki tek kopyadan kopyalanıyor.

  İlk deneme `actions/deploy-pages` ile yapıldı ve **başarısız oldu**:
  `configure-pages`in `enablement: true` seçeneği varsayılan `GITHUB_TOKEN`
  ile Pages sitesi oluşturamıyor, çünkü bu işlem depo yöneticisi yetkisi
  istiyor. İş akışı ilk adımda düştü. Çözüm `gh-pages` dalına itmek: yalnızca
  `contents: write` gerekiyor, elle ayar değişikliği gerekmiyor ve dalın
  varlığı Pages'i kendiliğinden açıyor (GitHub'ın kendi
  "pages build and deployment" işi tetikleniyor).
- `site/index.html` — tanıtım sayfası. "Ne yapmaz" bölümü sayfanın ortasında,
  kendi zemininde duruyor: projenin sınırı gizlenecek bir şey değil.

**Doğrulama:** 130 birim + 13 uçtan uca test, clippy temiz, yayın build'i
üretiliyor, uygulama gerçek pencerede indirme yapıyor.

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
