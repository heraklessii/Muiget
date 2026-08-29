import { useCallback, useEffect, useRef, useState } from 'react';

export type ToastKind = 'success' | 'error' | 'info';

/**
 * Bildirimin yanındaki tek eylem düğmesi.
 *
 * Pano izleme ve güncelleme bildirimi için eklendi: "panoda bağlantı var"
 * demek, kullanıcıyı menüye göndermeden bir tıklık karşılık sunmadıkça
 * yarım kalıyor. Eylem seçilince bildirim kendiliğinden kapanıyor.
 */
export interface ToastAction {
  label: string;
  onSelect: () => void;
}

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  action?: ToastAction;
}

/** Bildirimin kendiliğinden kaybolma süresi. Hatalar daha uzun duruyor. */
const SURE: Record<ToastKind, number> = {
  success: 3500,
  info: 4000,
  error: 8000,
};

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const sonraki = useRef(1);
  // Bileşen sökülürken bekleyen zamanlayıcıları temizlemek için.
  const zamanlayicilar = useRef(new Set<number>());

  const dismiss = useCallback((id: number) => {
    setToasts((o) => o.filter((t) => t.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, message: string, action?: ToastAction) => {
      const id = sonraki.current++;
      setToasts((o) => [...o, { id, kind, message, action }]);

      // Eylemli bildirim daha uzun duruyor: kullanıcının okuyup karar vermesi
      // gerekiyor, 3.5 saniye bunun için yeterli değil.
      const sure = action ? Math.max(SURE[kind], 12_000) : SURE[kind];
      const zaman = window.setTimeout(() => {
        zamanlayicilar.current.delete(zaman);
        dismiss(id);
      }, sure);
      zamanlayicilar.current.add(zaman);
    },
    [dismiss],
  );

  useEffect(() => {
    const kayitli = zamanlayicilar.current;
    return () => {
      kayitli.forEach((z) => window.clearTimeout(z));
      kayitli.clear();
    };
  }, []);

  return { toasts, push, dismiss };
}

export function Toasts({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;

  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div className={`toast ${toast.kind}`} key={toast.id}>
          <span>{toast.message}</span>
          {toast.action && (
            <button
              className="toast__action"
              onClick={() => {
                onDismiss(toast.id);
                toast.action?.onSelect();
              }}
            >
              {toast.action.label}
            </button>
          )}
          <button onClick={() => onDismiss(toast.id)} aria-label="Bildirimi kapat">
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
