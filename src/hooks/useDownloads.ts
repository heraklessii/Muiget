import { useCallback, useEffect, useRef, useState } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { listDownloads, onProgress } from '../lib/api';
import type { DownloadSnapshot } from '../lib/types';

/**
 * İndirme listesini canlı tutar.
 *
 * Motor her tick'te **tek bir** indirmenin anlık görüntüsünü yayınlıyor, tüm
 * listeyi değil: 20 indirme varken hepsini yarım saniyede bir göndermek
 * gereksiz. Bu yüzden liste burada birleştiriliyor.
 *
 * Silme işleminin olayı yok (kayıt artık mevcut değil), o yüzden `refresh`
 * dışarıya açılıyor.
 */
export function useDownloads() {
  const [downloads, setDownloads] = useState<DownloadSnapshot[]>([]);
  const [loading, setLoading] = useState(true);

  // Bileşen sökülmüşken `setState` çağırmamak için.
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    const liste = await listDownloads();
    if (alive.current) setDownloads(liste);
  }, []);

  useEffect(() => {
    alive.current = true;
    let unlisten: UnlistenFn | undefined;

    void refresh().finally(() => {
      if (alive.current) setLoading(false);
    });

    void onProgress((snapshot) => {
      if (!alive.current) return;
      setDownloads((onceki) => birlestir(onceki, snapshot));
    }).then((fn) => {
      // Abonelik kurulurken bileşen sökülmüş olabilir; sızdırmadan kapat.
      if (alive.current) unlisten = fn;
      else fn();
    });

    return () => {
      alive.current = false;
      unlisten?.();
    };
  }, [refresh]);

  return { downloads, loading, refresh, setDownloads };
}

/** Gelen anlık görüntüyü listeye işler; yeni kayıtsa sona ekler. */
function birlestir(
  liste: DownloadSnapshot[],
  snapshot: DownloadSnapshot,
): DownloadSnapshot[] {
  const sira = liste.findIndex((d) => d.id === snapshot.id);
  if (sira === -1) return [...liste, snapshot];

  const kopya = liste.slice();
  kopya[sira] = snapshot;
  return kopya;
}
