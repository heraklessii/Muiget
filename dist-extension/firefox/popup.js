/**
 * Popup mantığı.
 *
 * Sayfa taraması burada tetikleniyor, kalıcı bir content script'le değil:
 * kullanıcı popup'ı açmadıkça hiçbir sayfaya kod enjekte edilmiyor. Bu hem
 * gizlilik hem performans açısından doğru varsayılan.
 */

/**
 * Tarayıcı API'si. Gerekçesi background.js'in başında (karar #31): Firefox'ta
 * Promise döndüren ad `browser`, Chrome/Edge'de `chrome`.
 */
const api = globalThis.browser ?? globalThis.chrome;

/** Sayfada aranan dosya türleri. */
const UZANTILAR =
  /\.(mp4|mkv|webm|avi|mov|flv|m4v|mp3|m4a|flac|wav|ogg|opus|zip|rar|7z|tar|gz|xz|bz2|iso|exe|msi|dmg|pkg|deb|rpm|apk|pdf|epub)(\?|#|$)/i;

const durumEl = document.getElementById('durum');
const medyaEl = document.getElementById('medya');
const videolarEl = document.getElementById('videolar');
const hataEl = document.getElementById('hata');
const devralEl = document.getElementById('devral');
const cerezEl = document.getElementById('cerez');
const videoYakalaEl = document.getElementById('videoYakala');

/* ---------------------------------------------------------------------------
 * Sayfada çalışacak tarayıcı fonksiyonu
 *
 * DİKKAT: bu fonksiyonun gövdesi `scripting.executeScript` ile hedef
 * sayfaya serileştirilip gönderiliyor. Dışarıdaki hiçbir değişkeni ya da
 * fonksiyonu göremez — bu yüzden desen içeride yeniden tanımlanıyor.
 * ------------------------------------------------------------------------- */
function sayfayiTara() {
  const desen =
    /\.(mp4|mkv|webm|avi|mov|flv|m4v|mp3|m4a|flac|wav|ogg|opus|zip|rar|7z|tar|gz|xz|bz2|iso|exe|msi|dmg|pkg|deb|rpm|apk|pdf|epub)(\?|#|$)/i;

  const bulunanlar = new Map();

  const ekle = (url, tur) => {
    if (!url || !/^https?:\/\//i.test(url)) return;
    if (bulunanlar.has(url)) return;

    let ad = url;
    try {
      const yol = new URL(url).pathname;
      ad = decodeURIComponent(yol.split('/').pop() || url);
    } catch {
      /* Bozuk URL: ham hâliyle göster. */
    }
    bulunanlar.set(url, { url, ad, tur });
  };

  // Doğrudan medya elemanları.
  for (const el of document.querySelectorAll('video[src], audio[src]')) {
    ekle(el.src, el.tagName.toLowerCase());
  }
  for (const el of document.querySelectorAll('video source, audio source')) {
    ekle(el.src, 'medya');
  }

  // Dosyaya işaret eden bağlantılar.
  for (const el of document.querySelectorAll('a[href]')) {
    if (desen.test(el.href)) ekle(el.href, 'bağlantı');
  }

  return [...bulunanlar.values()].slice(0, 50);
}

/* ---------------------------------------------------------------------------
 * Yardımcılar
 * ------------------------------------------------------------------------- */

function mesajGonder(mesaj) {
  return new Promise((resolve, reject) => {
    api.runtime.sendMessage(mesaj, (yanit) => {
      if (api.runtime.lastError) {
        reject(new Error(api.runtime.lastError.message));
        return;
      }
      resolve(yanit);
    });
  });
}

function hataGoster(mesaj) {
  hataEl.textContent = mesaj ?? '';
}

/* ---------------------------------------------------------------------------
 * Köprü durumu
 * ------------------------------------------------------------------------- */

async function durumuYenile() {
  const sonuc = await mesajGonder({ type: 'ping' });

  if (sonuc?.ok) {
    durumEl.textContent = `bağlı${sonuc.version ? ` · v${sonuc.version}` : ''}`;
    durumEl.className = 'durum acik';
  } else {
    durumEl.textContent = 'Muiget bulunamadı';
    durumEl.className = 'durum kapali';
    hataGoster('Masaüstü uygulaması kurulu ve köprü kayıtlı olmalı (Ayarlar → Tarayıcı uzantısı).');
  }
}

/* ---------------------------------------------------------------------------
 * Medya listesi
 * ------------------------------------------------------------------------- */

async function medyayiListele() {
  const [sekme] = await api.tabs.query({ active: true, currentWindow: true });

  if (!sekme?.id || !/^https?:/i.test(sekme.url || '')) {
    medyaEl.innerHTML = '<p class="bos">Bu sayfa taranamıyor.</p>';
    return;
  }

  let bulunanlar = [];
  try {
    const sonuclar = await api.scripting.executeScript({
      target: { tabId: sekme.id },
      func: sayfayiTara,
    });
    bulunanlar = sonuclar?.[0]?.result ?? [];
  } catch (e) {
    // Mağaza sayfaları ve chrome:// | about: adreslerinde enjeksiyon yasak.
    medyaEl.innerHTML = '<p class="bos">Bu sayfada tarama yapılamıyor.</p>';
    console.warn('Muiget: sayfa taranamadı', e);
    return;
  }

  if (bulunanlar.length === 0) {
    medyaEl.innerHTML = '<p class="bos">İndirilebilir dosya bulunamadı.</p>';
    return;
  }

  const liste = document.createElement('ul');
  liste.className = 'ogeler';

  for (const oge of bulunanlar) {
    const satir = document.createElement('li');
    satir.className = 'oge';

    const ad = document.createElement('span');
    ad.className = 'oge__ad';
    ad.textContent = oge.ad;
    ad.title = oge.url;

    const tur = document.createElement('span');
    tur.className = 'oge__tur';
    tur.textContent = oge.tur;

    const dugme = document.createElement('button');
    dugme.textContent = 'İndir';
    dugme.addEventListener('click', async () => {
      dugme.disabled = true;
      dugme.textContent = '…';
      try {
        const yanit = await mesajGonder({
          type: 'download',
          payload: { url: oge.url, fileName: oge.ad, referrer: sekme.url },
        });
        if (!yanit?.ok) throw new Error(yanit?.error || 'gönderilemedi');
        dugme.textContent = 'Gönderildi';
        hataGoster('');
      } catch (e) {
        dugme.textContent = 'Hata';
        dugme.disabled = false;
        hataGoster(e.message);
      }
    });

    satir.append(ad, tur, dugme);
    liste.append(satir);
  }

  medyaEl.replaceChildren(liste);
}

/* ---------------------------------------------------------------------------
 * Yakalanan videolar
 *
 * DOM taramasıyla bulunamayan tek tür bunlar: HLS/DASH manifesti sayfanın
 * HTML'inde geçmiyor, oynatıcı JavaScript'i çalışırken isteniyor. Bu yüzden
 * liste arka plandaki ağ dinleyicisinden geliyor (bkz. `background.js`).
 * ------------------------------------------------------------------------- */

/** `.../vod/master.m3u8?token=x` → `vod / master.m3u8` */
function videoAdi(url) {
  try {
    const parcalar = new URL(url).pathname.split('/').filter(Boolean);
    return decodeURIComponent(parcalar.slice(-2).join(' / ')) || url;
  } catch {
    return url;
  }
}

async function videolariListele(sekme) {
  if (!sekme?.id) {
    videolarEl.innerHTML = '<p class="bos">Bu sayfa taranamıyor.</p>';
    return;
  }

  const sonuc = await mesajGonder({ type: 'getVideos', tabId: sekme.id });
  videoYakalaEl.checked = Boolean(sonuc?.enabled);

  if (!sonuc?.enabled) {
    videolarEl.innerHTML =
      '<p class="bos">Video yakalama kapalı. Aşağıdaki anahtarı açıp sayfayı yenileyin.</p>';
    return;
  }

  const videolar = sonuc.videos ?? [];
  if (videolar.length === 0) {
    videolarEl.innerHTML =
      '<p class="bos">Bu sayfada video yayını görülmedi. Oynatıcıyı başlatıp tekrar bakın — ' +
      'yayın adresi ancak video oynamaya başlayınca isteniyor.</p>';
    return;
  }

  const liste = document.createElement('ul');
  liste.className = 'ogeler';

  for (const video of videolar) {
    const satir = document.createElement('li');
    satir.className = 'oge';

    const ad = document.createElement('span');
    ad.className = 'oge__ad';
    // Doğrudan yakalanan akışlarda dosya adı yok (adres bir sorgu dizesi);
    // yakalama anında üretilen etiket tek anlamlı ad.
    ad.textContent = video.etiket ?? videoAdi(video.url);
    ad.title = video.url;

    const tur = document.createElement('span');
    tur.className = 'oge__tur';
    // Tür yakalama anında belirleniyor; `tur` yoksa kayıt eski sürümden.
    tur.textContent = video.tur ?? 'HLS';

    // Sessiz akış: video izi tek başına indirilirse ses olmuyor. Karar #25
    // gereği bunu indirme anında değil, **düğmeye basmadan önce** söylemek
    // gerekiyor; sessiz dosyayı teslim edip sonra açıklamak en kötüsü.
    if (video.sessiz) {
      const uyari = document.createElement('span');
      uyari.className = 'oge__uyari';
      uyari.textContent = 'sessiz — ses ayrı iniyor';
      satir.appendChild(uyari);
    }

    const dugme = document.createElement('button');
    dugme.textContent = 'İndir';
    dugme.addEventListener('click', async () => {
      dugme.disabled = true;
      dugme.textContent = '…';
      try {
        // Dosya adı **gönderilmiyor**: uzantısı ve kalitesi manifest okunmadan
        // bilinmiyor. Masaüstü tarafı adı manifestten türetiyor.
        const yanit = await mesajGonder({
          type: 'download',
          payload: { url: video.url, referrer: sekme.url },
        });
        if (!yanit?.ok) throw new Error(yanit?.error || 'gönderilemedi');
        dugme.textContent = 'Gönderildi';
        hataGoster('');
      } catch (e) {
        dugme.textContent = 'Hata';
        dugme.disabled = false;
        hataGoster(e.message);
      }
    });

    satir.append(ad, tur, dugme);
    liste.append(satir);
  }

  videolarEl.replaceChildren(liste);
}

videoYakalaEl.addEventListener('change', async () => {
  if (videoYakalaEl.checked) {
    const verildi = await izinIste({
      permissions: ['webRequest'],
      origins: ['<all_urls>'],
    });
    if (!verildi) {
      videoYakalaEl.checked = false;
      hataGoster('Video yakalamak için ağ isteklerini görme izni gerekiyor.');
      return;
    }
    hataGoster('Açıldı. Şu anki sayfayı yenileyince videolar burada görünecek.');
    return;
  }

  // İzni geri almak dinleyiciyi de düşürüyor: kapalıyken uzantı hiçbir ağ
  // isteğini görmüyor, yalnızca "dinlemiyorum" demiyor.
  await api.permissions.remove({ permissions: ['webRequest'] });
  videolarEl.innerHTML = '<p class="bos">Video yakalama kapalı.</p>';
  hataGoster('');
});

/* ---------------------------------------------------------------------------
 * Ayarlar
 *
 * İki ayar da ek tarayıcı izni gerektiriyor. İzin ancak kullanıcı anahtarı
 * açtığında isteniyor — kurulumda hepsini birden istemek, uzantının neye
 * eriştiğini anlaşılmaz kılardı.
 * ------------------------------------------------------------------------- */

async function ayarlariYukle() {
  const ayarlar = await mesajGonder({ type: 'getSettings' });
  devralEl.checked = Boolean(ayarlar?.interceptDownloads);
  cerezEl.checked = Boolean(ayarlar?.sendCookies);
}

async function izinIste(permissions) {
  return new Promise((resolve) => {
    api.permissions.request(permissions, (verildi) => resolve(Boolean(verildi)));
  });
}

devralEl.addEventListener('change', async () => {
  if (devralEl.checked) {
    const verildi = await izinIste({ permissions: ['downloads'] });
    if (!verildi) {
      devralEl.checked = false;
      hataGoster('İndirmeleri devralmak için izin gerekiyor.');
      return;
    }
  }
  await mesajGonder({
    type: 'setSettings',
    payload: { interceptDownloads: devralEl.checked },
  });
  hataGoster('');
});

cerezEl.addEventListener('change', async () => {
  if (cerezEl.checked) {
    const verildi = await izinIste({
      permissions: ['cookies'],
      origins: ['<all_urls>'],
    });
    if (!verildi) {
      cerezEl.checked = false;
      hataGoster('Çerez gönderimi için izin gerekiyor.');
      return;
    }
  }
  await mesajGonder({ type: 'setSettings', payload: { sendCookies: cerezEl.checked } });
  hataGoster('');
});

/* ---------------------------------------------------------------------------
 * Başlangıç
 * ------------------------------------------------------------------------- */

durumuYenile().catch((e) => hataGoster(e.message));
ayarlariYukle().catch((e) => hataGoster(e.message));
medyayiListele().catch((e) => hataGoster(e.message));

api.tabs
  .query({ active: true, currentWindow: true })
  .then(([sekme]) => videolariListele(sekme))
  .catch((e) => hataGoster(e.message));
