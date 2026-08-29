import { useCallback, useEffect, useRef, useState } from 'react';

export type ToastKind = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
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
    (kind: ToastKind, message: string) => {
      const id = sonraki.current++;
      setToasts((o) => [...o, { id, kind, message }]);

      const zaman = window.setTimeout(() => {
        zamanlayicilar.current.delete(zaman);
        dismiss(id);
      }, SURE[kind]);
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
          <button onClick={() => onDismiss(toast.id)} aria-label="Bildirimi kapat">
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
