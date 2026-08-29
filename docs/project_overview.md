# Proje Genel Bakış — Muiget

## Vizyon

IDM (Internet Download Manager) ve FDM (Free Download Manager) tarzında,
ama **tamamen açık kaynak (Apache 2.0)** bir indirme yöneticisi. Kapalı kaynak
ve ücretli modele karşı şeffaf, herkesin katkı verebileceği bir alternatif.

Tek cümle: *"IDM'in yaptığını yapan, ama kimsenin lisans anahtarı satın alması
gerekmeyen, kodunu herkesin okuyup değiştirebildiği bir araç."*

## Hedef Kitle

- Büyük dosya indiren (video, yazılım, arşiv) her gün onlarca indirme yapan kullanıcılar
- IDM'in ücretli/kırık lisans modelinden rahatsız olan kullanıcılar
- Açık kaynak araçları tercih eden geliştirici/power-user kitlesi
- Torrent + HTTP indirmeyi tek bir arayüzde toplamak isteyenler

## Rakip Analizi (Kısa)

| Araç | Artı | Eksi |
|---|---|---|
| IDM | Hızlı, olgun, tarayıcı entegrasyonu iyi | Kapalı kaynak, ücretli, Windows-only |
| FDM | Ücretsiz, torrent desteği var | Geliştirme temposu düşük, UI eski |
| aria2 | Çok güçlü, script edilebilir | CLI-first, sıradan kullanıcıya uzak, GUI'si zayıf |
| yt-dlp | Video indirmede efsane | Genel amaçlı dosya indirici değil, GUI yok |

**Muiget'in farkı**: Modern stack (Tauri/Rust — düşük kaynak tüketimi),
Apache 2.0 ile tam şeffaflık, hem HTTP hem torrent tek çatı altında, ve
topluluk plugin sistemiyle site-özel kurallar eklenebilir olması.

## Çekirdek Değer Önermesi

1. **Hız**: Çoklu bağlantı segmentasyonu (IDM ile aynı prensip, meşru HTTP
   optimizasyonu — limit aşma değil)
2. **Güvenilirlik**: Kesintide otomatik resume, adaptif segment yeniden dağıtımı
3. **Genişlik**: HTTP/HTTPS + torrent + (ileride) HLS/DASH stream indirme
4. **Şeffaflık**: Apache 2.0, telemetri yok, kod herkese açık
5. **Genişletilebilirlik**: Plugin sistemi ile topluluk site-özel handler ekleyebilir

## Net Olmayan / Asla Yapılmayacak Şeyler

- ❌ Premium link servislerinin (Rapidgator, Mega, vb.) hız/kota limitini aşma
- ❌ Herhangi bir sitenin ToS'unu ihlal eden "bypass" özelliği
- ❌ Kullanıcı verisi toplayan telemetri/analitik (varsayılan kapalı, opt-in bile
  olsa tamamen lokal olmayan hiçbir veri toplama yok)
- ❌ Reklam veya "premium sürüm" modeli — proje tamamen ücretsiz kalacak

## Ana Özellik Grupları

### 1. Çekirdek İndirme Motoru
- Segmented/paralel HTTP indirme (varsayılan 8 parça, adaptif)
- Pause/resume (uygulama kapansa bile)
- Bant genişliği sınırlama + zaman bazlı kurallar (ör. gece sınırsız, gündüz 2MB/s)
- Checksum doğrulama (MD5/SHA256)
- Otomatik yeniden deneme + üstel geri çekilme

### 2. Torrent Desteği
- librqbit tabanlı magnet/`.torrent` indirme
- Sequential download modu (izlerken/dinlerken indirme)
- Seed oranı/süresi ayarları

### 3. Chrome Extension
- Sayfa tarama ile indirilebilir medya tespiti (video/zip/exe vb.)
- Sağ tık context menu entegrasyonu
- Native messaging ile masaüstü uygulamaya köprü
- Opsiyonel clipboard link algılama

### 4. Medya Özel Yetenekler
- HLS/DASH stream indirme (m3u8 → mp4, ffmpeg entegrasyonu)
- yt-dlp entegrasyonu (video siteleri)

### 5. Kullanıcı Deneyimi
- Kategori bazlı otomatik klasörleme
- Sistem tepsisinde canlı hız grafiği
- Duplicate indirme tespiti
- Lokal istatistik dashboard'u (toplam veri, yoğun saatler — tamamen offline)

### 6. Topluluk / Genişletilebilirlik
- Plugin sistemi: site-özel indirme kuralları topluluk tarafından eklenebilir
  (İlker'in Muitoon extension'ındaki `site-comix.js`, `site-webtoons.js` gibi
  per-site handler mantığına benzer, ama üçüncü taraflar için açık)

### 7. Güven / Güvenlik
- İndirilen dosya için hash gösterimi (UI'da görünür checksum)
- Opsiyonel virus tarama tetikleme (Windows Defender API / VirusTotal — kullanıcı
  onayına bağlı, varsayılan kapalı)

## Başarı Kriterleri (İlk Sürüm İçin)

- [ ] Bir HTTP linkini IDM'den daha yavaş olmayan hızda indirebilmek
- [ ] Kesinti sonrası %100 güvenilir resume
- [ ] En az bir torrent'i (magnet link) sorunsuz indirebilmek
- [ ] Chrome extension'dan sağ-tık ile bir dosyayı masaüstü uygulamasına gönderebilmek
- [ ] Apache 2.0 LICENSE + NOTICE dosyalarının eksiksiz ve doğru olması
