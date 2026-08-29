import { memo } from 'react';

import {
  fileExtension,
  formatBytes,
  formatDuration,
  formatPercent,
  formatSpeed,
} from '../lib/format';
import { isActive, isResumable, type DownloadSnapshot, type DownloadStatus } from '../lib/types';
import {
  IconFolder,
  IconPause,
  IconPlay,
  IconStop,
  IconTrash,
  IconWarning,
} from './Icons';

interface Props {
  download: DownloadSnapshot;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRemove: (id: string) => void;
  onReveal: (path: string) => void;
}

const DURUM_METNI: Record<DownloadStatus, string> = {
  queued: 'Sırada',
  probing: 'Sunucu yoklanıyor',
  running: 'İniyor',
  paused: 'Duraklatıldı',
  completed: 'Tamamlandı',
  failed: 'Başarısız',
  cancelled: 'İptal edildi',
};

const DURUM_SINIFI: Record<DownloadStatus, string> = {
  queued: 'queued',
  probing: '',
  running: 'running',
  paused: 'paused',
  completed: 'completed',
  failed: 'failed',
  cancelled: '',
};

/**
 * Tek bir indirme satırı.
 *
 * `memo` bilinçli: motor her yarım saniyede bir anlık görüntü yayınlıyor ve
 * 20 indirmenin hepsinin yeniden çizilmesi gereksiz. Yalnızca değişen satır
 * güncelleniyor.
 */
export const DownloadRow = memo(function DownloadRow({
  download,
  onPause,
  onResume,
  onCancel,
  onRemove,
  onReveal,
}: Props) {
  const oran = download.totalSize > 0 ? download.downloaded / download.totalSize : 0;
  const aktif = isActive(download.status);
  const uzanti = fileExtension(download.fileName);

  /**
   * "İniyor" ama henüz tek byte inmemiş: bağlantı bekleniyor demek.
   *
   * İki durumda oluyor ve ikisinde de doğru: (1) istek yeni gitti, sunucu
   * henüz yanıt vermedi; (2) aynı siteye açık bağlantı sayısı kotayı doldurmuş
   * ve bu indirmenin parçaları sıra bekliyor. İkisinde de "İniyor · %0 · —"
   * yazmak kullanıcıya takılmış izlenimi veriyordu.
   */
  const baglantiBekliyor =
    download.status === 'running' && download.downloaded === 0 && download.speed === 0;

  const durumMetni = baglantiBekliyor ? 'Bağlantı bekleniyor' : DURUM_METNI[download.status];

  return (
    <article className={`download is-${download.status}`}>
      <div className="download__head">
        <div className="download__icon" aria-hidden>
          {uzanti || '?'}
        </div>

        <div className="download__title">
          <div className="download__name" title={download.fileName}>
            {download.fileName}
          </div>
          <div className="download__meta">
            <span
              className={`badge ${baglantiBekliyor ? 'queued' : DURUM_SINIFI[download.status]}`}
            >
              {durumMetni}
            </span>
            {download.totalSize > 0 && (
              <span className="dot">
                {formatBytes(download.downloaded)} / {formatBytes(download.totalSize)}
              </span>
            )}
            {download.segments.length > 1 && (
              <span className="dot">{download.segments.length} parça</span>
            )}
          </div>
        </div>

        <div className="download__actions">
          {aktif && (
            <button
              className="button small icon ghost"
              onClick={() => onPause(download.id)}
              title="Duraklat"
              aria-label={`${download.fileName} indirmesini duraklat`}
            >
              <IconPause />
            </button>
          )}

          {isResumable(download.status) && (
            <button
              className="button small icon ghost"
              onClick={() => onResume(download.id)}
              title="Devam et"
              aria-label={`${download.fileName} indirmesine devam et`}
            >
              <IconPlay />
            </button>
          )}

          {aktif && (
            <button
              className="button small icon ghost"
              onClick={() => onCancel(download.id)}
              title="İptal et"
              aria-label={`${download.fileName} indirmesini iptal et`}
            >
              <IconStop />
            </button>
          )}

          {download.status === 'completed' && (
            <button
              className="button small icon ghost"
              onClick={() => onReveal(download.targetPath)}
              title="Klasörde göster"
              aria-label={`${download.fileName} dosyasını klasörde göster`}
            >
              <IconFolder />
            </button>
          )}

          <button
            className="button small icon ghost danger"
            onClick={() => onRemove(download.id)}
            title="Listeden kaldır"
            aria-label={`${download.fileName} indirmesini listeden kaldır`}
          >
            <IconTrash />
          </button>
        </div>
      </div>

      <div
        className="progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.floor(oran * 100)}
        aria-label={`${download.fileName} ilerlemesi`}
      >
        <div className="progress__fill" style={{ width: `${oran * 100}%` }} />
      </div>

      {/* Segment şeridi yalnızca çok parçalı ve devam eden indirmelerde:
          tek parçalı bir indirmede ilerleme çubuğunun tekrarı olurdu. */}
      {download.segments.length > 1 && download.status !== 'completed' && (
        <div className="segments" aria-hidden>
          {download.segments.map((segment) => {
            const doluluk = segment.total > 0 ? segment.downloaded / segment.total : 0;
            const bitti = segment.downloaded >= segment.total;
            return (
              <div
                key={segment.index}
                className={`segment ${bitti ? 'is-done' : segment.active ? 'is-active' : ''}`}
                title={`Parça ${segment.index + 1}: ${formatPercent(doluluk)} — ${formatSpeed(
                  segment.speed,
                )}`}
              >
                <div className="segment__fill" style={{ width: `${doluluk * 100}%` }} />
              </div>
            );
          })}
        </div>
      )}

      <div className="download__foot">
        <span>{formatPercent(oran)}</span>
        {/* Hız ve kalan süre yalnızca gerçekten akarken: "— / kalan —" yazmak
            bilgi vermiyor, yalnızca satırı gürültüyle dolduruyordu. */}
        {download.status === 'running' && !baglantiBekliyor && (
          <>
            <span>{formatSpeed(download.speed)}</span>
            <span>kalan {formatDuration(download.etaSeconds)}</span>
          </>
        )}
        <span className="download__path push" title={download.targetPath}>
          {download.targetPath}
        </span>
      </div>

      {download.warning && (
        <p className="download__warning">
          <IconWarning />
          <span>{download.warning}</span>
        </p>
      )}

      {download.error && <p className="download__error">{download.error}</p>}
    </article>
  );
});
