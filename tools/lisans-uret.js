// Muiget üçüncü parti lisans bildirimi üretici.
//
// `NOTICE` dosyasını **elle** tutmak yayın öncesi bir borç olarak duruyordu:
// dosya yalnızca doğrudan bağımlılıkları listeliyordu (yüzlerce crate'in 25'i)
// ve içinde hiç kullanılmayan bir crate bile vardı (librqbit — Faz 4'te
// eklenecek, `Cargo.toml`'da yok). Elle tutulan bir liste her bağımlılık
// eklendiğinde güncellenmesi unutulacak bir adım demek; bu betik listeyi
// kilit dosyalarından türetiyor, `NOTICE` artık üretilmiş bir çıktı.
//
// Kullanım:
//   node tools/lisans-uret.js            # NOTICE'ı yeniden üret
//   node tools/lisans-uret.js --kontrol  # güncel mi? değilse çıkış kodu 1
//
// Kapsam kuralı — dağıtılan ne varsa listeleniyor:
//   * çalışma zamanı (normal) bağımlılıkları: binary'nin içindeler
//   * derleme (build) bağımlılıkları: ürettikleri kod binary'ye giriyor
//   * test (dev) bağımlılıkları HARİÇ: kullanıcıya giden pakette yoklar
//
// `cargo metadata` platform süzmesi yapmadan çalıştırılıyor: yayın Windows,
// Linux ve macOS paketleri üretiyor, dolayısıyla bildirim üçünün toplamını
// kapsamalı.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const KOK = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const KONTROL = process.argv.includes('--kontrol');
const CIZGI = '-'.repeat(80);
const CIFT_CIZGI = '='.repeat(80);

/** İzin verici (permissive) SPDX kimlikleri.
 *
 *  Bir lisans ifadesinin **en az bir** seçeneği tamamen bu kümedense crate
 *  toplu listeye giriyor. Değilse aşağıda tek tek, adıyla ve adresiyle
 *  yazılıyor — dikkat isteyen lisansın kalabalıkta kaybolmaması için. */
const IZIN_VERICI = new Set([
  '0BSD', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'BSL-1.0', 'CC0-1.0',
  'ISC', 'MIT', 'MIT-0', 'NCSA', 'OpenSSL', 'Unicode-3.0', 'Unicode-DFS-2016',
  'Unlicense', 'Zlib',
]);

/** Elle yazılan bölümler: hiçbir bağımlılık ağacında görünmedikleri için
 *  üretilemiyorlar. Gömülü yazı tipi pakete giriyor ve Apache 2.0 değil,
 *  ffmpeg ise dağıtılmıyor — ikisi de ayrı ayrı açıklanmayı hak ediyor. */
const ELLE = `${CIFT_CIZGI}
PAKETE GÖMÜLÜ YAZI TİPİ
${CIFT_CIZGI}

${CIZGI}
Outfit — arayüz yazı tipi
  Copyright (c) 2020 The Outfit Project Authors
  Lisans: SIL Open Font License, Version 1.1
  https://github.com/Outfitio/Outfit-Fonts

  src/assets/fonts/ altındaki dosyalar Apache License 2.0'a değil SIL OFL
  1.1'e tabi. OFL yazı tipinin yazılımla birlikte paketlenip dağıtılmasına
  izin veriyor; yasakladığı şey yazı tipinin tek başına satılması.
  Tam metin: https://openfontlicense.org/

${CIFT_CIZGI}
PAKETE GİRMEYEN DIŞ ARAÇ
${CIFT_CIZGI}

${CIZGI}
ffmpeg — isteğe bağlı kap dönüşümü ve ses/video birleştirme
  Lisans: LGPL-2.1-or-later ya da GPL-2.0-or-later (derlemesine göre)
  https://ffmpeg.org

  Muiget ffmpeg'i **dağıtmıyor ve bağlamıyor**; kullanıcının makinesinde
  varsa ayrı bir süreç olarak çağırıyor (src-tauri/src/media/mux.rs,
  karar #25). Bu yüzden ffmpeg'in lisansı Muiget'in dağıtımını bağlamıyor.
  ffmpeg kurulu değilse ilgili özellik çalışmıyor, uygulamanın geri kalanı
  etkilenmiyor.
`;

