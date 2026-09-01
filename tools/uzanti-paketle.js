// Muiget uzantı paketleyici.
//
// Tek kaynaktan (extension/) iki paket üretiyor:
//
//   dist-extension/chrome/    Chrome ve Edge — kaynak manifest olduğu gibi
//   dist-extension/firefox/   Firefox — manifest dönüştürülerek
//
// Neden ikinci bir `manifest.firefox.json` **yok**: iki manifesti elle eşit
// tutmak sürüm numarasından izin listesine kadar her değişiklikte kaçırılacak
// bir adım demek (uzantının sürümü üç yayın boyunca geride kalmıştı, bkz.
// docs/decisions.md). Firefox manifesti burada Chrome manifestinden
// türetiliyor; ayrıştıkları yerler aşağıda tek tek yazılı (karar #31).
//
// Kullanım:
//   node tools/uzanti-paketle.js            # ikisini de üret
//   node tools/uzanti-paketle.js --magaza   # mağaza derlemesi (YouTube kapalı)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const KOK = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const KAYNAK = path.join(KOK, 'extension');
const HEDEF = path.join(KOK, 'dist-extension');

const MAGAZA = process.argv.includes('--magaza');

/** Firefox uzantı kimliği. Rust tarafındaki `FIREFOX_EXTENSION_ID` ile
    birebir aynı olmak zorunda: köprü manifestine o yazılıyor. */
const FIREFOX_KIMLIK = 'muiget@muiget.app';

/** En düşük Firefox sürümü.
 *
 *  128 ESR: `optional_host_permissions` ve MV3 olay sayfaları bu sürümde
 *  oturmuş durumda. Daha aşağı inmek uzantıyı yükleyip özelliklerin sessizce
 *  çalışmadığı bir tarayıcıya izin vermek olurdu. */
const EN_DUSUK_FIREFOX = '128.0';

/** Paketlere girmeyecek dosyalar. */
const ATLANAN = new Set(['README.md']);

function temizle(yol) {
  fs.rmSync(yol, { recursive: true, force: true });
  fs.mkdirSync(yol, { recursive: true });
}

/** Klasörü olduğu gibi kopyalar; `manifest.json` ayrı yazılıyor. */
function kopyala(kaynak, hedef) {
  for (const giris of fs.readdirSync(kaynak, { withFileTypes: true })) {
    if (ATLANAN.has(giris.name) || giris.name === 'manifest.json') continue;

    const kaynakYol = path.join(kaynak, giris.name);
    const hedefYol = path.join(hedef, giris.name);

    if (giris.isDirectory()) {
      fs.mkdirSync(hedefYol, { recursive: true });
      kopyala(kaynakYol, hedefYol);
    } else {
      fs.copyFileSync(kaynakYol, hedefYol);
    }
  }
}

/**
 * Mağaza derlemesinde doğrudan medya yakalamayı kapatır (karar #27).
 *
 * Sabit bulunamazsa **hata veriyoruz**: sessizce geçmek, YouTube yakalaması
 * açık bir paketi Web Store'a göndermek demek olurdu ve bunun bedeli uzantının
 * mağazadan kaldırılması.
 */
function bayragiUygula(hedef) {
  if (!MAGAZA) return;

  const yol = path.join(hedef, 'background.js');
  const metin = fs.readFileSync(yol, 'utf8');
  const desen = /^const DOGRUDAN_MEDYA_YAKALAMA = true;$/m;

  if (!desen.test(metin)) {
    throw new Error(
      'background.js içinde `const DOGRUDAN_MEDYA_YAKALAMA = true;` satırı bulunamadı — ' +
        'mağaza derlemesi YouTube yakalaması açık çıkardı, paketleme durduruldu.',
    );
  }

  fs.writeFileSync(yol, metin.replace(desen, 'const DOGRUDAN_MEDYA_YAKALAMA = false;'));
}

/**
 * Chrome manifestini Firefox'un anladığı biçime çevirir.
 *
 * Ayrıştıkları yerler:
 *
 * - `background.service_worker` → `background.scripts`. Firefox MV3'te arka
 *   plan bir **olay sayfası**; servis çalışanı desteği yok. `background.js`
 *   bu yüzden modül değil (bkz. dosyanın başındaki not).
 * - `browser_specific_settings.gecko.id`: Firefox kimliği uzantının kendi
 *   beyanı ve köprü manifestine o kimlik yazılıyor; sabit olmak zorunda.
 * - `minimum_chrome_version` Firefox'ta tanınmayan bir alan; yüklemede uyarı
 *   üretiyor, o yüzden çıkarılıyor.
 */
function firefoxManifesti(manifest) {
  const cikti = { ...manifest };

  delete cikti.minimum_chrome_version;
  cikti.background = { scripts: ['background.js'] };
  cikti.browser_specific_settings = {
    gecko: { id: FIREFOX_KIMLIK, strict_min_version: EN_DUSUK_FIREFOX },
  };

  return cikti;
}

function paketle(ad, manifest) {
  const hedef = path.join(HEDEF, ad);
  temizle(hedef);
  kopyala(KAYNAK, hedef);
  fs.writeFileSync(
    path.join(hedef, 'manifest.json'),
    JSON.stringify(manifest, null, 2) + '\n',
  );
  bayragiUygula(hedef);
  console.log(`${ad.padEnd(8)} → ${path.relative(KOK, hedef)}`);
}

const manifest = JSON.parse(fs.readFileSync(path.join(KAYNAK, 'manifest.json'), 'utf8'));

paketle('chrome', manifest);
paketle('firefox', firefoxManifesti(manifest));

console.log(
  MAGAZA
    ? 'Mağaza derlemesi: doğrudan medya yakalama KAPALI (karar #27).'
    : 'GitHub derlemesi: doğrudan medya yakalama açık (karar #27).',
);
