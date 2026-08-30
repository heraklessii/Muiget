# Muiget Chrome Uzantısı

Bağlantıları masaüstündeki Muiget uygulamasına gönderir. Uzantı kendi başına
indirme yapmaz, ağa çıkmaz ve hiçbir sunucuya bağlanmaz — tek dış teması
`com.muiget.host` adlı yerel native messaging köprüsüdür.

## Kurulum

Uzantı henüz Chrome Web Store'da değil; geliştirici modunda yükleniyor.

### 1. Uzantıyı yükle

1. Chrome'da `chrome://extensions` adresini aç
2. Sağ üstten **Geliştirici modu**'nu aç
3. **Paketlenmemiş öğe yükle** → bu `extension/` klasörünü seç
4. Kartta görünen **kimliği (ID)** kopyala — 32 harflik bir dize

### 2. Köprüyü tanıt

Masaüstü uygulamasını aç → **Ayarlar → Tarayıcı uzantısı** → kopyaladığın
kimliği yapıştır → **Köprüyü kur**.

Bu adım iki şey yapıyor:
- `com.muiget.host.json` manifest dosyasını yazıyor
- Windows'ta `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.muiget.host`
  registry anahtarını oluşturuyor (Edge için de aynısı)

Kimliğin manifeste yazılması bir **güvenlik sınırı**: yalnızca listedeki uzantı
köprüyü başlatabiliyor.

### 3. Doğrula

Uzantı simgesine tıkla. Sağ üstte **bağlı · v0.1.0** yazıyorsa köprü çalışıyor.

## Kullanım

| Yol | Nasıl |
|---|---|
| Sağ tık | Bir bağlantıya/videoya/resme sağ tıkla → **Muiget ile indir** |
| Sayfa taraması | Uzantı simgesine tıkla → sayfadaki dosyalar listelenir |
| Devralma | Popup → **İndirmeleri devral** → Chrome'un başlattığı indirmeler Muiget'e geçer |
| Video yakalama | Popup → **Video yakala** → sayfayı yenile → HLS/DASH yayınları popup'ta listelenir |

## İzinler ve gizlilik

Kurulumda istenen izinler dar tutuldu; hassas olanlar **isteğe bağlı** ve
varsayılan olarak kapalı:

| İzin | Ne zaman | Niçin |
|---|---|---|
| `nativeMessaging` | Her zaman | Masaüstü uygulamasıyla konuşmak |
| `contextMenus` | Her zaman | Sağ tık menüsü |
| `activeTab` + `scripting` | Popup açılınca | Sayfayı **yalnızca o an** tarar; kalıcı content script yok |
| `downloads` | Kullanıcı devralmayı açınca | Chrome indirmesini iptal edip Muiget'e aktarmak |
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
- Devralma başarısız olursa Chrome'un indirmesine **dokunulmaz**; kullanıcı
  dosyayı yine de alır. Sessiz veri kaybı en kötü sonuç olurdu.
- MV3'te bir istek başlamadan engellenemiyor (`webRequest` engelleme kaldırıldı),
  bu yüzden devralma indirme başladıktan hemen sonra gerçekleşir.

## Geliştirme

Derleme adımı yok — düz JavaScript, ES modülleri. Dosyayı değiştir,
`chrome://extensions` sayfasında uzantının yenile düğmesine bas.

Arka plan servisinin log'ları: uzantı kartındaki **service worker** bağlantısı.

## Lisans

Apache 2.0 — ana proje ile aynı. Bkz. depo kökündeki `LICENSE`.
