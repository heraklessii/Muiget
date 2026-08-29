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
  return status === 'queued' || status === 'probing' || status === 'running';
}

export function isResumable(status: DownloadStatus): boolean {
  return status === 'paused' || status === 'failed';
}
