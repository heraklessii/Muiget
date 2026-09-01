# CLAUDE.md — Muiget Proje Rehberi

Bu dosya, Claude Code (veya başka bir oturum) projeye her girdiğinde ilk okuması
gereken bağlam dosyasıdır. Amaç: sıfırdan bağlam kurmadan kaldığın yerden devam
edebilmek.

## Proje Nedir

**Muiget**, açık kaynaklı (Apache 2.0), IDM/FDM tarzı bir indirme yöneticisi.
HTTP/HTTPS çoklu-bağlantılı (segmented) indirme, torrent desteği ve bir tarayıcı
uzantısı (Chrome/Edge/Firefox) ile tarayıcıdan yakalama içerecek. Hedef: IDM'e gerçek, ücretsiz,
şeffaf bir alternatif olmak — kapalı kaynak değil, ücretli değil.

Geliştirici: İlker (solo developer). Muitoon platformunun da sahibi/geliştiricisi,
oradan gelen Node.js/extension deneyimi bu projeye taşınıyor.

## Kapsam Dışı — Net Sınır

Bu proje **sitelerin indirme limitlerini veya premium linkli servisleri (Rapidgator,
Mega, vb.) aşan bir araç değildir.** Böyle bir özellik hiçbir zaman eklenmeyecek.
"Hızlı indirme" burada sadece meşru HTTP seviyesinde optimizasyon anlamına gelir:
paralel Range istekleri, resume, bant genişliği yönetimi. Bir talep bu sınırı
aşmayı hedefliyorsa reddedilmeli ve bu dosyadaki sınır hatırlatılmalı.

## Teknoloji Yığını

