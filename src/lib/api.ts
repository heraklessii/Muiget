/**
 * Backend köprüsü — `invoke` çağrılarının tek toplandığı yer.
 *
 * Bileşenler `invoke('start_download', ...)` gibi dizeler taşımıyor; komut
 * adları burada bir kez yazılıyor ve tipler `types.ts`'ten geliyor. Komut adı
 * değişirse tek dosya güncelleniyor.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppSettings,
  DownloadSnapshot,
  FfmpegInfo,
  ManagerConfig,
  MediaInfo,
  MediaSelection,
  ServerCapabilities,
  UpdateInfo,
} from './types';

/** `src-tauri/src/lib.rs` → `PROGRESS_EVENT` ile aynı olmak zorunda. */
export const PROGRESS_EVENT = 'muiget://progress';

/** `src-tauri/src/lib.rs` → `CLIPBOARD_EVENT`. Yük: yakalanan adres. */
export const CLIPBOARD_EVENT = 'muiget://clipboard';

export function appVersion(): Promise<string> {
  return invoke<string>('app_version');
}

export function probeUrl(url: string): Promise<ServerCapabilities> {
  return invoke<ServerCapabilities>('probe_url', { url });
}

export function startDownload(url: string, directory?: string): Promise<string> {
  return invoke<string>('start_download', { url, directory: directory ?? null });
}

export function listDownloads(): Promise<DownloadSnapshot[]> {
  return invoke<DownloadSnapshot[]>('list_downloads');
}

/**
 * Akış manifestini (m3u8 / mpd) okuyup kalite seçeneklerini döner.
 *
 * `probeUrl`den ayrı: orası sunucu yeteneklerini soruyor, burada manifest
 * indirilip ayrıştırılıyor ve ffmpeg'in varlığı da kontrol ediliyor.
 */
export function probeMedia(url: string): Promise<MediaInfo> {
  return invoke<MediaInfo>('probe_media', { url });
}

/** Akış indirmesini kullanıcının kalite/ses seçimiyle başlatır. */
export function startMediaDownload(
  url: string,
  options: { directory?: string; selection?: MediaSelection; fileName?: string } = {},
): Promise<string> {
  return invoke<string>('start_media_download', {
    url,
    directory: options.directory ?? null,
    selection: options.selection ?? null,
    fileName: options.fileName ?? null,
  });
}

/**
 * ffmpeg bulunuyor mu? `path` verilirse yalnızca o deneniyor — ayarlarda
 * yazılan yolu kaydetmeden sınamak için.
 */
export function ffmpegStatus(path?: string): Promise<FfmpegInfo | null> {
  return invoke<FfmpegInfo | null>('ffmpeg_status', { path: path ?? null });
}

/**
 * İndirme klasörünü tarar, listede olmayan yarım indirmeleri geri yükler.
 * Dönen sayı yeni eklenen kayıt adedi. Açılışta zaten bir kez çalışıyor.
 */
export function rescanDownloads(directory?: string): Promise<number> {
  return invoke<number>('rescan_downloads', { directory: directory ?? null });
}

export function pauseDownload(id: string): Promise<void> {
  return invoke('pause_download', { id });
}

export function resumeDownload(id: string): Promise<void> {
  return invoke('resume_download', { id });
}

export function cancelDownload(id: string): Promise<void> {
  return invoke('cancel_download', { id });
}

/** Çalışan ve kuyrukta bekleyen tüm indirmeleri duraklatır; etkilenen sayı. */
export function pauseAllDownloads(): Promise<number> {
  return invoke<number>('pause_all_downloads');
}

/** Duraklatılmış ve başarısız tüm indirmeleri kuyruğa alır; etkilenen sayı. */
export function resumeAllDownloads(): Promise<number> {
  return invoke<number>('resume_all_downloads');
}

export function removeDownload(id: string, deleteFiles: boolean): Promise<void> {
  return invoke('remove_download', { id, deleteFiles });
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings');
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return invoke('save_settings', { settings });
}

export function effectiveSpeedLimit(): Promise<number> {
  return invoke<number>('effective_speed_limit');
}

export function engineDefaults(): Promise<ManagerConfig> {
  return invoke<ManagerConfig>('engine_defaults');
}

export function installNativeHost(extensionIds: string[]): Promise<string> {
  return invoke<string>('install_native_host', { extensionIds });
}

export function revealInFolder(path: string): Promise<void> {
  return invoke('reveal_in_folder', { path });
}

/**
 * Tamamlanmış bir indirmenin özetini hesaplar (küçük harfli hex).
 * Büyük dosyada saniyeler sürebilir.
 */
export function fileChecksum(id: string, algorithm: 'sha256' | 'md5' = 'sha256'): Promise<string> {
  return invoke<string>('file_checksum', { id, algorithm });
}

/** Bu adres listede zaten var mı? Varsa mevcut kayıt döner. */
export function findDuplicate(url: string): Promise<DownloadSnapshot | null> {
  return invoke<DownloadSnapshot | null>('find_duplicate', { url });
}

/** GitHub'daki son yayına bakar. Ağ hatasında reddediyor; arayüz yutuyor. */
export function checkForUpdate(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>('check_for_update');
}

/** Adresi kullanıcının tarayıcısında açar (yalnızca `https://`). */
export function openExternal(url: string): Promise<void> {
  return invoke('open_external', { url });
}

/** Panoda yakalanan bağlantıları dinler. Dönen fonksiyon aboneliği bitirir. */
export function onClipboardLink(handler: (url: string) => void): Promise<UnlistenFn> {
  return listen<string>(CLIPBOARD_EVENT, (event) => handler(event.payload));
}

/** Motorun ilerleme yayınına abone olur. Dönen fonksiyon aboneliği bitirir. */
export function onProgress(
  handler: (snapshot: DownloadSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<DownloadSnapshot>(PROGRESS_EVENT, (event) => handler(event.payload));
}

/**
 * Tauri hataları `Err(String)` olarak geliyor ama `unknown` tipinde.
 * Arayüzde gösterilebilir bir metne çevirir.
 */
export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
