# Muiget

**Açık kaynak indirme yöneticisi.** IDM'in yaptığını yapan, ama kimsenin lisans
anahtarı satın alması gerekmeyen, kodunu herkesin okuyup değiştirebildiği bir araç.

[![CI](https://github.com/heraklessii/Muiget/actions/workflows/ci.yml/badge.svg)](https://github.com/heraklessii/Muiget/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/durum-geli%C5%9Ftirme%20a%C5%9Famas%C4%B1nda-orange.svg)](docs/tasks.md)

> ⚠️ **Bu proje erken geliştirme aşamasında.** İndirme motoru, video akışı
> indirme, arayüz ve tarayıcı uzantısı çalışıyor (341 test geçiyor) ama uygulama
> geniş çapta sahada denenmedi ve torrent desteği henüz eklenmedi. İlerlemeyi
> [`docs/tasks.md`](docs/tasks.md) üzerinden takip edebilirsiniz.

**[muiget sayfası](https://heraklessii.github.io/Muiget/)** ·
**[sürümler](https://github.com/heraklessii/Muiget/releases)**

---

## Ne Yapar

- **Çoklu bağlantılı indirme** — Dosya HTTP Range istekleriyle parçalara bölünüp
  (varsayılan 8) paralel indirilir. Tek bağlantıya göre belirgin hız artışı.
- **Kesintiye dayanıklı** — Uygulama kapansa, bilgisayar çökse bile indirme
  kaldığı yerden devam eder. Yarım indirmeler açılışta listeye geri gelir.
- **Kuyruk** — Aynı anda kaç indirmenin çalışacağını siz belirlersiniz
  (varsayılan 3); fazlası sıraya girer. Hepsini birden başlatmak toplam süreyi
  kısaltmaz, yalnızca ilk dosyanın bitişini geciktirir.
- **Yolunuzdan çekilen arayüz** — Listede arama ve sıralama, satır sağ tık
  menüsü, sürükle-bırakla bağlantı ekleme, tümünü duraklat / sürdür, klavye
  kısayolları (Ctrl+N yeni indirme, Ctrl+F arama, Ctrl+, ayarlar) ve pencere
  kapalıyken işletim sistemi bildirimi.
- **Toplu ekleme** — Kutuya birden çok adresi bir arada yapıştırın, hepsi
  kuyruğa girsin.
- **Kategori klasörleri** — İnen dosya türüne göre `Video`, `Müzik`,
  `Belgeler`, `Arşivler`, `Programlar`, `Resimler` alt klasörlerine ayrılır.
  Varsayılan kapalı; ayarlardan tek anahtarla açılır.
- **Adaptif** — Bir parça bitince en yavaş parçanın kalanı ikiye bölünüp
  devralınır; hız sınırı ve saat bazlı kurallar (gece sınırsız, gündüz 2 MB/s).
  Aynı siteden birden çok indirme varsa bağlantı kotası aralarında adil
  bölüşülür — hiçbiri sıfır byte'ta beklemez.
- **Video akışı indirme (HLS/DASH)** — Sayfadaki oynatıcı videoyu tek dosya
  yerine yüzlerce parça hâlinde alıyorsa (`.m3u8`, `.mpd`) Muiget parçaları
  paralel indirip tek dosyada birleştirir. Kalite ve ses dili indirmeden önce
  seçilir; parçalar paralel iner ama **sırayla** yazılır, indirme duraklatılıp
  sürdürülebilir. HLS'in `AES-128` parça şifrelemesi çözülür.
  [ffmpeg](https://ffmpeg.org/) **isteğe bağlı**: yalnızca `.ts` → `.mp4`
  dönüşümü ve ayrı inen sesin görüntüyle birleştirilmesi için gerekiyor;
  kurulu değilse video yine iniyor, gerekli olduğu durumda da indirme
  başlamadan söyleniyor.
- **Altyazı** — Yayında altyazı varsa videonun yanına `film.tr.vtt` olarak
  iner. Parçalar uç uca eklenmez, cue düzeyinde birleştirilir ve zaman ekseni
  hizalanır — aksi hâlde çoğu oynatıcı dosyayı hiç açmaz. Dil tercihine uyan
  altyazı seçilir; Ayarlar'dan "hepsi" ya da "indirme" yapılabilir. Altyazının
  inmemesi videoyu hiçbir zaman düşürmez.
- **Tarayıcı entegrasyonu** — Chrome, Edge ve Firefox uzantısı ile sağ tık →
  "Muiget ile indir",
  sayfa taraması, **video yakalama** (sayfadaki HLS/DASH yayınlarını bulur;
  isteğe bağlı izin, varsayılan kapalı) ve isteğe bağlı indirme devralma
  ([kurulum](extension/README.md)).
- **Pano izleme** — Kopyaladığınız adres indirilebilir bir dosyaya işaret
  ediyorsa Muiget sorar; uzantı kurmadan da çalışır. Varsayılan **kapalı**:
  panoyu okumak sessizce açılacak bir yetenek değil.
- **Vekil sunucu** — `http`, `https` ve `socks5` proxy desteği. Kurumsal ağ
  arkasında da çalışır.
- **Korumalı bağlantılar** — `https://kullanıcı:parola@site/dosya.zip` biçimi
  desteklenir; parola listede ve kayıt dosyalarında **saklanmaz**, isteğe
  `Authorization` başlığı olarak taşınır.
- **Checksum** — İnen dosyanın SHA-256/MD5 özetini sağ tık menüsünden
  hesaplayıp sitedeki değerle karşılaştırın. Otomatik değil: büyük dosyada
  diski baştan sona bir kez daha okumak, istemeyen herkese ödetilecek bir
  bedel değil.
- **Kopya uyarısı** — Aynı adresi ikinci kez eklerseniz söyler, ama engellemez.
- **Torrent** — Magnet link ve `.torrent` desteği (librqbit motoru).
  *Henüz eklenmedi, bkz. yol haritası.*
- **Küçük** — Tauri v2 + Rust. Windows x64 kurulum paketi **3,4 MB**
  (NSIS; MSI 4,9 MB), kurulu uygulama 14,1 MB. Electron tabanlı bir indirme
  yöneticisi tipik olarak bunun on katından fazlasını kaplıyor.
- **Telemetri yok** — Hiçbir veri toplanmaz. Uygulamanın kendiliğinden yaptığı
  tek dış istek, açılışta GitHub'daki son sürüm numarasına bakmak; o da
  ayarlardan kapatılabiliyor ve hiçbir kullanıcı verisi taşımıyor.

## Ne Yapmaz

Bu net bir sınırdır ve değişmeyecektir:

- ❌ Sitelerin indirme limitlerini, kotalarını veya hız sınırlarını **aşmaz**.
- ❌ Rapidgator, Mega gibi premium link servislerinin kısıtlarını **atlatmaz**.
- ⚠️ **Bazı siteler indirmeyi kullanım şartlarında yasaklıyor** — YouTube başta.
  Muiget burada teknik bir koruma aşmıyor: imzalı adresi tarayıcının kendisi
  üretip zaten istiyor, uzantı yalnızca o adresi görüyor. Şifre çözme, imza
  kırma yok. Ama **indirmenin kendisi** o sitelerin şartlarına aykırı olabilir
  ve sorumluluk kullanıcıda. Bu yüzden YouTube yakalama ayrı bir izin arkasında,
  varsayılan kapalı, ve Chrome Web Store'a gidecek derlemede hiç yok
  (bkz. [karar #27](docs/decisions.md)).
- ❌ **DRM korumalı video indirmez.** HLS'in `AES-128` parça şifrelemesi
  destekleniyor — anahtar manifestin gösterdiği adresten herkese açık veriliyor
  ve tarayıcıdaki oynatıcı da aynısını yapıyor. Buna karşılık `SAMPLE-AES`
  (FairPlay), Widevine ve PlayReady korumalı içerik açıkça reddedilir; oradaki
  anahtar bir lisans sunucusundan alınıyor ve onu aşmak bu sınırın dışı olurdu.
- ❌ Canlı yayın kaydetmez.

Buradaki "hızlı indirme" yalnızca meşru HTTP seviyesinde optimizasyon demektir:
paralel Range istekleri, resume ve bant genişliği yönetimi. Yukarıdaki ❌
maddeleri değişmeyecek; şifreleme kırmayı, kota aşmayı ya da ödeme duvarı
atlatmayı hedefleyen özellik talepleri kabul edilmez.

## Ekran Görüntüsü

![Muiget penceresi: üç eşzamanlı indirme, segment şeritleri ve durum çubuğu](docs/ekran-goruntusu.png)

_Uygulamanın kendi penceresinden alındı; yerel bir test sunucusundan üç
eşzamanlı indirme._

## Kurulum

Hazır paketler: [Sürümler](https://github.com/heraklessii/Muiget/releases)

| Platform | Dosya |
|---|---|
| Windows x64 | `Muiget_x.y.z_x64-setup.exe` (NSIS) veya `.msi` |
| Linux x64 | `.AppImage` (kurulum gerektirmez) veya `.deb` |
| macOS | `.dmg` (universal — Apple Silicon + Intel) |

**Linux ve macOS paketleri derleniyor ama geliştirici tarafından denenmedi** —
proje Windows'ta geliştiriliyor. O platformlarda karşılaştığınız sorunları
issue olarak bildirin.

Paketler **imzasız**: Windows SmartScreen ve macOS Gatekeeper uyarı gösterecek.
Kod imzalama sertifikası henüz yok.

### Tarayıcı uzantısı

Uzantı henüz mağazalarda değil. İki yoldan da kurulabilir — hazır paketler
depoda [`dist-extension/`](dist-extension) altında duruyor ve her sürümde
zip olarak yayına da ekleniyor:

| Tarayıcı | Adımlar |
|---|---|
| Chrome / Edge | `chrome://extensions` (Edge: `edge://extensions`) → **Geliştirici modu** → **Paketlenmemiş öğe yükle** → `dist-extension/chrome` |
| Firefox | `about:debugging#/runtime/this-firefox` → **Geçici Eklenti Yükle** → `dist-extension/firefox/manifest.json` |

Sonra masaüstü uygulamasında **Ayarlar → Tarayıcı uzantısı**: Chrome/Edge
kullanıyorsanız uzantı kartındaki 32 harflik kimliği yapıştırın, yalnızca
Firefox kullanıyorsanız kutuyu boş bırakın → **Köprüyü kur**.

Ayrıntılar ve izin açıklamaları: [`extension/README.md`](extension/README.md).

> Firefox'ta geçici eklenti tarayıcı kapanınca kalkar (kalıcı kurulum imzalama
> istiyor). Firefox tarafı henüz sahada denenmedi.

### Gereksinimler

| Araç | Sürüm | Not |
|---|---|---|
| Rust | 1.77+ | [rustup.rs](https://rustup.rs) üzerinden |
| Node.js | 20+ | Frontend derlemesi için |
| Tauri CLI | v2 | Ayrıca kurmaya gerek yok — `npm install` ile geliyor |

Platforma özel Tauri bağımlılıkları (Windows'ta WebView2, Linux'ta
`webkit2gtk` vb.) için: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

### Derleme

```bash
git clone https://github.com/heraklessii/Muiget.git
cd Muiget
npm install
npm run tauri dev    # geliştirme modu
npm run tauri build  # üretim binary'si
```

## Teknoloji Yığını

| Katman | Teknoloji |
|---|---|
| Masaüstü kabuk | Tauri v2 |
| Backend | Rust + Tokio |
| HTTP istemci | reqwest |
| Akış videosu | Kendi kodumuz (m3u8/MPD) + ffmpeg (isteğe bağlı) |
| Torrent motoru | librqbit |
| Frontend | React + Vite + TypeScript |
| Tarayıcı uzantısı | MV3 (Chrome/Edge + Firefox) + Native Messaging |

Bu seçimlerin gerekçeleri için: [`docs/decisions.md`](docs/decisions.md).

## Yol Haritası

| Faz | İçerik | Durum |
|---|---|---|
| 0 | Dokümantasyon + proje iskeleti | ✅ Tamamlandı |
| 1 | Segmentasyon motoru (HTTP Range, resume) | ✅ Tamamlandı |
| 2 | Adaptif optimizasyon, bant genişliği kuralları | ✅ Tamamlandı |
| 3 | Tauri UI (React) | ✅ Tamamlandı |
| 4 | Torrent entegrasyonu | ⚪ Ertelendi |
| 5 | Tarayıcı uzantısı (Chrome/Edge/Firefox) | ✅ Tamamlandı |
| 6 | HLS/DASH video, altyazı, checksum, opsiyonel virüs taraması | 🟡 Video, altyazı ve checksum bitti |
| 7 | Plugin sistemi, istatistikler, katkı rehberi | ⚪ Bekliyor |

**Bilinen eksikler:** torrent desteği henüz yok; HLS/DASH indirme gerçek bir
video sitesine karşı denenmedi (testler yerel sunucuya karşı); uzantı henüz
mağazalarda değil ve Firefox tarafı sahada denenmedi; açılışta yalnızca
varsayılan indirme klasörü taranıyor
(başka klasörler için Ayarlar → "Klasörü tara"); hız, gerçek dünyada henüz
başka bir indirme yöneticisiyle karşılaştırılmadı; kurulum paketleri imzasız.

Detaylı görev listesi: [`docs/tasks.md`](docs/tasks.md).

## Katkı

Proje şu an tek geliştiricili ve erken aşamada. Katkı rehberi
(`CONTRIBUTING.md`) Faz 7'de yazılacak. O zamana kadar issue açarak fikir ve
hata bildirimi yapabilirsiniz.

Katkı verirken okunması gereken dosyalar:

1. [`CLAUDE.md`](CLAUDE.md) — proje bağlamı ve çalışma tarzı
2. [`docs/project_overview.md`](docs/project_overview.md) — ürün vizyonu
3. [`docs/decisions.md`](docs/decisions.md) — mimari kararlar ve gerekçeleri
4. [`docs/tasks.md`](docs/tasks.md) — güncel görev listesi

## Lisans

[Apache License 2.0](LICENSE) — hem masaüstü uygulaması hem tarayıcı uzantısı
için.

Üçüncü parti bileşenlerin lisans bildirimleri: [`NOTICE`](NOTICE).
