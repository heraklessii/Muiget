# CLAUDE.md — Muiget Proje Rehberi

Bu dosya, Claude Code (veya başka bir oturum) projeye her girdiğinde ilk okuması
gereken bağlam dosyasıdır. Amaç: sıfırdan bağlam kurmadan kaldığın yerden devam
edebilmek.

## Proje Nedir

**Muiget**, açık kaynaklı (Apache 2.0), IDM/FDM tarzı bir indirme yöneticisi.
HTTP/HTTPS çoklu-bağlantılı (segmented) indirme, torrent desteği ve bir Chrome
uzantısı ile tarayıcıdan yakalama içerecek. Hedef: IDM'e gerçek, ücretsiz,
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
| Frontend UI | React + Vite + TypeScript | Tauri ile birinci sınıf entegrasyon |
| Extension | Chrome MV3 | İlker'in Muitoon extension deneyimiyle örtüşüyor |
| Extension ↔ App köprüsü | Native Messaging (stdin/stdout, length-prefixed JSON) | Chrome'un desteklediği tek güvenli yerel IPC yolu |

## Lisans

Apache License 2.0 — hem ana uygulama hem Chrome extension için. Her yeni
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
├── tools/                      ✅ ikon-uret.js (uygulama ikonu üretici)
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
│   │                              SpeedGraph, Toasts, Icons
│   ├── hooks/                  ✅ useDownloads.ts (ilerleme yayınına abonelik),
│   │                              useHotkeys.ts (klavye kısayolları)
│   └── lib/                    ✅ api.ts (invoke sarmalayıcıları), types.ts,
│                                  format.ts, notify.ts (OS bildirimi)
├── src-tauri/                  ✅ Rust backend
│   ├── Cargo.toml              ✅ crate: muiget, lib: muiget_lib
│   ├── tauri.conf.json         ✅ identifier: com.muiget.app
│   ├── capabilities/           ✅ Dar izin listesi
│   ├── icons/                  ✅ (varsayılan Tauri ikonları — değiştirilecek)
│   ├── tests/                  ✅ indirme_uctan_uca.rs (yerel HTTP sunucusu)
│   └── src/
│       ├── main.rs             ✅ --native-host kipi ayrımı
│       ├── lib.rs              ✅ Tauri kurulumu, tepsi, tek örnek
│       ├── commands.rs         ✅ Frontend'e açılan komutlar
│       ├── settings.rs         ✅ settings.json, normalize()
│       ├── download/           ✅ Faz 1–2
│       │   ├── mod.rs             # Hata tipleri
│       │   ├── http.rs            # probe(): sunucu yetenekleri, dosya adı
│       │   ├── segmenter.rs       # Range parçalarına bölme planı (saf)
│       │   ├── writer.rs          # Sparse file, seek+write
│       │   ├── resume.rs          # .muiget meta, tazelik kontrolü
│       │   ├── worker.rs          # Segment task'ı, retry + backoff
│       │   ├── speed.rs           # EWMA hız ölçümü
│       │   ├── throttle.rs        # Token bucket, host kotası, zaman kuralları
│       │   └── manager.rs         # Orkestrasyon + adaptif bölme
│       ├── extension_bridge/   ✅ Faz 5
│       │   ├── mod.rs             # İstek işleme, host kaydı
│       │   └── native_host.rs     # Chrome native messaging protokolü
│       └── torrent/            ⬜ Faz 4
│           └── engine.rs          # librqbit sarmalayıcı
└── extension/                  ✅ Faz 5 — Chrome MV3 uzantısı
    ├── manifest.json           ✅
    ├── background.js           ✅ Sağ tık, köprü, indirme devralma
    ├── popup.html/.js/.css     ✅ Sayfa taraması, ayarlar
    ├── icons/                  ✅
    └── README.md               ✅ Kurulum ve izin açıklamaları
```

## Geliştirme Komutları

```bash
npm run dev       # sadece frontend (Vite, localhost:1420)
npm run build     # tsc + vite build → dist/
npm run tauri dev # tam uygulama (Rust + pencere)
cargo check       # src-tauri/ içinde, hızlı Rust doğrulaması
cargo test        # 130 birim + 13 uçtan uca test
```

Uçtan uca testler (`src-tauri/tests/indirme_uctan_uca.rs`) elle yazılmış küçük
bir HTTP sunucusu kaldırıp motoru ona karşı çalıştırıyor. `Range` davranışını
teste göre değiştirebilmek gerektiği için hazır bir test sunucusu kullanılmadı.

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
7. **Torrent**: librqbit `Session` API'si üzerinden magnet/`.torrent` desteği.
   Sequential download modu (streaming izleme) ileride eklenecek.

## Çalışma Tarzı Notları

- İlker doğrudan ve gayri resmi iletişim kuruyor, hızlı aksiyon tercih ediyor.
- Uzun açıklamadan çok somut adım/kod istiyor — ama bu doküman aşamasında
  sadece MD dosyaları isteniyor, kod yazımı bilinçli olarak erteleniyor.
- Her yeni oturumda önce `docs/worklog.md`'nin son girdisini, sonra
  `docs/tasks.md`'nin "Sıradaki" bölümünü oku.

## Sıradaki Adım

**Faz 0, 1, 2, 3 ve 5 tamamlandı.** Çalışan bir segmentli indirme motoru, tam
bir arayüz ve Chrome uzantısı var. 143 test geçiyor. Uygulama gerçek
penceresinde uçtan uca doğrulandı (8 MB dosya, 8 paralel segment, SHA-256
birebir aynı) ve v0.1.0 ön sürümü yayınlandı.

Ayrıca teknik borcun üç maddesi kapandı: indirme listesi oturumlar arası
korunuyor (açılışta `.muiget` taraması), uzantıdan gelen başlıklar metaya
yazılıyor, eşzamanlı indirme sayısı sınırlanabiliyor.

Arayüz tarafında Faz 3'ün açık maddeleri de bitti: listede arama/sıralama,
klavye kısayolları, işletim sistemi bildirimi ve toplu duraklat/sürdür.

**Faz 4 (torrent) bilinçli olarak ertelendi:** librqbit ~100 yeni bağımlılık
getiriyor ve gerçek bir swarm'a karşı denenmeden doğru çalıştığı söylenemez.
HTTP motorunu yerel bir sunucuyla test edebiliyoruz; torrent'te "derleniyor"
yeterli kanıt değil.

Sıradaki öncelikler (`docs/tasks.md` → "Sıradaki"):

1. **Gerçek dünya doğrulaması** — Faz 1'in IDM hız karşılaştırması ve Faz 5'in
   gerçek Chrome ile köprü denemesi. İkisi de "yazıldı, sahada denenmedi".
   Kod işi değil; İlker'in makinesinde denenmesi gerekiyor.
2. **Faz 4 (torrent)** — yukarıdaki bitmeden başlanmamalı.
3. **Kalan arayüz işleri** — sürükle-bırak ile bağlantı ekleme, satır sağ tık
   menüsü. Küçük ve bağımsız.

**İlker'e kalan (kod dışı):** GitHub reposunun oluşturulması ve ilk commit.
Repoda hâlâ hiç commit yok, remote tanımlı değil.

Her yeni oturum şu sırayla okunmalı:
1. Bu dosya (`CLAUDE.md`) — genel bağlam
2. `docs/worklog.md` — en üstteki (en son) girdi
3. `docs/tasks.md` — "Sıradaki" bölümü
4. Gerekirse `docs/decisions.md` — ilgili kararın numarasına bak
