/**
 * Backend ile paylaşılan tipler.
 *
 * Rust tarafındaki karşılıkları `serde(rename_all = "camelCase")` ile
 * serileştiriliyor; buradaki alan adları onlarla birebir aynı olmak zorunda.
 * İlgili Rust dosyaları: `src-tauri/src/download/manager.rs`,
 * `src-tauri/src/settings.rs`.
 */

export type DownloadStatus =
  | 'queued'
  | 'probing'
  | 'running'
  /** Yalnızca akış indirmelerinde: parçalar indi, ffmpeg birleştiriyor. */
  | 'merging'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'cancelled';

export interface SegmentSnapshot {
  index: number;
  start: number;
  end: number;
  downloaded: number;
  total: number;
  speed: number;
  active: boolean;
}

/** Akış protokolü. Rust tarafında `media::Protocol`. */
export type MediaProtocol = 'hls' | 'dash';

/**
 * Bir akış indirmesinin ilerlemesi.
 *
 * Akışta ilerleme parça sayısıyla ölçülüyor: toplam boyut ancak son parça
 * inince kesinleşiyor, o zamana kadar `estimated` doğru.
 */
export interface MediaProgress {
  /** `HLS` ya da `DASH`. */
  protocol: string;
  /** Kalite etiketi (`1920x1080 · 5.0 Mbps`). */
  label: string | null;
  segmentsDone: number;
  segmentsTotal: number;
  estimated: boolean;
  /** Ses ayrı iniyor; sonunda ffmpeg birleştirecek. */
  merging: boolean;
}

/** Manifestteki bir kalite ya da ses seçeneği. */
export interface TrackInfo {
  id: string;
  kind: 'muxed' | 'video' | 'audio';
  /** Hazır etiket — arayüzün yeniden biçimlendirmesi gerekmiyor. */
  label: string;
  width: number | null;
  height: number | null;
  bandwidth: number;
  codecs: string | null;
  language: string | null;
  name: string | null;
}

export interface FfmpegInfo {
  path: string;
  version: string;
}

/** `probe_media` sonucu — yeni indirme penceresinin gösterdiği her şey. */
export interface MediaInfo {
  protocol: MediaProtocol;
  live: boolean;
  durationSeconds: number | null;
  video: TrackInfo[];
  audio: TrackInfo[];
  /**
   * İndirilebilir altyazılar. fMP4'e sarılmış (`wvtt`/`stpp`) olanlar burada
   * görünmüyor: listede olup inmemeleri kullanıcıya yalan söylemek olurdu.
   */
  subtitles: TrackInfo[];
  /** Ayarlardaki tercihle seçilecek parçalar — diyalog bunları işaretliyor. */
  defaultVideo: string | null;
  defaultAudio: string | null;
  /** Varsayılan seçimde ses ayrı iniyor mu — yani ffmpeg şart mı? */
  requiresFfmpeg: boolean;
  ffmpeg: FfmpegInfo | null;
  suggestedFileName: string;
  /** Bant genişliği × süre. Gerçek boyut değil. */
  estimatedSize: number;
}

/** Kullanıcının kalite/ses seçimi. Boş alanlar ayarlardaki tercihe düşüyor. */
export interface MediaSelection {
  video?: string | null;
  audio?: string | null;
  /** Sesi hiç indirme — ffmpeg yokken tek çıkış yolu. */
  videoOnly?: boolean;
}

export interface DownloadSnapshot {
  id: string;
  url: string;
  fileName: string;
  targetPath: string;
  status: DownloadStatus;
  totalSize: number;
  downloaded: number;
  speed: number;
  etaSeconds: number | null;
  segments: SegmentSnapshot[];
  error: string | null;
  warning: string | null;
  supportsRanges: boolean;
  /** Doluysa bu bir akış (HLS/DASH) indirmesi. */
  media: MediaProgress | null;
  createdAt: number;
  completedAt: number | null;
}

export interface ServerCapabilities {
  finalUrl: string;
  supportsRanges: boolean;
  contentLength: number | null;
  etag: string | null;
  lastModified: string | null;
  fileName: string;
  contentType: string | null;
}

export interface BandwidthRule {
  startMinute: number;
  endMinute: number;
  /** Byte/saniye. 0 = bu aralıkta sınırsız. */
  limitBytes: number;
  enabled: boolean;
}

export interface UpdateInfo {
  current: string;
  latest: string;
  /** Yayının GitHub sayfası. */
  url: string;
  available: boolean;
}

export interface ManagerConfig {
  segments: number;
  minSegmentSize: number;
  maxRetries: number;
  adaptive: boolean;
  minStealSize: number;
  maxConnectionsPerHost: number;
  /** Aynı anda kaç indirme çalışsın. 0 = sınırsız; fazlası kuyrukta bekler. */
  maxConcurrentDownloads: number;
  /** İnen dosyayı türüne göre alt klasöre koy. */
  categorize: boolean;
  globalSpeedLimit: number;
  bandwidthRules: BandwidthRule[];
  userAgent: string;
  connectTimeoutSecs: number;
  readTimeoutSecs: number;
  /** Vekil sunucu. Boş = doğrudan bağlantı. `http://`, `socks5://`… */
  proxy: string;
  /** ffmpeg yolu. Boşsa uygulamanın yanına, sonra `PATH`e bakılıyor. */
  ffmpegPath: string;
  /** Varsayılan kalite: `best` | `worst` | `1080` | `720`… */
  mediaQuality: string;
  /** Ses dili tercihi (`tr`, `en`). Boşsa manifestin varsayılanı. */
  mediaLanguage: string;
  /** Aynı anda kaç video parçası insin. */
  mediaConcurrency: number;
  /** `auto` | `all` | `off` — bkz. `docs/decisions.md` #29. */
  mediaSubtitles: string;
}

export interface AppSettings {
  downloadDir: string;
  theme: 'dark' | 'light';
  minimizeToTray: boolean;
  notifyOnComplete: boolean;
  /** Açılışta diskten geri yüklenen yarım indirmeler kendiliğinden sürsün mü. */
  resumeOnStart: boolean;
  /** Panoya kopyalanan bağlantı yakalansın mı. Varsayılan kapalı. */
  clipboardWatch: boolean;
  /** Açılışta yeni sürüm var mı diye bakılsın mı. */
  checkUpdates: boolean;
  engine: ManagerConfig;
  /** Native messaging köprüsünü kullanabilecek Chrome uzantısı kimlikleri. */
  extensionIds: string[];
}

/** Bir indirmenin hâlâ iş yapıyor olup olmadığı. */
export function isActive(status: DownloadStatus): boolean {
  return (
    status === 'queued' ||
    status === 'probing' ||
    status === 'running' ||
    status === 'merging'
  );
}

/**
 * Adres bir akış manifestine benziyor mu?
 *
 * Yalnızca ön eleme: kesin karar Rust tarafında, içerik türüne de bakılarak
 * veriliyor (`media::detect`). Burada tek amaç, `.m3u8`/`.mpd` yazan kullanıcıya
 * kalite seçeneklerini gösterecek ikinci bir istek atmak.
 */
export function looksLikeStream(url: string): boolean {
  const yol = url.split(/[?#]/)[0].toLowerCase();
  return yol.endsWith('.m3u8') || yol.endsWith('.m3u') || yol.endsWith('.mpd');
}

export function isResumable(status: DownloadStatus): boolean {
  return status === 'paused' || status === 'failed';
}
