/**
 * Satır içi SVG ikonlar.
 *
 * İkon kütüphanesi yerine elle yazıldı: uygulamanın ihtiyacı sekiz ikon ve
 * bunun için birkaç yüz kilobaytlık bir paket bağımlılığı taşımak — hele
 * çevrimdışı çalışması gereken bir uygulamada — orantısız.
 *
 * Hepsi `currentColor` kullanıyor, yani renk CSS'ten geliyor.
 */

type IconProps = { className?: string };

const ortak = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
};

export function IconDownload({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M12 3v12" />
      <path d="m7 11 5 5 5-5" />
      <path d="M4 20h16" />
    </svg>
  );
}

export function IconPlus({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function IconPause({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M9 5v14M15 5v14" />
    </svg>
  );
}

export function IconPlay({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M7 4.5v15l13-7.5z" />
    </svg>
  );
}

export function IconStop({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );
}

export function IconTrash({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M4 7h16" />
      <path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
      <path d="M6 7v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" />
      <path d="M10 11v6M14 11v6" />
    </svg>
  );
}

export function IconFolder({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

export function IconSettings({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 9 19.4a1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </svg>
  );
}

export function IconSun({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </svg>
  );
}

export function IconMoon({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
    </svg>
  );
}

export function IconWarning({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" />
      <path d="M12 9v4M12 17h.01" />
    </svg>
  );
}

export function IconSearch({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  );
}

export function IconClose({ className }: IconProps) {
  return (
    <svg {...ortak} className={className}>
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  );
}
