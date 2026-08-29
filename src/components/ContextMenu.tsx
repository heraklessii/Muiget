import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  /** Kırmızı gösterilir — geri alınamayan eylemler için. */
  danger?: boolean;
  /** Üstüne ayırıcı çizgi koyar. */
  separated?: boolean;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/**
 * Fare imlecinin yanında açılan bağlam menüsü.
 *
 * Konum `position: fixed` ile veriliyor ve açıldıktan sonra ölçülüp
 * pencereye sığacak şekilde düzeltiliyor: sağ alt köşeye tıklandığında menü
 * ekran dışında kalırdı. Ölçüm `useLayoutEffect` içinde, yani kullanıcı
 * menüyü yanlış yerde bir kare bile görmüyor.
 */
export function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [konum, setKonum] = useState({ x, y });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;

    const kutu = el.getBoundingClientRect();
    const pay = 8;
    const solaKay = x + kutu.width + pay > window.innerWidth;
    const yukariKay = y + kutu.height + pay > window.innerHeight;

    setKonum({
      x: solaKay ? Math.max(pay, x - kutu.width) : x,
      y: yukariKay ? Math.max(pay, y - kutu.height) : y,
    });
  }, [x, y]);

  // Menü, dışarıya tıklayınca / Esc'e basınca / pencere kayınca kapanıyor.
  // Kaydırmada kapatmak bilinçli: menü sabit konumda, liste altından kayarsa
  // hangi satıra ait olduğu belirsizleşirdi.
  useEffect(() => {
    const kapat = () => onClose();
    const tusla = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };

    window.addEventListener('mousedown', kapat);
    window.addEventListener('resize', kapat);
    window.addEventListener('blur', kapat);
    window.addEventListener('scroll', kapat, true);
    window.addEventListener('keydown', tusla, true);

    return () => {
      window.removeEventListener('mousedown', kapat);
      window.removeEventListener('resize', kapat);
      window.removeEventListener('blur', kapat);
      window.removeEventListener('scroll', kapat, true);
      window.removeEventListener('keydown', tusla, true);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="context-menu"
      style={{ left: konum.x, top: konum.y }}
      role="menu"
      // Menünün kendi içindeki basış dışarı sayılmasın.
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, i) => (
        <button
          key={i}
          className={`context-menu__item${item.danger ? ' danger' : ''}${
            item.separated ? ' separated' : ''
          }`}
          role="menuitem"
          onClick={() => {
            onClose();
            item.onSelect();
          }}
        >
          <span className="context-menu__icon" aria-hidden>
            {item.icon}
          </span>
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}
