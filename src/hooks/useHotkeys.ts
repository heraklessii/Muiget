import { useEffect, useRef } from 'react';

/**
 * Tek bir klavye kısayolu.
 *
 * `ctrl` Windows/Linux'ta Ctrl, macOS'ta Cmd anlamına geliyor: masaüstü
 * uygulamalarında beklenen davranış bu ve Tauri her iki platformda da aynı
 * pencereyi çalıştırıyor.
 */
export interface Hotkey {
  /** Küçük harf tuş adı: 'n', 'f', ',', '/'. */
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  handler: () => void;
}

/**
 * Pencere düzeyinde klavye kısayolları bağlar.
 *
 * Yazarken tetiklenmiyor: kullanıcı arama kutusuna "n" yazdığında yeni indirme
 * diyaloğu açılsaydı kutu kullanılamaz olurdu. Ctrl'lü kısayollar bu kuralın
 * dışında — `Ctrl+F` metin alanındayken de aramaya odaklanmalı.
 *
 * `etkin` false iken hiçbir şey dinlenmiyor: diyalog açıkken arkadaki kısayollar
 * çalışmamalı, yoksa Esc ile kapatılan diyaloğun üstüne bir yenisi açılırdı.
 */
export function useHotkeys(hotkeys: Hotkey[], etkin = true): void {
  // Kısayol listesi her çizimde yeni bir dizi; efekti ona bağlamak dinleyiciyi
  // her çizimde söküp takmak olurdu. Ref üzerinden hep sonuncusu okunuyor.
  const guncel = useRef(hotkeys);
  guncel.current = hotkeys;

  useEffect(() => {
    if (!etkin) return;

    const handler = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      const hedef = e.target as HTMLElement | null;
      const yaziyor =
        hedef?.tagName === 'INPUT' ||
        hedef?.tagName === 'TEXTAREA' ||
        hedef?.isContentEditable === true;

      for (const kisayol of guncel.current) {
        if (e.key.toLowerCase() !== kisayol.key) continue;
        if (!!kisayol.ctrl !== ctrl) continue;
        if (!!kisayol.shift !== e.shiftKey) continue;
        // Ctrl'süz kısayollar yalnızca yazmıyorken.
        if (yaziyor && !kisayol.ctrl) continue;

        e.preventDefault();
        kisayol.handler();
        return;
      }
    };

    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [etkin]);
}
