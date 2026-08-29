import { useEffect, useRef, useState } from 'react';

import { formatSpeed } from '../lib/format';

/** Kaç örnek gösterilsin. 60 örnek × 1 saniye = son bir dakika. */
const PENCERE = 60;

/**
 * Durum çubuğundaki canlı hız grafiği.
 *
 * Örnekleme burada yapılıyor, motorun ilerleme yayınından değil: yayın her
 * yarım saniyede bir ve indirme başına geliyor, grafiğin ise sabit aralıklı
 * tek bir seriye ihtiyacı var. Kendi zamanlayıcısıyla örneklemek grafiğin
 * yatay eksenini gerçekten zamana bağlıyor.
 */
export function SpeedGraph({ speed }: { speed: number }) {
  const [ornekler, setOrnekler] = useState<number[]>(() => new Array(PENCERE).fill(0));

  // Zamanlayıcı her saniye en güncel hızı okumalı; `speed`i bağımlılığa
  // koymak zamanlayıcıyı her yenilemede sıfırlardı.
  const guncelHiz = useRef(speed);
  guncelHiz.current = speed;

  useEffect(() => {
    const zaman = window.setInterval(() => {
      setOrnekler((onceki) => [...onceki.slice(1), guncelHiz.current]);
    }, 1000);
    return () => window.clearInterval(zaman);
  }, []);

  const tepe = Math.max(...ornekler, 1);
  // Hepsi sıfırsa düz çizgi çizmek yerine grafiği gizlemek daha sakin.
  const bosDurum = tepe <= 1;

  const genislik = 108;
  const yukseklik = 18;
  const adim = genislik / (PENCERE - 1);

  const noktalar = ornekler
    .map((deger, i) => {
      const x = i * adim;
      const y = yukseklik - (deger / tepe) * (yukseklik - 2) - 1;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');

  return (
    <span
      className="speed-graph"
      title={`Son bir dakika — tepe ${formatSpeed(tepe)}`}
      aria-hidden
    >
      <svg width={genislik} height={yukseklik} viewBox={`0 0 ${genislik} ${yukseklik}`}>
        {!bosDurum && (
          <>
            <polyline
              points={`0,${yukseklik} ${noktalar} ${genislik},${yukseklik}`}
              className="speed-graph__area"
            />
            <polyline points={noktalar} className="speed-graph__line" />
          </>
        )}
      </svg>
    </span>
  );
}
