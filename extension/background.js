/**
 * Muiget Chrome uzantısı — arka plan servisi.
 *
 * Uzantının tek işi var: bir bağlantıyı masaüstü uygulamasına iletmek. Kendisi
 * indirme yapmıyor, ağa çıkmıyor, hiçbir sunucuya bağlanmıyor. Tek dış teması
 * `com.muiget.host` native messaging köprüsü.
 *
 * MV3 service worker'ı her an uykuya alınabilir, bu yüzden hiçbir durum bellekte
 * tutulmuyor; ayarlar `chrome.storage.local`'da.
 */

const HOST = 'com.muiget.host';

/** Varsayılan ayarlar. Gizlilik açısından hassas olanlar KAPALI başlıyor. */
const VARSAYILAN_AYARLAR = {
  /** Chrome'un başlattığı indirmeleri devral. */
  interceptDownloads: false,
  /** Oturum çerezlerini Muiget'e gönder (giriş gerektiren dosyalar için). */
  sendCookies: false,
  /** Devralınmayacak alan adları. */
  ignoredHosts: [],
};

export async function ayarlariAl() {
  const kayitli = await chrome.storage.local.get(VARSAYILAN_AYARLAR);
  return { ...VARSAYILAN_AYARLAR, ...kayitli };
}

/* ---------------------------------------------------------------------------
 * Köprü
 * ------------------------------------------------------------------------- */

/**
 * Köprüye tek bir mesaj gönderir.
 *
 * `connectNative` yerine `sendNativeMessage`: köprü süreci durumsuz ve kısa
 * ömürlü (bkz. src-tauri/src/extension_bridge/native_host.rs). Kalıcı bir port
 * açmak, service worker uykuya daldığında kopardı zaten.
 */
function kopruyeGonder(mesaj) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST, mesaj, (yanit) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
        return;
      }
      resolve(yanit);
    });
  });
}

/** Köprü ayakta mı? Popup bağlantı durumunu bununla gösteriyor. */
export async function kopruyuSina() {
  try {
    const yanit = await kopruyeGonder({ type: 'ping' });
    return { ok: yanit?.type === 'pong', version: yanit?.version ?? null };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}

/**
 * Bir indirmeyi Muiget'e gönderir.
 *
 * `Referer` her zaman gönderiliyor (hotlink korumalı siteler için şart), çerez
 * yalnızca kullanıcı açıkça izin verdiyse — çerez oturum kimliği taşır ve
 * varsayılan olarak dışarı vermek doğru değil.
 */
export async function indirmeyiGonder({ url, fileName, referrer }) {
  if (!/^https?:\/\//i.test(url)) {
    throw new Error('Yalnızca http ve https adresleri gönderilebilir');
  }

  const ayarlar = await ayarlariAl();
  const istek = {
    type: 'download',
    url,
    fileName: fileName ?? null,
    referrer: referrer ?? null,
    userAgent: navigator.userAgent,
    cookies: null,
  };

  if (ayarlar.sendCookies) {
    istek.cookies = await cerezBasligi(url);
  }

  const yanit = await kopruyeGonder(istek);
  if (yanit?.type === 'rejected') {
    throw new Error(yanit.reason || 'Muiget isteği reddetti');
  }
  return yanit;
}

/** Adres için `Cookie` başlığı üretir. İzin yoksa sessizce boş döner. */
async function cerezBasligi(url) {
  if (!chrome.cookies) return null;
  try {
    const cerezler = await chrome.cookies.getAll({ url });
    if (!cerezler.length) return null;
    return cerezler.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    // İzin verilmemiş olabilir; çerezsiz denemek hiç denememekten iyi.
    return null;
  }
}

/* ---------------------------------------------------------------------------
 * Sağ tık menüsü
 * ------------------------------------------------------------------------- */

const MENU_ID = 'muiget-indir';

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: 'Muiget ile indir',
      contexts: ['link', 'image', 'video', 'audio'],
    });
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== MENU_ID) return;

  // Bağlam türüne göre doğru adres: bağlantıda linkUrl, medyada srcUrl.
  const url = info.linkUrl || info.srcUrl;
  if (!url) return;

  try {
    await indirmeyiGonder({ url, referrer: info.pageUrl || tab?.url });
    await rozetGoster('✓', '#2dd4bf');
  } catch (e) {
    await rozetGoster('!', '#ff7a6b');
    console.warn('Muiget: indirme gönderilemedi', e);
  }
});

