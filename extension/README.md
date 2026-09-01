# Muiget Tarayıcı Uzantısı

Bağlantıları masaüstündeki Muiget uygulamasına gönderir. Uzantı kendi başına
indirme yapmaz, ağa çıkmaz ve hiçbir sunucuya bağlanmaz — tek dış teması
`com.muiget.host` adlı yerel native messaging köprüsüdür.

Chrome, Edge ve Firefox destekleniyor. Kaynak tek: bu klasör. Paketler
`tools/uzanti-paketle.js` ile üretiliyor, çünkü Firefox manifesti üç noktada
Chrome'unkinden ayrılıyor (karar #31).

Üretilmiş paketler depoda: `dist-extension/chrome` ve
`dist-extension/firefox`. Yani uzantıyı kurmak için hiçbir şey derlemeniz
gerekmiyor — klasörü seçmeniz yetiyor. Paketler her sürümde yayına zip olarak
da ekleniyor ve güncelliklerini CI kontrol ediyor.

## Paketleri üret

```bash
npm run uzanti          # dist-extension/chrome ve dist-extension/firefox
npm run uzanti:magaza   # mağaza derlemesi: YouTube yakalaması kapalı (karar #27)
```

Chrome paketi kaynağın birebir kopyası; Firefox paketinde manifest
dönüştürülüyor (olay sayfası, `browser_specific_settings`).

## Kurulum

Uzantı henüz mağazalarda değil; geliştirici modunda yükleniyor.

### 1. Uzantıyı yükle

**Chrome / Edge**

