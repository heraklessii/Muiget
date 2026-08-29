/**
 * Sayı ve süre biçimlendirme.
 *
 * Hepsi saf fonksiyon: bileşenlerde `toFixed` dağılmasın ve birim/dil kararı
 * tek yerde kalsın diye burada toplandı.
 */

const BIRIMLER = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** 1536 → "1,5 KB" */
export function formatBytes(bytes: number, ondalik = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';

  let deger = bytes;
  let birim = 0;
  while (deger >= 1024 && birim < BIRIMLER.length - 1) {
    deger /= 1024;
    birim += 1;
  }

  // Byte'ta ondalık anlamsız: "512,0 B" değil "512 B".
  const basamak = birim === 0 ? 0 : ondalik;
  return `${deger.toLocaleString('tr-TR', {
    minimumFractionDigits: basamak,
    maximumFractionDigits: basamak,
  })} ${BIRIMLER[birim]}`;
}

/** 1048576 → "1,0 MB/s" */
export function formatSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 1) return '—';
  return `${formatBytes(bytesPerSecond)}/s`;
}

/**
 * Saniyeyi okunur süreye çevirir.
 *
 * Kaba ama dürüst: 90 dakikalık bir tahminde saniye hassasiyeti göstermek
 * gerçekte olmayan bir kesinlik iddiası olurdu.
 */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '—';
  if (seconds < 1) return '1 sn';

  const sn = Math.round(seconds);
  if (sn < 60) return `${sn} sn`;

  const dk = Math.floor(sn / 60);
  if (dk < 60) return `${dk} dk ${sn % 60} sn`;

  const sa = Math.floor(dk / 60);
  if (sa < 24) return `${sa} sa ${dk % 60} dk`;

  return `${Math.floor(sa / 24)} gün ${sa % 24} sa`;
}

/** 0.4237 → "%42" */
export function formatPercent(oran: number): string {
  if (!Number.isFinite(oran)) return '%0';
  return `%${Math.floor(Math.min(Math.max(oran, 0), 1) * 100)}`;
}

/** Gün içi dakikayı saat biçimine çevirir: 135 → "02:15" */
export function minuteToClock(minute: number): string {
  const m = Math.min(Math.max(Math.round(minute), 0), 1440);
  const sa = Math.floor(m / 60) % 24;
  const dk = m % 60;
  return `${String(sa).padStart(2, '0')}:${String(dk).padStart(2, '0')}`;
}

/** "02:15" → 135. Geçersiz girdide `null`. */
export function clockToMinute(clock: string): number | null {
  const eslesme = /^(\d{1,2}):(\d{2})$/.exec(clock.trim());
  if (!eslesme) return null;

  const sa = Number(eslesme[1]);
  const dk = Number(eslesme[2]);
  if (sa > 24 || dk > 59) return null;

  return Math.min(sa * 60 + dk, 1440);
}

/**
 * Dosya adından uzantı rozetini üretir: "film.tar.gz" → "GZ".
 * Uzantı yoksa ya da anlamsız uzunluktaysa boş döner.
 */
export function fileExtension(fileName: string): string {
  const nokta = fileName.lastIndexOf('.');
  if (nokta <= 0 || nokta === fileName.length - 1) return '';

  const uzanti = fileName.slice(nokta + 1);
  return uzanti.length <= 4 ? uzanti.toUpperCase() : '';
}

/** Hız sınırı girdisi: kullanıcı MB/s yazıyor, motor byte/s bekliyor. */
export function mbpsToBytes(mbps: number): number {
  if (!Number.isFinite(mbps) || mbps <= 0) return 0;
  return Math.round(mbps * 1024 * 1024);
}

export function bytesToMbps(bytes: number): number {
  if (!Number.isFinite(bytes) || bytes <= 0) return 0;
  return Math.round((bytes / (1024 * 1024)) * 100) / 100;
}
