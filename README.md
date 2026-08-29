# Muiget

**Açık kaynak indirme yöneticisi.** IDM'in yaptığını yapan, ama kimsenin lisans
anahtarı satın alması gerekmeyen, kodunu herkesin okuyup değiştirebildiği bir araç.

[![CI](https://github.com/heraklessii/Muiget/actions/workflows/ci.yml/badge.svg)](https://github.com/heraklessii/Muiget/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/durum-geli%C5%9Ftirme%20a%C5%9Famas%C4%B1nda-orange.svg)](docs/tasks.md)

> ⚠️ **Bu proje erken geliştirme aşamasında.** İndirme motoru, arayüz ve Chrome
> uzantısı çalışıyor (158 test geçiyor) ama uygulama geniş çapta sahada
> denenmedi ve torrent desteği henüz eklenmedi. İlerlemeyi
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
- **Tarayıcı entegrasyonu** — Chrome uzantısı ile sağ tık → "Muiget ile indir",
  sayfa taraması ve isteğe bağlı indirme devralma
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
- ❌ Herhangi bir sitenin kullanım şartlarını ihlal eden "bypass" özelliği
  içermez.

Buradaki "hızlı indirme" yalnızca meşru HTTP seviyesinde optimizasyon demektir:
paralel Range istekleri, resume ve bant genişliği yönetimi. Bu sınırı aşmayı
hedefleyen özellik talepleri kabul edilmez.

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
| Torrent motoru | librqbit |
| Frontend | React + Vite + TypeScript |
| Tarayıcı uzantısı | Chrome MV3 + Native Messaging |

Bu seçimlerin gerekçeleri için: [`docs/decisions.md`](docs/decisions.md).

## Yol Haritası

| Faz | İçerik | Durum |
|---|---|---|
| 0 | Dokümantasyon + proje iskeleti | ✅ Tamamlandı |
| 1 | Segmentasyon motoru (HTTP Range, resume) | ✅ Tamamlandı |
| 2 | Adaptif optimizasyon, bant genişliği kuralları | ✅ Tamamlandı |
| 3 | Tauri UI (React) | ✅ Tamamlandı |
| 4 | Torrent entegrasyonu | ⚪ Ertelendi |
| 5 | Chrome uzantısı | ✅ Tamamlandı |
| 6 | HLS/DASH, checksum, opsiyonel virüs taraması | 🟡 Checksum bitti |
| 7 | Plugin sistemi, istatistikler, katkı rehberi | ⚪ Bekliyor |

**Bilinen eksikler:** torrent desteği henüz yok; video sitelerinden HLS/DASH
(m3u8) akışı indirilemiyor; yalnızca Chrome uzantısı var (Firefox/Edge yok ve
uzantı mağazada değil); açılışta yalnızca varsayılan indirme klasörü taranıyor
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

[Apache License 2.0](LICENSE) — hem masaüstü uygulaması hem Chrome uzantısı için.

Üçüncü parti bileşenlerin lisans bildirimleri: [`NOTICE`](NOTICE).