| Katman | Teknoloji | Neden |
|---|---|---|
| Masaüstü kabuk | Tauri v2 | Electron'a göre çok daha düşük RAM/binary boyutu, Rust native |
| Backend mantığı | Rust + Tokio | Async paralel indirme için doğal uyum, güvenli bellek yönetimi |
| HTTP istemci | reqwest | Stream + Range header desteği hazır |
| Torrent motoru | librqbit | Apache-2.0, saf Rust, zaten Tauri masaüstü örneği var (rqbit projesi) |
| Akış videosu | Kendi kodumuz (`src/media/`) + `aes`/`cbc` | m3u8/MPD ayrıştırma ve parça birleştirme dış bağımlılık istemiyor; AES için RustCrypto |
| Kap dönüşümü | ffmpeg — **isteğe bağlı** dış araç | Gömmek paketi on katına çıkarırdı; yalnızca `.ts`→`.mp4` ve ses/video birleştirme için gerekli (karar #25) |
| Frontend UI | React + Vite + TypeScript | Tauri ile birinci sınıf entegrasyon |
| Extension | MV3 — Chrome/Edge + Firefox | İlker'in Muitoon extension deneyimiyle örtüşüyor; Firefox paketi Chrome manifestinden türetiliyor (karar #31) |
| Extension ↔ App köprüsü | Native Messaging (stdin/stdout, length-prefixed JSON) | Tarayıcıların desteklediği tek güvenli yerel IPC yolu; Firefox aynı protokolü konuşuyor (karar #31) |

## Lisans

Apache License 2.0 — hem ana uygulama hem tarayıcı uzantısı için. Her yeni
dosyanın başına lisans header'ı eklenmeyecek (Apache 2.0 bunu zorunlu kılmıyor,
NOTICE dosyası yeterli) — sade tutulacak.

## Dizin Yapısı

`✅` = var ve derleniyor, `⬜` = henüz yok (planlanan).

```
muiget/
├── CLAUDE.md                   ✅ Bu dosya
├── LICENSE                     ✅ Apache 2.0 tam metni
├── NOTICE                      ✅ Üçüncü parti lisans bildirimleri
├── README.md                   ✅ Kullanıcıya dönük tanıtım
├── .gitignore                  ✅
├── package.json                ✅ npm — Vite/React + @tauri-apps/cli
├── tsconfig.json               ✅ Tek tsconfig (src + vite.config.ts)
├── vite.config.ts              ✅ Port 1420, strictPort, src-tauri watch ignore
├── index.html                  ✅
├── .github/workflows/          ✅ ci.yml, release.yml, pages.yml
├── site/                       ✅ GitHub Pages tanıtım sayfası (index.html)
├── tools/                      ✅ ikon-uret.js (uygulama ikonu üretici),
│                                  uzanti-paketle.js (Chrome + Firefox paketi)
├── docs/
│   ├── ekran-goruntusu.png     ✅ Uygulamanın kendi penceresinden
│   ├── project_overview.md     ✅ Ürün vizyonu, hedef kitle, rakip analizi
│   ├── decisions.md            ✅ Mimari kararlar (ADR tarzı, kronolojik)
│   ├── worklog.md              ✅ Oturum bazlı ilerleme günlüğü
│   └── tasks.md                ✅ Yapılacaklar listesi, aşamalara bölünmüş
├── src/                        ✅ React/Vite frontend
│   ├── main.tsx                ✅
│   ├── App.tsx                 ✅ Kabuk: topbar, araç çubuğu (filtre/arama/
│   │                              sıralama/toplu eylem), liste, durum çubuğu
│   ├── styles.css              ✅ Tasarım sistemi (Mui paleti, karar #10)
│   ├── assets/fonts/           ✅ Outfit (gömülü, OFL-1.1)
│   ├── components/             ✅ DownloadRow, AddDialog, SettingsDialog,
│   │                              SpeedGraph, Toasts, Icons, ContextMenu
│   ├── hooks/                  ✅ useDownloads.ts (ilerleme yayınına abonelik),
│   │                              useHotkeys.ts (klavye kısayolları)
│   └── lib/                    ✅ api.ts (invoke sarmalayıcıları), types.ts,
│                                  format.ts, notify.ts (OS bildirimi),
│                                  clipboard.ts (pano, yedek yollu)
├── src-tauri/                  ✅ Rust backend
│   ├── Cargo.toml              ✅ crate: muiget, lib: muiget_lib
│   ├── tauri.conf.json         ✅ identifier: com.muiget.app
│   ├── capabilities/           ✅ Dar izin listesi
│   ├── icons/                  ✅ (varsayılan Tauri ikonları — değiştirilecek)
│   ├── tests/                  ✅ indirme_uctan_uca.rs, akis_uctan_uca.rs
│   │                              (ikisi de yerel HTTP sunucusu kaldırıyor)
│   └── src/
│       ├── main.rs             ✅ --native-host kipi ayrımı
│       ├── lib.rs              ✅ Tauri kurulumu, tepsi, tek örnek, pano izleyici
│       ├── commands.rs         ✅ Frontend'e açılan komutlar
│       ├── settings.rs         ✅ settings.json, normalize(), proxy doğrulama
│       ├── clipboard.rs        ✅ Pano süzgeci (karar #24)
│       ├── update.rs           ✅ GitHub yayın kontrolü (karar #23)
│       ├── download/           ✅ Faz 1–2
│       │   ├── mod.rs             # Hata tipleri
│       │   ├── category.rs        # Uzantı → kategori klasörü (karar #18)
│       │   ├── checksum.rs        # SHA-256 / MD5 (karar #21)
│       │   ├── http.rs            # probe(), proxy'li istemci, kimlik ayırma
│       │   ├── segmenter.rs       # Range parçalarına bölme planı (saf)
│       │   ├── writer.rs          # Sparse file, seek+write
│       │   ├── resume.rs          # .muiget meta, tazelik kontrolü
│       │   ├── worker.rs          # Segment task'ı, retry + backoff
│       │   ├── speed.rs           # EWMA hız ölçümü
│       │   ├── throttle.rs        # Token bucket, host kotası + adil pay
│       │   │                      #   (karar #17), zaman kuralları
│       │   └── manager.rs         # Orkestrasyon + adaptif bölme
│       ├── media/              ✅ Faz 6 — akış videosu (karar #25)
│       │   ├── mod.rs             # Tipler, protokol tespiti, plan, seçim
│       │   ├── url.rs             # Manifestteki göreli adresleri çözme
│       │   ├── xml.rs             # Küçük XML okuyucu (yalnızca DASH için)
│       │   ├── m3u8.rs            # HLS master + medya playlist
│       │   ├── mpd.rs             # DASH manifesti
│       │   ├── vtt.rs             # WebVTT parçalarını birleştirme (karar #29)
│       │   ├── crypt.rs           # AES-128 parça çözme, DRM reddi
│       │   ├── pipeline.rs        # Paralel indir, sırayla yaz
│       │   └── mux.rs             # ffmpeg bulma ve çağırma
│       ├── extension_bridge/   ✅ Faz 5
│       │   ├── mod.rs             # İstek işleme, host kaydı
│       │   └── native_host.rs     # Native messaging protokolü + tarayıcıya
│       │                          #   göre manifest (Chromium / Firefox)
│       └── torrent/            ⬜ Faz 4
│           └── engine.rs          # librqbit sarmalayıcı
├── dist-extension/             ✅ Üretilmiş uzantı paketleri — DEPODA
│   ├── chrome/                    # Kullanıcı klonlayıp doğrudan yükleyebilsin
│   └── firefox/                   # diye commit'leniyor; güncelliğini CI kontrol
│                                  # ediyor (npm run uzanti → git diff boş olmalı)
└── extension/                  ✅ Faz 5 — MV3 uzantısı (Chrome/Edge/Firefox)
    ├── manifest.json           ✅ Tek kaynak; Firefox'unki buradan türetiliyor
    ├── background.js           ✅ Sağ tık, köprü, indirme devralma
    │                              (modül değil: Firefox'ta olay sayfası)
    ├── popup.html/.js/.css     ✅ Sayfa taraması, ayarlar
    ├── icons/                  ✅
    └── README.md               ✅ Üç tarayıcı için kurulum, izin açıklamaları
```

## Geliştirme Komutları

```bash
npm run dev       # sadece frontend (Vite, localhost:1420)
npm run build     # tsc + vite build → dist/
npm run tauri dev # tam uygulama (Rust + pencere)
cargo check       # src-tauri/ içinde, hızlı Rust doğrulaması
cargo test        # 300 birim + 41 uçtan uca test
npm run uzanti    # extension/ → dist-extension/{chrome,firefox}
npm run uzanti:magaza  # mağaza derlemesi: YouTube yakalaması kapalı (#27)
```

Uçtan uca testler elle yazılmış küçük HTTP sunucuları kaldırıp motoru onlara
karşı çalıştırıyor; hazır bir test sunucusu bu kontrolü vermiyordu.
`indirme_uctan_uca.rs` `Range` davranışını (206 / 200 / "Range'i yok say")
teste göre değiştirebiliyor; `akis_uctan_uca.rs` manifest, parça, anahtar ve
MPD gibi farklı yanıtları bir yönlendirme tablosundan verip **hangi yolun kaç
kez istendiğini sayıyor** — devam etmenin gerçekten parçaları atladığını başka
türlü kanıtlayamıyorduk.

ffmpeg entegrasyonu sahte bir ffmpeg betiğiyle sınanıyor: gerçek ffmpeg her
makinede yok ve olsa bile testi onun sürümüne bağlamak sonucu değişken yapardı.

> Tauri CLI global kurulu değil, `package.json`'daki `@tauri-apps/cli`'den
> geliyor (bkz. `docs/decisions.md` #9). Bu yüzden `cargo tauri dev` değil,
> `npm run tauri dev` kullanılıyor.

## Çekirdek Mimari Kararlar (Özet)

Detaylar için `docs/decisions.md`. Kısa özet:

1. **Segmentasyon**: HEAD isteğiyle `Accept-Ranges` ve `Content-Length` kontrol
   edilir. Destekleniyorsa dosya N parçaya (varsayılan 8) bölünür, her parça
   ayrı async task ile paralel indirilir.
2. **Sparse file yazma**: Ayrı `.part` dosyaları yerine hedef dosya baştan tam
   boyutuna `set_len` ile ayrılır, her worker kendi offsetine `seek+write`
   yapar. Birleştirme adımı yok.
3. **Resume**: Her indirme için `<dosya>.muiget` JSON meta dosyası tutulur
   (segment durumları, ETag, Last-Modified). Uygulama çökse bile kaldığı
   yerden devam edilir.
4. **Adaptif segment bölme**: Bir segment yavaş/başarısızsa kalan byte aralığı
   ikiye bölünüp boşta kalan worker'a devredilir ("work stealing").
5. **Oturumlar arası liste**: ayrı bir liste veritabanı yok. Açılışta indirme
   klasörü taranıp `.muiget` dosyaları okunuyor; durum her zaman dosyanın
   yanında duruyor, iki gerçeklik kaynağı ayrışması olmuyor (karar #15).
6. **Kuyruk**: eşzamanlı indirme sınırı yöneticide, tek bir `pump()`
   fonksiyonunda uygulanıyor. `start`/`resume` süpervizörü doğrudan
   başlatmıyor, isteği kuyruğa bırakıyor (karar #16).
7. **Host kotası adaleti**: kota (varsayılan 8 bağlantı) aynı sunucudaki
   indirmelere bölüştürülüyor. Adalet izin dağıtımında değil segment planında:
   her indirme yalnızca payı kadar segment açıyor. Bir indirme bitince pay
   büyüyor ve adaptif bölme boşalan slotu değerlendiriyor (karar #17).
8. **Kategori klasörleri**: uzantıya göre `Video`, `Müzik`, `Belgeler`…
   Gömülü eşleme, varsayılan kapalı. Tarama (madde 5) bu klasörlere de
   bakıyor, başkalarına değil (karar #18).
9. **Vekil sunucu**: tek alan (`engine.proxy`), boş = doğrudan. `socks5://`
   destekli. Geçersiz şema boşaltılıyor — bozuk vekille istemci hiç kurulamaz
   ve uygulama tek dosya bile indiremezdi (karar #19).
10. **Kimlik bilgisi**: `https://kullanıcı:parola@…` motorun kapısında ayrılıp
    `Authorization` başlığına taşınıyor; adres listede/log'da/metada parolasız
    duruyor (karar #20).
11. **Pano izleme**: Rust tarafında saniyede bir, dar süzgeçle (tek satır +
    http(s) + tanınan dosya uzantısı), varsayılan kapalı (karar #24).
12. **Sürüm kontrolü**: GitHub yayın listesi. İmzalı updater yok. Bu,
    uygulamanın kendiliğinden yaptığı tek dış istek (karar #23).
13. **Akış videosu (HLS/DASH)**: manifest ayrıştırma, parça indirme, `AES-128`
    çözme ve birleştirme kendi kodumuzda (`src/media/`); ffmpeg **isteğe
    bağlı** ve yalnızca `.ts`→`.mp4` dönüşümü ile ayrı inen sesin
    birleştirilmesi için. Parçalar paralel iniyor ama **sırayla** yazılıyor;
    devam noktası "kaç parça + kaç byte". Ayrı ses varken ffmpeg yoksa indirme
    tek byte inmeden duruyor. DRM (SAMPLE-AES/Widevine/PlayReady) ve canlı
    yayın ayrıştırma anında reddediliyor (karar #25).
14. **Uzantıda video yakalama**: `webRequest` isteğe bağlı izin, varsayılan
    kapalı; manifest adresleri sekme başına `storage.session`de (karar #26).
15. **Torrent**: librqbit `Session` API'si üzerinden magnet/`.torrent` desteği.
    Sequential download modu (streaming izleme) ileride eklenecek.
16. **YouTube (manifestsiz) yakalama**: `.m3u8`/`.mpd` süzgecinin yanına
    doğrudan medya kuralları eklendi. Tek sabite bağlı
    (`DOGRUDAN_MEDYA_YAKALAMA`): GitHub paketinde açık, Chrome Web Store
    derlemesinde kapalı — mağaza politikası YouTube indirmeyi yasaklıyor.
    İmza çözülmüyor; tarayıcının zaten istediği adres görülüyor. Sessiz video
    akışları popup'ta işaretleniyor (karar #27).
17. **Ses indirme**: varsayılan `-vn -c:a copy` ile kayıpsız çıkarma
    (`.m4a`/`.opus`); MP3 isteğe bağlı ve yeniden kodluyor. Çıktı uzantısı
    `codecs` alanından türetiliyor (karar #28).
18. **Altyazı**: HLS `SUBTITLES` ve DASH `text` parçaları videonun yanına
    `film.tr.vtt` olarak iniyor. Parçalar uç uca eklenmiyor — her biri tam bir
    WebVTT belgesi olduğu için `media/vtt.rs` cue düzeyinde birleştirip
    `X-TIMESTAMP-MAP`e göre hizalıyor. Varsayılan açık; altyazı hiçbir koşulda
    indirmeyi düşürmüyor (karar #29).
19. **İlerleme olayları biriktiriliyor**: chunk başına değil, 256 KB ya da
    100 ms'de bir. Tek tüketici hız ölçer; ilerlemenin kaynağı `downloaded`
    atomiği (karar #30).
20. **Firefox desteği**: protokol aynı, ayrışan üç nokta karşılandı — ayrı
    köprü manifesti (`allowed_extensions`, ayrı dosya adı, `HKCU\Software\
    Mozilla\…`), Firefox'un verdiği argümanların (manifest yolu + eklenti
    kimliği) köprü kipi sayılması ve sabit eklenti kimliği
    (`muiget@muiget.app`, kullanıcıdan istenmiyor). Uzantı paketleri
    `tools/uzanti-paketle.js` ile **tek kaynaktan türetiliyor**; ikinci bir
    elle yazılmış manifest yok (karar #31).

## Çalışma Tarzı Notları

- İlker doğrudan ve gayri resmi iletişim kuruyor, hızlı aksiyon tercih ediyor.
- Uzun açıklamadan çok somut adım/kod istiyor. "Bir sürü şey yap" dediğinde
  beklenen küçük bir düzeltme değil, bitmiş ve testli birkaç iş.
- Öncelik ölçütü: **IDM ile arayı kapatmak**. Bir özelliğin "IDM'de var mı"
  sorusu, iç güzellikten önce geliyor.
- Her yeni oturumda önce `docs/worklog.md`'nin son girdisini, sonra
  `docs/tasks.md`'nin "Sıradaki" bölümünü oku.

## Sıradaki Adım

**Faz 0, 1, 2, 3, 5 ve Faz 6'nın çekirdeği tamamlandı.** Çalışan bir segmentli
indirme motoru, **HLS/DASH video indirme**, tam bir arayüz ve **Chrome/Edge +
Firefox** uzantısı var. 341 test geçiyor. Uygulama gerçek penceresinde uçtan
uca doğrulandı (8 MB dosya, 8 paralel segment, SHA-256 birebir aynı) ve
10. oturumda akış indirmesi de aynı yöntemle doğrulandı (yerel VOD playlisti,
parçalar paralel indi, SHA-256 birebir). Chrome köprüsü de Chrome'un gerçek
çağrısıyla doğrulandı — **Firefox köprüsü denenmedi**, yalnızca testli; son
yayın v0.1.5.

IDM'e yaklaştıran eklemeler: host kotasının indirmeler arasında adil
bölüşülmesi (karar #17), kategori klasörleri (#18), vekil sunucu (#19),
adrese gömülü kimlik bilgisi (#20), checksum (#21), kopya uyarısı (#22),
sürüm bildirimi (#23), pano izleme (#24), **akış videosu (#25)**, **uzantıda
video yakalama (#26)**, YouTube yakalama (#27), ses çıkarma (#28),
**altyazı (#29)**, satır sağ tık menüsü, sürükle-bırakla bağlantı ekleme
ve toplu ekleme.

Yayın artık üç platform: Windows + Linux (`.deb`/`.AppImage`) + macOS
(universal). Linux/macOS paketleri **derleniyor ama sahada denenmedi**; yayın
notu bunu açıkça yazıyor. CI'da Linux `cargo check` işi var.

Teknik borcun çoğu kapandı: indirme listesi oturumlar arası korunuyor
(açılışta `.muiget` taraması), uzantıdan gelen başlıklar metaya yazılıyor,
eşzamanlı indirme sayısı sınırlanabiliyor. Açık kalan tek borç: **arayüzün
otomatik testi yok** (Vitest kararı İlker'in).

**Faz 4 (torrent) bilinçli olarak ertelendi:** librqbit ~100 yeni bağımlılık
getiriyor ve gerçek bir swarm'a karşı denenmeden doğru çalıştığı söylenemez.
HTTP motorunu yerel bir sunucuyla test edebiliyoruz; torrent'te "derleniyor"
yeterli kanıt değil.

Sıradaki öncelikler (`docs/tasks.md` → "Sıradaki"):

1. **Gerçek dünya doğrulaması** — büyük bir dosyayı indirip IDM ile hız
   karşılaştırması, ve Chrome'da uzantının yüklenmesi. İkisi de kod işi değil;
   İlker'in makinesinde denenmesi gerekiyor. Ölçüm yapılana kadar "IDM kadar
   hızlı" cümlesi tahmin.
2. **Video akışının sahada denenmesi** — kod ve testler hazır ama gerçek bir
   video sitesine karşı hiç denenmedi; ffmpeg'li birleştirme de gerçek
   ffmpeg'le çalıştırılmadı (bu makinede ffmpeg yok). **Altyazı da bu listede:**
   13. oturumda eklendi, yerel sunucuya karşı altı uçtan uca testi var, gerçek
   bir sağlayıcıya karşı hiç çalıştırılmadı.
3. **Tarayıcı kapsamı** — kod tarafı 14. oturumda bitti (karar #31): Firefox
   ve Edge destekleniyor, paketler `npm run uzanti` ile üretiliyor. Kalan iki
   iş kod değil: Firefox'ta gerçek deneme ve mağaza yayınları (Chrome Web
   Store + AMO).
4. **Faz 4 (torrent)** — IDM'de zaten yok; 2 ve 3'ten sonra.

**İlker'e kalan (kod dışı):**
- Tarayıcıya uzantıyı yükleme: önce `npm run uzanti`, sonra Chrome/Edge'de
  `chrome://extensions` → **Paketlenmemiş öğe yükle** → `dist-extension/chrome`,
  Firefox'ta `about:debugging` → **Geçici Eklenti Yükle** →
  `dist-extension/firefox/manifest.json`. Köprünün geri kalanı (host kaydı,
  kimlik, uçtan uca indirme) Chrome'da 8. oturumda doğrulandı; dosya seçme
  penceresi otomatikleştirilemiyor. **Firefox tarafı hiç denenmedi.**
- **Kod imzalama sertifikası.** Paketler imzasız olduğu sürece Windows
  SmartScreen ve macOS Gatekeeper uyarı gösteriyor; indirenlerin çoğu orada
  duruyor. Kodla çözülmüyor.
- **ffmpeg.** İsteğe bağlı ama olmadan ayrı sesli yayınlar (DASH'in tamamı)
  inmiyor. Kurup Ayarlar → "ffmpeg yolu" alanına yazmak ya da `PATH`e eklemek
  yeterli; uygulama yanındaki kopyayı da buluyor.

**Uyarı — arayüzü denerken:** `cargo run` ile açılan debug binary arayüzü
`dist/` yerine `devUrl`den (localhost:1420) yüklüyor. Vite çalışmıyorken
pencere boş kalır ve arayüz kodu hiç çalışmaz. Rust tarafını böyle denemek
geçerli; arayüz için `npm run tauri dev` şart.

Her yeni oturum şu sırayla okunmalı:
1. Bu dosya (`CLAUDE.md`) — genel bağlam
2. `docs/worklog.md` — en üstteki (en son) girdi
3. `docs/tasks.md` — "Sıradaki" bölümü
4. Gerekirse `docs/decisions.md` — ilgili kararın numarasına bak
