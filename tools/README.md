# tools/

Projeye ait tek seferlik yardımcı betikler. Uygulamanın çalışması için gerekli
değiller; kaynağa dâhil edilmelerinin sebebi üretilmiş dosyaların nasıl
üretildiğinin kaybolmaması.

## `ikon-uret.js`

Uygulama ikonunu üretir. Harici bir görüntü kütüphanesi kullanmıyor — Node'un
kendi `zlib`'i ile PNG elle yazılıyor, kenar yumuşatma 4 kat supersampling ile
yapılıyor. Biçim arayüzdeki `IconDownload` ile aynı: aşağı ok + taban çizgisi,
Mui ailesinin teal vurgusu (`#2dd4bf`) koyu bir zemin üzerinde.

```bash
node tools/ikon-uret.js src-tauri/icons/kaynak-1024.png
npx tauri icon src-tauri/icons/kaynak-1024.png
```

İkinci komut platformların istediği tüm boyutları (`.ico`, `.icns`, Windows
Store logoları) `src-tauri/icons/` altına yazar. Mobil (iOS/Android) klasörleri
bilinçli olarak silindi: proje masaüstü hedefliyor, ihtiyaç olursa aynı komut
yeniden üretir.