1. `chrome://extensions` (Edge'de `edge://extensions`) adresini aç
2. Sağ üstten **Geliştirici modu**'nu aç
3. **Paketlenmemiş öğe yükle** → `dist-extension/chrome` klasörünü seç
4. Kartta görünen **kimliği (ID)** kopyala — 32 harflik bir dize

**Firefox**

1. `about:debugging#/runtime/this-firefox` adresini aç
2. **Geçici Eklenti Yükle** → `dist-extension/firefox/manifest.json` dosyasını seç
3. Kimlik kopyalamaya gerek yok: Firefox kimliği manifestte sabit
   (`muiget@muiget.app`) ve köprü onu kendiliğinden tanıyor

> Geçici eklenti Firefox kapanınca kalkar. Kalıcı kurulum imzalama gerektiriyor
> (`about:config` → `xpinstall.signatures.required` yalnızca Developer/Nightly
> sürümlerde kapatılabiliyor).

### 2. Köprüyü tanıt

Masaüstü uygulamasını aç → **Ayarlar → Tarayıcı uzantısı**:

- Chrome/Edge kullanıyorsan kopyaladığın kimliği yapıştır
- Yalnızca Firefox kullanıyorsan kutuyu **boş bırak**

→ **Köprüyü kur**.

Bu adım iki manifest yazıyor ve Windows'ta kullanıcı kapsamındaki (`HKCU`)
registry anahtarlarını oluşturuyor:

| Tarayıcı | Manifest | Registry anahtarı |
|---|---|---|
| Chrome | `com.muiget.host.json` | `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.muiget.host` |
| Edge | (aynı dosya) | `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.muiget.host` |
| Firefox | `com.muiget.host.firefox.json` | `HKCU\Software\Mozilla\NativeMessagingHosts\com.muiget.host` |

İki manifest gerekiyor çünkü izin listesinin alan adı farklı: Chromium
`allowed_origins` içinde `chrome-extension://<kimlik>/`, Firefox
`allowed_extensions` içinde çıplak kimlik bekliyor.

Kimliğin manifeste yazılması bir **güvenlik sınırı**: yalnızca listedeki uzantı
köprüyü başlatabiliyor. Bir uyarı: Chrome kimliği uzantının açık anahtarından
türediği için sahtelenemiyor, Firefox kimliği ise uzantının kendi beyanı —
geçici olarak yüklenmiş başka bir eklenti aynı kimliği yazabilir. Köprünün
kapısı yine de dar: yalnızca `http(s)` adresleri geçiyor.

### 3. Doğrula

Uzantı simgesine tıkla. Sağ üstte **bağlı · v0.1.5** yazıyorsa köprü çalışıyor
(numara uygulamanın sürümü — köprü onu bildiriyor, uzantınınkini değil).

## Kullanım

| Yol | Nasıl |
|---|---|
| Sağ tık | Bir bağlantıya/videoya/resme sağ tıkla → **Muiget ile indir** |
| Sayfa taraması | Uzantı simgesine tıkla → sayfadaki dosyalar listelenir |
| Devralma | Popup → **İndirmeleri devral** → tarayıcının başlattığı indirmeler Muiget'e geçer |
| Video yakalama | Popup → **Video yakala** → sayfayı yenile → HLS/DASH yayınları popup'ta listelenir |

## İzinler ve gizlilik

Kurulumda istenen izinler dar tutuldu; hassas olanlar **isteğe bağlı** ve
varsayılan olarak kapalı:

| İzin | Ne zaman | Niçin |
|---|---|---|
| `nativeMessaging` | Her zaman | Masaüstü uygulamasıyla konuşmak |
| `contextMenus` | Her zaman | Sağ tık menüsü |
| `activeTab` + `scripting` | Popup açılınca | Sayfayı **yalnızca o an** tarar; kalıcı content script yok |
| `downloads` | Kullanıcı devralmayı açınca | Tarayıcı indirmesini iptal edip Muiget'e aktarmak |
| `cookies` + `<all_urls>` | Kullanıcı çerez gönderimini açınca | Giriş gerektiren dosyalar için `Cookie` başlığı |
| `webRequest` + `<all_urls>` | Kullanıcı video yakalamayı açınca | Sayfadaki HLS/DASH manifest adreslerini görmek |

Çerez gönderimi kapalı gelir ve açıkça açılması gerekir: çerez oturum kimliği
taşır ve varsayılan olarak uygulama dışına vermek doğru değildir.

**Video yakalama** da kapalı gelir. Açıldığında uzantı gezdiğiniz sayfaların ağ
isteklerinin **adreslerini** görür; bu ağır bir yetki ve kurulumda sessizce
istenmemeli. Görülen adreslerin yalnızca `.m3u8`/`.mpd` olanları saklanır,
onlar da sekme başına `storage.session`de tutulur: diske yazılmaz, sayfa
değişince ve tarayıcı kapanınca silinir, hiçbir yere gönderilmez.

Neden ayrı bir yakalayıcı gerekiyor: HLS/DASH manifestinin adresi sayfanın
HTML'inde geçmiyor, oynatıcı JavaScript'i çalışırken isteniyor. Popup'ın DOM
taraması bu videoları tanım gereği bulamaz.

## Sınırlar

- `blob:` ve `data:` adresleri devralınmaz — bu veriler sayfanın belleğinde
  durur, Muiget onlara erişemez.
- Devralma başarısız olursa tarayıcının indirmesine **dokunulmaz**; kullanıcı
  dosyayı yine de alır. Sessiz veri kaybı en kötü sonuç olurdu.
- MV3'te bir istek başlamadan engellenemiyor (`webRequest` engelleme kaldırıldı),
  bu yüzden devralma indirme başladıktan hemen sonra gerçekleşir.
- Firefox tarafı **sahada denenmedi**: manifest dönüşümü ve köprü kaydı yazıldı
  ve testli, ama gerçek bir Firefox kurulumunda çalıştırılmadı.

## Geliştirme

Derleme adımı yok — düz JavaScript. `background.js` bilerek modül **değil**:
aynı dosya Chrome'da servis çalışanı, Firefox'ta olay sayfası olarak yükleniyor
ve olay sayfası bağlamında `export` sözdizimi hatası olurdu.

Tarayıcı API'sine `api` üzerinden erişiliyor
(`globalThis.browser ?? globalThis.chrome`): Firefox'ta Promise döndüren ad
`browser`, Chrome/Edge'de `chrome`.

Dosyayı değiştirdikten sonra paketi yeniden üret (`npm run uzanti`) ve
uzantının yenile düğmesine bas. Arka planın log'ları: Chrome'da uzantı
kartındaki **service worker** bağlantısı, Firefox'ta
`about:debugging` → **İncele**.

## Lisans

Apache 2.0 — ana proje ile aynı. Bkz. depo kökündeki `LICENSE`.
