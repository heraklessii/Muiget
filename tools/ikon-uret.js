// Muiget uygulama ikonu üretici.
//
// Harici bir görüntü kütüphanesi yok: Node'un zlib'i ile elle PNG yazılıyor.
// Çizim 4 kat büyük yapılıp küçültülüyor (supersampling) — kenarlar böylece
// yumuşak çıkıyor.
//
// Biçim, arayüzdeki `IconDownload` ile aynı: aşağı ok + taban çizgisi.
// Renk, Mui ailesinin teal vurgusu (#2dd4bf) koyu bir zemin üzerinde.
const fs = require('fs');
const zlib = require('zlib');

const BOYUT = 1024;
const AA = 4; // supersampling katsayısı
const N = BOYUT * AA;

// --- Renkler ---
const ZEMIN_UST = [16, 32, 38]; // #102026
const ZEMIN_ALT = [8, 17, 21]; // #081115
const VURGU = [45, 212, 191]; // #2dd4bf
const VURGU_KOYU = [20, 160, 145];

const tampon = Buffer.alloc(N * N * 4);

function karistir(a, b, t) {
  return [
    Math.round(a[0] + (b[0] - a[0]) * t),
    Math.round(a[1] + (b[1] - a[1]) * t),
    Math.round(a[2] + (b[2] - a[2]) * t),
  ];
}

function koy(x, y, renk, alfa = 255) {
  if (x < 0 || y < 0 || x >= N || y >= N) return;
  const i = (y * N + x) * 4;
  const mevcutA = tampon[i + 3] / 255;
  const yeniA = alfa / 255;
  const sonA = yeniA + mevcutA * (1 - yeniA);
  if (sonA === 0) return;
  for (let k = 0; k < 3; k++) {
    tampon[i + k] = Math.round(
      (renk[k] * yeniA + tampon[i + k] * mevcutA * (1 - yeniA)) / sonA,
    );
  }
  tampon[i + 3] = Math.round(sonA * 255);
}

// Yuvarlatılmış kare içinde mi?
function yuvarlakKarede(x, y, sol, ust, sag, alt, r) {
  if (x < sol || x > sag || y < ust || y > alt) return false;
  const cx = Math.min(Math.max(x, sol + r), sag - r);
  const cy = Math.min(Math.max(y, ust + r), alt - r);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r;
}

// Kalın çizgi parçası (yuvarlak uçlu) üzerinde mi?
function cizgide(px, py, x1, y1, x2, y2, kalinlik) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const uzunluk2 = dx * dx + dy * dy;
  let t = uzunluk2 === 0 ? 0 : ((px - x1) * dx + (py - y1) * dy) / uzunluk2;
  t = Math.min(Math.max(t, 0), 1);
  const kx = x1 + t * dx;
  const ky = y1 + t * dy;
  const mx = px - kx;
  const my = py - ky;
  return mx * mx + my * my <= (kalinlik / 2) * (kalinlik / 2);
}

// --- Zemin: yuvarlatılmış kare, dikey degrade ---
const kenar = N * 0.055;
const sol = kenar;
const ust = kenar;
const sag = N - kenar;
const alt = N - kenar;
const yaricap = N * 0.235;

for (let y = 0; y < N; y++) {
  const t = y / N;
  const zemin = karistir(ZEMIN_UST, ZEMIN_ALT, t);
  for (let x = 0; x < N; x++) {
    if (yuvarlakKarede(x, y, sol, ust, sag, alt, yaricap)) koy(x, y, zemin);
  }
}

// --- Ok: dikey gövde + aşağı chevron + taban çizgisi ---
const merkez = N / 2;
const kalin = N * 0.085;

const govdeUst = N * 0.235;
const govdeAlt = N * 0.605;
const chevronY = N * 0.605;
const chevronX = N * 0.155;
const chevronUst = N * 0.44;
const tabanY = N * 0.80;
const tabanYari = N * 0.235;

for (let y = 0; y < N; y++) {
  // Ok gövdesi üstte açık, altta koyu: hafif bir derinlik.
  const tRenk = Math.min(Math.max((y / N - 0.2) / 0.65, 0), 1);
  const renk = karistir(VURGU, VURGU_KOYU, tRenk * 0.55);

  for (let x = 0; x < N; x++) {
    if (!yuvarlakKarede(x, y, sol, ust, sag, alt, yaricap)) continue;

    const okta =
      cizgide(x, y, merkez, govdeUst, merkez, govdeAlt, kalin) ||
      cizgide(x, y, merkez - chevronX, chevronUst, merkez, chevronY, kalin) ||
      cizgide(x, y, merkez + chevronX, chevronUst, merkez, chevronY, kalin) ||
      cizgide(x, y, merkez - tabanYari, tabanY, merkez + tabanYari, tabanY, kalin);

    if (okta) koy(x, y, renk);
  }
}

// --- Küçült (supersampling ortalaması) ---
const cikti = Buffer.alloc(BOYUT * BOYUT * 4);
for (let y = 0; y < BOYUT; y++) {
  for (let x = 0; x < BOYUT; x++) {
    let r = 0, g = 0, b = 0, a = 0;
    for (let dy = 0; dy < AA; dy++) {
      for (let dx = 0; dx < AA; dx++) {
        const i = ((y * AA + dy) * N + (x * AA + dx)) * 4;
        const pa = tampon[i + 3] / 255;
        r += tampon[i] * pa;
        g += tampon[i + 1] * pa;
        b += tampon[i + 2] * pa;
        a += pa;
      }
    }
    const adet = AA * AA;
    const j = (y * BOYUT + x) * 4;
    if (a > 0) {
      cikti[j] = Math.round(r / a);
      cikti[j + 1] = Math.round(g / a);
      cikti[j + 2] = Math.round(b / a);
    }
    cikti[j + 3] = Math.round((a / adet) * 255);
  }
}

// --- PNG yaz ---
function crc32(buf) {
  let c;
  const tablo = [];
  for (let n = 0; n < 256; n++) {
    c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    tablo[n] = c >>> 0;
  }
  let crc = 0xffffffff;
  for (const b of buf) crc = tablo[(crc ^ b) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function parca(tur, veri) {
  const uzunluk = Buffer.alloc(4);
  uzunluk.writeUInt32BE(veri.length);
  const govde = Buffer.concat([Buffer.from(tur, 'ascii'), veri]);
  const kontrol = Buffer.alloc(4);
  kontrol.writeUInt32BE(crc32(govde));
  return Buffer.concat([uzunluk, govde, kontrol]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(BOYUT, 0);
ihdr.writeUInt32BE(BOYUT, 4);
ihdr[8] = 8; // bit derinliği
ihdr[9] = 6; // RGBA
const satirlar = Buffer.alloc(BOYUT * (BOYUT * 4 + 1));
for (let y = 0; y < BOYUT; y++) {
  satirlar[y * (BOYUT * 4 + 1)] = 0; // filtre yok
  cikti.copy(satirlar, y * (BOYUT * 4 + 1) + 1, y * BOYUT * 4, (y + 1) * BOYUT * 4);
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  parca('IHDR', ihdr),
  parca('IDAT', zlib.deflateSync(satirlar, { level: 9 })),
  parca('IEND', Buffer.alloc(0)),
]);

const hedef = process.argv[2];
fs.writeFileSync(hedef, png);
console.log(`ikon yazildi: ${hedef} (${png.length} byte, ${BOYUT}x${BOYUT})`);