/** Lisans ifadesini tek bir yazıma çekiyor; yalnızca gruplama düzgün olsun
 *  diye — anlam değişmiyor.
 *
 *  İki iş yapıyor: `MIT/Apache-2.0` gibi eski yazımları SPDX'e çeviriyor ve
 *  `OR` seçeneklerini alfabetik sıraya sokuyor. İkincisi olmadan
 *  `MIT OR Apache-2.0` ile `Apache-2.0 OR MIT` ayrı iki başlık oluyor ve aynı
 *  lisans listede üç yere dağılıyordu (340 crate, üç grup). Parantezli
 *  ifadelere dokunulmuyor: orada sıralamak anlamı bozabilir. */
function duzelt(ifade) {
  const duz = (ifade || 'BELIRTILMEMIS').replace(/\s*\/\s*/g, ' OR ').trim();
  if (duz.includes('(')) return duz;

  return duz
    .split(/\s+OR\s+/)
    .map((secenek) => secenek.trim())
    .sort((a, b) => a.localeCompare(b, 'en'))
    .join(' OR ');
}

function izinVerici(ifade) {
  return duzelt(ifade)
    .replace(/[()]/g, ' ')
    .split(/\s+OR\s+/)
    .some((secenek) =>
      secenek
        .split(/\s+AND\s+/)
        .map((terim) => terim.trim().split(/\s+WITH\s+/)[0])
        .filter(Boolean)
        .every((terim) => IZIN_VERICI.has(terim)),
    );
}

/** Uzun crate listesini 78 sütuna sığdırıyor. */
function sar(girdiler) {
  const satirlar = [];
  let satir = ' ';

  for (const girdi of girdiler) {
    const parca = ` ${girdi}`;
    if (satir.length + parca.length > 78) {
      satirlar.push(satir);
      satir = ' ';
    }
    satir += parca;
  }

  if (satir.trim()) satirlar.push(satir);
  return satirlar.join('\n');
}

/** Rust tarafı: çözümlenmiş graf üzerinde kökten yürüyor, `dev` kenarlarını
 *  atlıyor. Kilit dosyasının tamamını okumak yanlış olurdu — `Cargo.lock`
 *  test bağımlılıklarını da içeriyor ve onlar kullanıcıya gitmiyor. */
function rustPaketleri() {
  const cikti = execFileSync(
    'cargo',
    ['metadata', '--format-version', '1'],
    {
      cwd: path.join(KOK, 'src-tauri'),
      encoding: 'utf8',
      maxBuffer: 128 * 1024 * 1024,
    },
  );

  const meta = JSON.parse(cikti);
  const paketler = new Map(meta.packages.map((p) => [p.id, p]));
  const dugumler = new Map(meta.resolve.nodes.map((n) => [n.id, n]));

  const gorulen = new Set();
  const sira = [meta.resolve.root];

  while (sira.length) {
    const id = sira.shift();
    if (!id || gorulen.has(id)) continue;
    gorulen.add(id);

    for (const kenar of dugumler.get(id)?.deps ?? []) {
      if (kenar.dep_kinds.some((k) => k.kind !== 'dev')) sira.push(kenar.pkg);
    }
  }

  gorulen.delete(meta.resolve.root);

  return [...gorulen]
    .map((id) => paketler.get(id))
    .filter(Boolean)
    .map((p) => ({
      ad: p.name,
      surum: p.version,
      lisans: duzelt(p.license),
      adres: p.repository || '',
    }))
    .sort((a, b) => a.ad.localeCompare(b.ad, 'en'));
}

/** npm tarafı doğrudan `package-lock.json`'dan okunuyor.
 *
 *  `npm ls` çağırmak yerine kilit dosyası: lockfile v3 her girdinin lisansını
 *  ve `dev` işaretini zaten taşıyor, yani ne node_modules'ün kurulu olması ne
 *  de bir alt süreç gerekiyor. (Windows'ta `npm.cmd`'yi doğrudan çağırmak
 *  Node 20+ ile EINVAL veriyor; kilit dosyası bu sorunu da atlıyor.) */
function npmPaketleri() {
  const kilit = JSON.parse(
    fs.readFileSync(path.join(KOK, 'package-lock.json'), 'utf8'),
  );

  return Object.entries(kilit.packages)
    .filter(([yol, bilgi]) => yol.startsWith('node_modules/') && !bilgi.dev)
    .map(([yol, bilgi]) => {
      const ad = yol.slice(yol.lastIndexOf('node_modules/') + 'node_modules/'.length);
      return {
        ad,
        surum: bilgi.version,
        lisans: duzelt(bilgi.license),
        adres: `https://www.npmjs.com/package/${ad}`,
      };
    })
    .sort((a, b) => a.ad.localeCompare(b.ad, 'en'));
}

