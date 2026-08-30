import { useEffect, useMemo, useRef, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';

import { errorMessage, findDuplicate, probeMedia, probeUrl } from '../lib/api';
import { DURUM_METNI } from './DownloadRow';
import { formatBytes, formatDuration } from '../lib/format';
import {
  looksLikeStream,
  type DownloadSnapshot,
  type MediaInfo,
  type MediaSelection,
  type ServerCapabilities,
} from '../lib/types';
import { IconClose, IconFolder, IconWarning } from './Icons';

/** Ses parçası seçicisinde "hiç indirme" seçeneğinin değeri. */
const SESSIZ = '__ses-yok__';

interface Props {
  defaultDirectory: string;
  /** Sürükle-bırakla gelen adres. Verilirse kutu dolu açılıyor ve yoklama
      kullanıcı bir tuşa basmadan başlıyor. */
  initialUrl?: string;
  onClose: () => void;
  onStart: (url: string, directory: string) => Promise<void>;
  /** Birden çok adres yapıştırıldığında — tek yenileme turuyla hepsini başlatır. */
  onStartMany: (urls: string[], directory: string) => Promise<void>;
  /** Adres bir HLS/DASH manifesti çıktığında — kalite seçimiyle birlikte. */
  onStartMedia: (
    url: string,
    directory: string,
    selection: MediaSelection,
  ) => Promise<void>;
}

/**
 * Yeni indirme diyaloğu.
 *
 * URL yapıştırılınca sunucu **otomatik yoklanıyor**: kullanıcı indirmeye
 * başlamadan önce dosya adını, boyutunu ve çoklu bağlantının mümkün olup
 * olmadığını görüyor. Yoklama başarısız olsa bile indirme engellenmiyor —
 * bazı sunucular HEAD'e kapalı ama GET'e açık.
 */
export function AddDialog({
  defaultDirectory,
  initialUrl,
  onClose,
  onStart,
  onStartMany,
  onStartMedia,
}: Props) {
  const [url, setUrl] = useState(initialUrl ?? '');
  const [directory, setDirectory] = useState(defaultDirectory);
  const [caps, setCaps] = useState<ServerCapabilities | null>(null);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  /** Aynı adres listede varsa o kayıt (karar #22). Engel değil, uyarı. */
  const [kopya, setKopya] = useState<DownloadSnapshot | null>(null);
  /** Adres bir akış manifestiyse kalite seçenekleri (karar #25). */
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [videoId, setVideoId] = useState('');
  const [audioId, setAudioId] = useState('');

  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  /**
   * Kutuya birden çok adres yapıştırılmış olabilir (IDM'in "toplu indirme"si).
   *
   * `<input>` satır sonlarını boşluğa çevirdiği için ayırıcı olarak boşluk
   * yetiyor. Tek adreste davranış eskisi gibi: yoklama + önizleme. Birden
   * çoksa yoklama atlanıyor — on sunucuyu sırayla yoklamak kullanıcıyı
   * bekletirdi ve önizleme zaten tek dosyayı anlatmak için tasarlandı.
   */
  const adresler = useMemo(
    () =>
      url
        .split(/\s+/)
        .map((parca) => parca.trim())
        .filter((parca) => /^https?:\/\/\S+$/i.test(parca)),
    [url],
  );
  const toplu = adresler.length > 1;

  // Esc ile kapatma — diyalog açıkken tüm pencerede geçerli.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  // URL değişince yoklama — her tuş vuruşunda istek atmamak için gecikmeli.
  useEffect(() => {
    const kirpilmis = url.trim();
    if (toplu || !/^https?:\/\/\S+$/i.test(kirpilmis)) {
      setCaps(null);
      setMedia(null);
      setProbeError(null);
      setKopya(null);
      return;
    }

    let iptal = false;
    const zamanlayici = window.setTimeout(() => {
      setProbing(true);
      setProbeError(null);
      setMedia(null);

      // Kopya kontrolü yoklamadan bağımsız: sunucu yanıt vermese bile
      // "bu dosya zaten listede" bilgisi doğru ve gösterilmeye değer.
      findDuplicate(kirpilmis).then(
        (mevcut) => {
          if (!iptal) setKopya(mevcut);
        },
        () => {},
      );

      yokla(kirpilmis)
        .catch((e) => {
          if (!iptal) {
            setCaps(null);
            setMedia(null);
            setProbeError(errorMessage(e));
          }
        })
        .finally(() => {
          if (!iptal) setProbing(false);
        });

      /**
       * Adres bir dosya mı, bir akış manifesti mi?
       *
       * Uzantı yetmiyor — CDN'ler `.m3u8`i sorgunun arkasına saklıyor — bu
       * yüzden uzantı tutmazsa önce normal yoklama yapılıp `Content-Type`e
       * bakılıyor. Uzantı tutuyorsa doğrudan manifest okunuyor: `probe_url`
       * bir manifest için zaten anlamlı bir şey söylemiyor.
       */
      async function yokla(adres: string) {
        if (looksLikeStream(adres)) {
          const bilgi = await probeMedia(adres);
          if (iptal) return;
          akisiKur(bilgi);
          return;
        }

        const sonuc = await probeUrl(adres);
        if (iptal) return;
        setCaps(sonuc);

        const tur = (sonuc.contentType ?? '').toLowerCase();
        if (tur.includes('mpegurl') || tur.includes('dash+xml')) {
          const bilgi = await probeMedia(adres);
          if (!iptal) akisiKur(bilgi);
        }
      }

      function akisiKur(bilgi: MediaInfo) {
        setMedia(bilgi);
        // Varsayılan seçim motorun kendi tercihi: önizleme ile indirmenin
        // ayrışmaması için karar tek yerde (`media::describe`) veriliyor.
        setVideoId(bilgi.defaultVideo ?? bilgi.video[0]?.id ?? '');
        setAudioId(bilgi.defaultAudio ?? '');
      }
    }, 450);

    return () => {
      iptal = true;
      window.clearTimeout(zamanlayici);
    };
  }, [url, toplu]);

  const secilenVideo = media?.video.find((t) => t.id === videoId) ?? media?.video[0] ?? null;
  const sesSecilebilir = !!media && media.audio.length > 0 && secilenVideo?.kind === 'video';
  const sessiz = audioId === SESSIZ;
  /** Bu seçimle ffmpeg gerçekten gerekiyor mu? */
  const ffmpegGerekli = sesSecilebilir && !sessiz;
  const ffmpegEksik = ffmpegGerekli && !media?.ffmpeg;

  /**
   * Seçilen kaliteye göre boyut tahmini.
   *
   * Sunucudan gelen tahmin varsayılan seçim için; kullanıcı kaliteyi
   * değiştirince rakamın olduğu yerde kalması yanlış bilgi olurdu.
   */
  const tahminiBoyut = useMemo(() => {
    if (!media?.durationSeconds || !secilenVideo) return media?.estimatedSize ?? 0;
    const ses = ffmpegGerekli
      ? (media.audio.find((t) => t.id === audioId)?.bandwidth ?? 0)
      : 0;
    return Math.round(((secilenVideo.bandwidth + ses) * media.durationSeconds) / 8);
  }, [media, secilenVideo, audioId, ffmpegGerekli]);

  async function klasorSec() {
    const secilen = await openDialog({ directory: true, defaultPath: directory });
    if (typeof secilen === 'string') setDirectory(secilen);
  }

  async function basla() {
    if (adresler.length === 0) return;

    setStarting(true);
    try {
      if (toplu) {
        await onStartMany(adresler, directory);
      } else if (media) {
        await onStartMedia(adresler[0], directory, {
          video: videoId || null,
          audio: sessiz ? null : audioId || null,
          videoOnly: sessiz,
        });
      } else {
        await onStart(adresler[0], directory);
      }
    } finally {
      setStarting(false);
    }
  }

  // Canlı yayın kaydedilemiyor, ffmpeg'siz birleştirme de yapılamıyor: ikisinde
  // de düğmeyi kapatıp sebebini yazmak, indirmeyi başlatıp saniyeler sonra
  // hata göstermekten dürüst.
  const gecerli = adresler.length > 0 && !media?.live && !ffmpegEksik;

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal aria-label="Yeni indirme">
        <div className="dialog__head">
          <h2>Yeni indirme</h2>
          <button className="button icon ghost" onClick={onClose} aria-label="Kapat">
            <IconClose />
          </button>
        </div>

        <div className="dialog__body">
          <label className="field">
            <span>Bağlantı adresi</span>
            <input
              ref={inputRef}
              className="text-input"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && gecerli && !starting) void basla();
              }}
              placeholder="https://ornek.com/dosya.zip — birden çok adresi boşlukla ayırın"
              spellCheck={false}
              autoComplete="off"
            />
          </label>

          <label className="field">
            <span>Hedef klasör</span>
            <div className="row">
              <input
                className="text-input grow"
                value={directory}
                onChange={(e) => setDirectory(e.target.value)}
                spellCheck={false}
              />
              <button className="button icon" onClick={klasorSec} title="Klasör seç">
                <IconFolder />
              </button>
            </div>
          </label>

          {toplu && (
            <p className="field-hint">
              <strong>{adresler.length} bağlantı</strong> bulundu. Hepsi kuyruğa alınacak;
              eşzamanlı indirme sınırı ayarlardan geliyor. Toplu eklemede sunucu
              önizlemesi gösterilmiyor.
            </p>
          )}

          {kopya && (
            <p className="field-hint" style={{ color: 'var(--warning)' }}>
              Bu adres listede zaten var: <strong>{kopya.fileName}</strong> (
              {DURUM_METNI[kopya.status].toLocaleLowerCase('tr')}). Yine de
              indirebilirsiniz; dosya adı çakışırsa yanına numara eklenir.
            </p>
          )}

          {probing && <p className="field-hint">Sunucu yoklanıyor…</p>}

          {probeError && (
            <p className="field-hint" style={{ color: 'var(--warning)' }}>
              Sunucu yoklanamadı: {probeError}
              <br />
              Yine de indirmeyi deneyebilirsiniz; bazı sunucular yalnızca indirme
              isteğine yanıt veriyor.
            </p>
          )}

          {media && (
            <div className="media-panel">
              <div className="media-panel__head">
                <span className="badge running">{media.protocol.toUpperCase()} video</span>
                {media.durationSeconds !== null && (
                  <span className="dot">{formatDuration(media.durationSeconds)}</span>
                )}
                {tahminiBoyut > 0 && <span className="dot">~{formatBytes(tahminiBoyut)}</span>}
              </div>

              {media.live ? (
                <p className="field-hint" style={{ color: 'var(--warning)' }}>
                  <IconWarning /> Bu bir <strong>canlı yayın</strong>. Muiget canlı
                  yayın kaydetmiyor — akışın sonu belli olmadığı için indirmenin de
                  sonu olmazdı.
                </p>
              ) : (
                <>
                  <label className="field">
                    <span>Kalite</span>
                    <select
                      className="select"
                      value={videoId}
                      onChange={(e) => setVideoId(e.target.value)}
                    >
                      {media.video.map((parca) => (
                        <option key={parca.id} value={parca.id}>
                          {parca.label}
                        </option>
                      ))}
                    </select>
                  </label>

                  {sesSecilebilir && (
                    <label className="field">
                      <span>Ses</span>
                      <select
                      className="select"
                        value={audioId}
                        onChange={(e) => setAudioId(e.target.value)}
                      >
                        {media.audio.map((parca) => (
                          <option key={parca.id} value={parca.id}>
                            {parca.label}
                          </option>
                        ))}
                        <option value={SESSIZ}>Ses indirme — yalnızca görüntü</option>
                      </select>
                    </label>
                  )}

                  {ffmpegEksik && (
                    <p className="field-hint" style={{ color: 'var(--warning)' }}>
                      <IconWarning /> Bu yayında ses ve görüntü <strong>ayrı</strong>{' '}
                      iniyor; tek dosyada birleştirmek için <strong>ffmpeg</strong>{' '}
                      gerekiyor ve bulunamadı. ffmpeg kurup Ayarlar → ffmpeg yolunu
                      gösterebilir ya da yukarıdan “yalnızca görüntü”yü seçebilirsin.
                    </p>
                  )}

                  {ffmpegGerekli && media.ffmpeg && (
                    <p className="field-hint">
                      Ses ve görüntü ayrı iniyor, sonunda ffmpeg ile tek `.mp4`
                      dosyasında birleştirilecek. Yeniden kodlama yok.
                    </p>
                  )}

                  {!media.ffmpeg && !ffmpegGerekli && (
                    <p className="field-hint">
                      ffmpeg bulunamadı; dosya parçaların kabıyla (`.ts` ya da
                      `.mp4`) kaydedilecek. Çoğu oynatıcı açar.
                    </p>
                  )}
                </>
              )}
            </div>
          )}

          {caps && !media && (
            <dl className="preview">
              <dt>Dosya adı</dt>
              <dd>{caps.fileName}</dd>

              <dt>Boyut</dt>
              <dd>
                {caps.contentLength !== null ? formatBytes(caps.contentLength) : 'bilinmiyor'}
              </dd>

              <dt>Çoklu bağlantı</dt>
              <dd className={caps.supportsRanges ? 'yes' : 'no'}>
                {caps.supportsRanges
                  ? 'destekleniyor — paralel inecek'
                  : 'desteklenmiyor — tek bağlantı'}
              </dd>

              {caps.contentType && (
                <>
                  <dt>Tür</dt>
                  <dd className="mono">{caps.contentType}</dd>
                </>
              )}
            </dl>
          )}
        </div>

        <div className="dialog__foot">
          <button className="button ghost" onClick={onClose}>
            Vazgeç
          </button>
          <button className="button primary" onClick={basla} disabled={!gecerli || starting}>
            {starting
              ? 'Başlatılıyor…'
              : toplu
                ? `${adresler.length} dosyayı indir`
                : media
                  ? 'Videoyu indir'
                  : 'İndir'}
          </button>
        </div>
      </div>
    </div>
  );
}
