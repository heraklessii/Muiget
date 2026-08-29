import { useEffect, useRef, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';

import { errorMessage, probeUrl } from '../lib/api';
import { formatBytes } from '../lib/format';
import type { ServerCapabilities } from '../lib/types';
import { IconClose, IconFolder } from './Icons';

interface Props {
  defaultDirectory: string;
  onClose: () => void;
  onStart: (url: string, directory: string) => Promise<void>;
}

/**
 * Yeni indirme diyaloğu.
 *
 * URL yapıştırılınca sunucu **otomatik yoklanıyor**: kullanıcı indirmeye
 * başlamadan önce dosya adını, boyutunu ve çoklu bağlantının mümkün olup
 * olmadığını görüyor. Yoklama başarısız olsa bile indirme engellenmiyor —
 * bazı sunucular HEAD'e kapalı ama GET'e açık.
 */
export function AddDialog({ defaultDirectory, onClose, onStart }: Props) {
  const [url, setUrl] = useState('');
  const [directory, setDirectory] = useState(defaultDirectory);
  const [caps, setCaps] = useState<ServerCapabilities | null>(null);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

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
    if (!/^https?:\/\/\S+$/i.test(kirpilmis)) {
      setCaps(null);
      setProbeError(null);
      return;
    }

    let iptal = false;
    const zamanlayici = window.setTimeout(() => {
      setProbing(true);
      setProbeError(null);

      probeUrl(kirpilmis)
        .then((sonuc) => {
          if (!iptal) setCaps(sonuc);
        })
        .catch((e) => {
          if (!iptal) {
            setCaps(null);
            setProbeError(errorMessage(e));
          }
        })
        .finally(() => {
          if (!iptal) setProbing(false);
        });
    }, 450);

    return () => {
      iptal = true;
      window.clearTimeout(zamanlayici);
    };
  }, [url]);

  async function klasorSec() {
    const secilen = await openDialog({ directory: true, defaultPath: directory });
    if (typeof secilen === 'string') setDirectory(secilen);
  }

  async function basla() {
    const kirpilmis = url.trim();
    if (!kirpilmis) return;

    setStarting(true);
    try {
      await onStart(kirpilmis, directory);
    } finally {
      setStarting(false);
    }
  }

  const gecerli = /^https?:\/\/\S+$/i.test(url.trim());

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
              placeholder="https://ornek.com/dosya.zip"
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

          {probing && <p className="field-hint">Sunucu yoklanıyor…</p>}

          {probeError && (
            <p className="field-hint" style={{ color: 'var(--warning)' }}>
              Sunucu yoklanamadı: {probeError}
              <br />
              Yine de indirmeyi deneyebilirsiniz; bazı sunucular yalnızca indirme
              isteğine yanıt veriyor.
            </p>
          )}

          {caps && (
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
            {starting ? 'Başlatılıyor…' : 'İndir'}
          </button>
        </div>
      </div>
    </div>
  );
}