/** Lisansa göre gruplanmış toplu liste. */
function topluListe(paketler) {
  const gruplar = new Map();

  for (const p of paketler) {
    if (!gruplar.has(p.lisans)) gruplar.set(p.lisans, []);
    gruplar.get(p.lisans).push(`${p.ad} ${p.surum}`);
  }

  return [...gruplar.entries()]
    .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0], 'en'))
    .map(([lisans, girdiler]) =>
      `${CIZGI}\n${lisans}  (${girdiler.length} paket)\n\n${sar(girdiler)}`)
    .join('\n\n');
}

/** İzin verici olmayan lisanslar tek tek, adresiyle. */
function dikkatListesi(paketler) {
  const isaretli = paketler.filter((p) => !izinVerici(p.lisans));
  if (!isaretli.length) return 'Bu derlemede izin verici olmayan bir lisans yok.\n';

  return isaretli
    .map((p) =>
      `${CIZGI}\n${p.ad} ${p.surum}\n  Lisans: ${p.lisans}` +
      (p.adres ? `\n  ${p.adres}` : ''))
    .join('\n\n') + '\n';
}

const rust = rustPaketleri();
const npm = npmPaketleri();
const eksik = [...rust, ...npm].filter((p) => p.lisans === 'BELIRTILMEMIS');

if (eksik.length) {
  console.error(
    `Lisansı okunamayan paket: ${eksik.map((p) => p.ad).join(', ')}\n` +
    'Bunlar elle incelenmeli; bildirim eksik kalamaz.',
  );
  process.exit(1);
}

const metin = `Muiget
Copyright 2026 Muiget Contributors

Bu ürün Muiget projesi tarafından geliştirilen yazılımı içerir
(https://github.com/heraklessii/Muiget).

Apache License, Version 2.0 ile lisanslanmıştır. Tam metin için LICENSE
dosyasına bakın.

BU DOSYA ÜRETİLMİŞTİR — elle düzenlemeyin. Üretici: tools/lisans-uret.js
("npm run lisans"). Kaynak: src-tauri/Cargo.lock + package-lock.json.
Kapsam: dağıtılan pakete giren çalışma zamanı ve derleme bağımlılıkları;
test (dev) bağımlılıkları hariç. Rust tarafı platform süzmesi yapılmadan
çıkarıldı, yani Windows + Linux + macOS'un toplamı.

Toplam: ${rust.length} Rust crate'i, ${npm.length} npm paketi.

${CIFT_CIZGI}
DİKKAT İSTEYEN LİSANSLAR
${CIFT_CIZGI}

Aşağıdakiler izin verici (MIT / Apache-2.0 / BSD / ISC / Zlib ve benzeri)
kümesinin dışında. Hiçbiri Muiget'in Apache-2.0 dağıtımını engellemiyor —
MPL-2.0 dosya düzeyinde copyleft, yani değiştirilmiş **kendi** kaynak
dosyalarının paylaşılmasını istiyor, bizim kodumuza bulaşmıyor — ama
kalabalık listede gözden kaçmasınlar diye ayrı yazıldılar.

${dikkatListesi([...rust, ...npm])}
${CIFT_CIZGI}
RUST CRATE'LERİ (${rust.length})
${CIFT_CIZGI}

${topluListe(rust)}

${CIFT_CIZGI}
NPM PAKETLERİ (${npm.length})
${CIFT_CIZGI}

${topluListe(npm)}

${ELLE}`;

const hedef = path.join(KOK, 'NOTICE');

if (KONTROL) {
  const mevcut = fs.existsSync(hedef) ? fs.readFileSync(hedef, 'utf8') : '';

  if (mevcut !== metin) {
    console.error(
      "NOTICE güncel değil. 'npm run lisans' çalıştırıp sonucu commit'leyin.",
    );
    process.exit(1);
  }

  console.log(`NOTICE güncel — ${rust.length} crate, ${npm.length} npm paketi.`);
} else {
  fs.writeFileSync(hedef, metin);
  console.log(`NOTICE yazıldı — ${rust.length} crate, ${npm.length} npm paketi.`);
}