/** Kullanıcıya kısa görsel geri bildirim — bildirim izni istemeden. */
async function rozetGoster(metin, renk) {
  await chrome.action.setBadgeText({ text: metin });
  await chrome.action.setBadgeBackgroundColor({ color: renk });
  setTimeout(() => chrome.action.setBadgeText({ text: '' }), 2500);
}

/* ---------------------------------------------------------------------------
 * İndirme devralma
 *
 * Chrome indirmeyi başlattıktan sonra iptal edip Muiget'e devrediyoruz.
 * MV3'te isteği başlamadan engellemek mümkün değil (webRequest engelleme
 * kaldırıldı), bu yüzden yakalama noktası `onCreated`.
 * ------------------------------------------------------------------------- */

chrome.downloads?.onCreated.addListener(async (item) => {
  const ayarlar = await ayarlariAl();
  if (!ayarlar.interceptDownloads) return;
  if (!/^https?:\/\//i.test(item.url || '')) return;

  // Blob ve data adresleri sayfanın belleğinde; Muiget onları çözemez.
  if (item.url.startsWith('blob:') || item.url.startsWith('data:')) return;

  const host = alanAdi(item.url);
  if (host && ayarlar.ignoredHosts.includes(host)) return;

  try {
    await indirmeyiGonder({
      url: item.finalUrl || item.url,
      fileName: dosyaAdi(item.filename),
      referrer: item.referrer,
    });

    // Devralma başarılı: Chrome'un kopyasını iptal et ve listeden temizle.
    // Sıra önemli — önce iptal, sonra silme; ters sırada iptal hedefini
    // bulamıyor.
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
    await rozetGoster('✓', '#2dd4bf');
  } catch (e) {
    // Devralma başarısızsa Chrome'un indirmesine DOKUNMUYORUZ: kullanıcı
    // dosyasını yine de alsın. Sessiz veri kaybı en kötü sonuç olurdu.
    console.warn('Muiget: devralma başarısız, Chrome indirmeye devam ediyor', e);
    await rozetGoster('!', '#fbbf24');
  }
});

function alanAdi(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

/** Chrome tam yol verebiliyor; bize yalnızca son parça lazım. */
function dosyaAdi(yol) {
  if (!yol) return null;
  const parcalar = yol.split(/[\\/]/);
  return parcalar[parcalar.length - 1] || null;
}

/* ---------------------------------------------------------------------------
 * Video yakalama (HLS / DASH)
 *
 * IDM'i bugün satan özellik bu: sayfadaki oynatıcı videoyu tek bir dosyadan
 * değil, bir manifest üzerinden yüzlerce parça hâlinde alıyor. O manifestin
 * adresi sayfanın HTML'inde yok — JavaScript çalışırken isteniyor. Yani
 * `popup.js`'teki DOM taraması bunu **hiçbir zaman** bulamaz; ağ isteklerine
 * bakmak şart.
 *
 * `webRequest` izni bilerek **isteğe bağlı** ve varsayılan kapalı: verildiğinde
 * uzantı ziyaret edilen her sayfanın ağ isteklerinin adreslerini görüyor.
 * Bu ağır bir yetki ve sessizce açık gelmemeli — aynı gerekçe masaüstündeki
 * pano izlemede de var (karar #24).
 *
 * Görülen adresler hiçbir yere gönderilmiyor: yalnızca `storage.session`de,
 * sekme başına, tarayıcı kapanınca silinecek şekilde tutuluyor ve süzgeçten
 * geçen (`.m3u8` / `.mpd`) adresler dışında hiçbiri kaydedilmiyor.
 * ------------------------------------------------------------------------- */

const MANIFEST_DESENI = /\.(m3u8|mpd)(\?|#|$)/i;

/** Sekme başına saklanan en fazla adres. Bir oynatıcı kalite değiştirdikçe
    yeni manifest istiyor; sınırsız biriktirmenin kimseye faydası yok. */
const VIDEO_SINIRI = 12;

const videoAnahtari = (tabId) => `video:${tabId}`;

function videoYakala(details) {
  // `tabId < 0`: sekmeye ait olmayan istek (uzantı, service worker).
  if (details.tabId < 0) return;
  if (!MANIFEST_DESENI.test(details.url)) return;
  void videoKaydet(details.tabId, details.url);
}

async function videoKaydet(tabId, url) {
  const anahtar = videoAnahtari(tabId);
  const kayit = await chrome.storage.session.get(anahtar);
  const liste = kayit[anahtar] ?? [];
  if (liste.some((v) => v.url === url)) return;

  liste.unshift({ url, at: Date.now() });
  await chrome.storage.session.set({ [anahtar]: liste.slice(0, VIDEO_SINIRI) });

  // Sekmeye özel rozet: geçici "✓ / !" rozetleri genel olduğu için
  // birbirlerini ezmiyorlar.
  await chrome.action.setBadgeText({ tabId, text: String(Math.min(liste.length, 9)) });
  await chrome.action.setBadgeBackgroundColor({ tabId, color: '#7c5cff' });
}

/** Dinleyiciyi kurar. İzin verilmemişse sessizce vazgeçiyor. */
function videoDinleyiciyiKur() {
  if (!chrome.webRequest?.onBeforeRequest) return false;
  if (chrome.webRequest.onBeforeRequest.hasListener(videoYakala)) return true;

  try {
    chrome.webRequest.onBeforeRequest.addListener(videoYakala, { urls: ['<all_urls>'] });
    return true;
  } catch (e) {
    // Host izni verilmemiş olabilir.
    console.warn('Muiget: video yakalama kurulamadı', e);
    return false;
  }
}

export async function videoYakalamaAcikMi() {
  return chrome.permissions.contains({
    permissions: ['webRequest'],
    origins: ['<all_urls>'],
  });
}

export async function sekmeninVideolari(tabId) {
  const kayit = await chrome.storage.session.get(videoAnahtari(tabId));
  return kayit[videoAnahtari(tabId)] ?? [];
}

/** Sayfa değişince liste sıfırlanıyor: önceki sayfanın videosunu göstermek
    kullanıcıyı yanıltırdı. */
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (!changeInfo.url) return;
  void chrome.storage.session.remove(videoAnahtari(tabId));
  void chrome.action.setBadgeText({ tabId, text: '' });
});

chrome.tabs.onRemoved.addListener((tabId) => {
  void chrome.storage.session.remove(videoAnahtari(tabId));
});

// İzin sonradan verilirse service worker'ı yeniden başlatmadan devreye gir.
chrome.permissions.onAdded.addListener(() => {
  videoDinleyiciyiKur();
});

// Service worker her uyanışta yeniden çalışıyor; dinleyici burada kuruluyor.
videoDinleyiciyiKur();

/* ---------------------------------------------------------------------------
 * Popup ile mesajlaşma
 * ------------------------------------------------------------------------- */

chrome.runtime.onMessage.addListener((mesaj, _sender, sendResponse) => {
  // `true` döndürmek Chrome'a "yanıtı async vereceğim" demek; olmazsa kanal
  // hemen kapanıyor ve popup yanıtı hiç alamıyor.
  (async () => {
    try {
      switch (mesaj?.type) {
        case 'ping':
          sendResponse(await kopruyuSina());
          break;
        case 'download':
          sendResponse({ ok: true, result: await indirmeyiGonder(mesaj.payload) });
          break;
        case 'getSettings':
          sendResponse(await ayarlariAl());
          break;
        case 'setSettings':
          await chrome.storage.local.set(mesaj.payload);
          sendResponse(await ayarlariAl());
          break;
        case 'getVideos':
          sendResponse({
            ok: true,
            enabled: await videoYakalamaAcikMi(),
            videos: await sekmeninVideolari(mesaj.tabId),
          });
          break;
        default:
          sendResponse({ ok: false, error: 'bilinmeyen mesaj' });
      }
    } catch (e) {
      sendResponse({ ok: false, error: e.message });
    }
  })();

  return true;
});
