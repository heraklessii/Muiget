/**
 * Popup mantığı.
 *
 * Sayfa taraması burada tetikleniyor, kalıcı bir content script'le değil:
 * kullanıcı popup'ı açmadıkça hiçbir sayfaya kod enjekte edilmiyor. Bu hem
 * gizlilik hem performans açısından doğru varsayılan.
 */

/** Sayfada aranan dosya türleri. */
const UZANTILAR =
  /\.(mp4|mkv|webm|avi|mov|flv|m4v|mp3|m4a|flac|wav|ogg|opus|zip|rar|7z|tar|gz|xz|bz2|iso|exe|msi|dmg|pkg|deb|rpm|apk|pdf|epub)(\?|#|$)/i;

const durumEl = document.getElementById('durum');
const medyaEl = document.getElementById('medya');
const hataEl = document.getElementById('hata');
const devralEl = document.getElementById('devral');
const cerezEl = document.getElementById('cerez');

/* ---------------------------------------------------------------------------
 * Sayfada çalışacak tarayıcı fonksiyonu
 *
 * DİKKAT: bu fonksiyonun gövdesi `chrome.scripting.executeScript` ile hedef
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
    chrome.runtime.sendMessage(mesaj, (yanit) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
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
  const [sekme] = await chrome.tabs.query({ active: true, currentWindow: true });

  if (!sekme?.id || !/^https?:/i.test(sekme.url || '')) {
    medyaEl.innerHTML = '<p class="bos">Bu sayfa taranamıyor.</p>';
    return;
  }

  let bulunanlar = [];
  try {
    const sonuclar = await chrome.scripting.executeScript({
      target: { tabId: sekme.id },
      func: sayfayiTara,
    });
    bulunanlar = sonuclar?.[0]?.result ?? [];
  } catch (e) {
    // Chrome Web Store, chrome:// gibi sayfalarda enjeksiyon yasak.
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
 * Ayarlar
 *
 * İki ayar da ek Chrome izni gerektiriyor. İzin ancak kullanıcı anahtarı
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
    chrome.permissions.request(permissions, (verildi) => resolve(Boolean(verildi)));
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
medyayiListele().catch((e) => hataGoster(e.message));
ayarlariYukle().catch((e) => hataGoster(e.message));
