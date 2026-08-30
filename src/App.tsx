import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { AddDialog } from './components/AddDialog';
import { ContextMenu, type MenuItem } from './components/ContextMenu';
import { DownloadRow } from './components/DownloadRow';
import {
  IconClose,
  IconCopy,
  IconDownload,
  IconFolder,
  IconHash,
  IconLink,
  IconMoon,
  IconPause,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconSettings,
  IconSun,
  IconTrash,
} from './components/Icons';
import { SettingsDialog } from './components/SettingsDialog';
import { SpeedGraph } from './components/SpeedGraph';
import { Toasts, useToasts } from './components/Toasts';
import { useDownloads } from './hooks/useDownloads';
import { useHotkeys } from './hooks/useHotkeys';
import * as api from './lib/api';
import { copyText } from './lib/clipboard';
import { formatBytes, formatSpeed } from './lib/format';
import { osBildirimi } from './lib/notify';
import {
  isActive,
  isResumable,
  type AppSettings,
  type DownloadSnapshot,
  type DownloadStatus,
  type MediaSelection,
} from './lib/types';

/**
 * Adresin son parçası — bildirimde tam URL yerine dosya adını göstermek için.
 * Sunucunun vereceği gerçek ad farklı olabilir; burada amaç yalnızca
 * kullanıcının neyi kopyaladığını tanıması.
 */
function urlDosyaAdi(url: string): string {
  const yol = url.split(/[?#]/)[0];
  const son = yol.split('/').pop() ?? '';
  return decodeURIComponent(son) || url;
}

type Filtre = 'tumu' | 'aktif' | 'tamamlanan';

const FILTRE_ETIKETLERI: Record<Filtre, string> = {
  tumu: 'Tümü',
  aktif: 'Aktif',
  tamamlanan: 'Tamamlanan',
};

type Siralama = 'yeni' | 'eski' | 'ad' | 'boyut' | 'ilerleme';

/** Seçenek metinleri ne yaptığını açıkça söylüyor; ayrı bir ipucu gerekmesin. */
const SIRALAMA_ETIKETLERI: Record<Siralama, string> = {
  yeni: 'En yeni önce',
  eski: 'En eski önce',
  ad: 'Ad (A→Z)',
  boyut: 'Boyut (büyük→küçük)',
  ilerleme: 'İlerleme (az→çok)',
};

export default function App() {
  const { downloads, loading, refresh } = useDownloads();
  const { toasts, push, dismiss } = useToasts();

  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [filtre, setFiltre] = useState<Filtre>('tumu');
  const [arama, setArama] = useState('');
  const [siralama, setSiralama] = useState<Siralama>('yeni');
  const [addAcik, setAddAcik] = useState(false);
  const [ayarlarAcik, setAyarlarAcik] = useState(false);
  const [hizSiniri, setHizSiniri] = useState(0);

  /** Sağ tık menüsü: imleç konumu + hangi indirmeye ait olduğu. */
  const [menu, setMenu] = useState<{ x: number; y: number; id: string } | null>(null);
  /** Pencereye bırakılan adres — yeni indirme kutusunu dolu açıyor. */
  const [birakilanUrl, setBirakilanUrl] = useState<string | null>(null);
  const [surukleniyor, setSurukleniyor] = useState(false);

  const aramaRef = useRef<HTMLInputElement>(null);
  const diyalogAcik = addAcik || ayarlarAcik;

  // --- Ayarları yükle ---
  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch((e) => push('error', `Ayarlar yüklenemedi: ${api.errorMessage(e)}`));
  }, [push]);

  // --- Temayı uygula ---
  useEffect(() => {
    if (settings) document.documentElement.dataset.theme = settings.theme;
  }, [settings]);

  // --- Geçerli hız sınırını izle ---
  // Zaman kuralları saat başı devreye girebiliyor; durum çubuğu bunu göstermeli.
  useEffect(() => {
    let alive = true;
    const oku = () => {
      api.effectiveSpeedLimit().then(
        (limit) => {
          if (alive) setHizSiniri(limit);
        },
        () => {},
      );
    };

    oku();
    const zaman = window.setInterval(oku, 30_000);
    return () => {
      alive = false;
      window.clearInterval(zaman);
    };
  }, [settings]);

  // --- Panoda yakalanan bağlantı (karar #24) ---
  //
  // Motor yalnızca haber veriyor; indirme kendiliğinden başlamıyor. Kopyalanan
  // her adresi sessizce indirmeye başlamak, kullanıcının istemediği dosyaları
  // diske yazmak olurdu. Bildirimdeki düğme yeni indirme kutusunu dolu açıyor.
  useEffect(() => {
    const abonelik = api.onClipboardLink((url) => {
      push('info', `Panoda bağlantı: ${urlDosyaAdi(url)}`, {
        label: 'İndir',
        onSelect: () => {
          setBirakilanUrl(url);
          setAddAcik(true);
        },
      });
    });

    return () => {
      void abonelik.then((bitir) => bitir());
    };
  }, [push]);

  // --- Yeni sürüm kontrolü (karar #23) ---
  //
  // Oturumda bir kez, ayar açıksa. Hata yutuluyor: sürüm kontrolünün
  // başarısız olması kullanıcıyı ilgilendiren bir olay değil ve çevrimdışı
  // açılışta her seferinde hata göstermek rahatsız edici olurdu.
  const surumBakildi = useRef(false);
  useEffect(() => {
    if (!settings?.checkUpdates || surumBakildi.current) return;
    surumBakildi.current = true;

    api.checkForUpdate().then(
      (bilgi) => {
        if (!bilgi.available) return;
        push('info', `Yeni sürüm çıktı: v${bilgi.latest} (kurulu: v${bilgi.current})`, {
          label: 'Yayına git',
          onSelect: () => void api.openExternal(bilgi.url).catch(() => {}),
        });
      },
      () => {},
    );
  }, [settings, push]);

  // --- Biten ve başarısız olan indirmeleri duyur ---
  //
  // Motor her tick'te aynı durumu yeniden yayınlıyor, o yüzden ölçüt "şu an bu
  // durumda mı" değil "bu duruma yeni mi girdi" olmak zorunda; aksi hâlde
  // bildirim yarım saniyede bir tekrarlanırdı. Son durum indirme başına
  // saklanıyor: başarısız olup yeniden denenen ve yine başarısız olan bir
  // indirme ikinci kez de duyurulmalı.
  const sonDurum = useRef(new Map<string, DownloadStatus>());
  const ilkListeIslendi = useRef(false);

  useEffect(() => {
    if (loading) return;

    // Açılıştaki liste yalnızca referans alınıyor, duyurulmuyor: önceki
    // oturumdan geri yüklenen bir kayıt için "indirildi" demek yanlış olurdu.
    if (!ilkListeIslendi.current) {
      for (const indirme of downloads) sonDurum.current.set(indirme.id, indirme.status);
      ilkListeIslendi.current = true;
      return;
    }

    /**
     * Pencere önümüzdeyse uygulama içi bildirim, değilse işletim sistemi
     * bildirimi. İkisini birden göstermek, ekrana bakan kullanıcıya aynı şeyi
     * iki kez söylemek olurdu.
     */
    const duyur = async (tur: 'success' | 'error', baslik: string, mesaj: string) => {
      if (!settings?.notifyOnComplete) return;
      if (!document.hasFocus() && (await osBildirimi(baslik, mesaj))) return;
      push(tur, mesaj);
    };

    for (const indirme of downloads) {
      const onceki = sonDurum.current.get(indirme.id);
      sonDurum.current.set(indirme.id, indirme.status);
      if (onceki === undefined || onceki === indirme.status) continue;

      if (indirme.status === 'completed') {
        void duyur('success', 'İndirme tamamlandı', `${indirme.fileName} indirildi`);
      } else if (indirme.status === 'failed') {
        void duyur(
          'error',
          'İndirme başarısız',
          `${indirme.fileName}: ${indirme.error ?? 'bilinmeyen hata'}`,
        );
      }
    }

    // Listeden kaldırılanların kaydı da gitsin; harita sınırsız büyümesin.
    if (sonDurum.current.size > downloads.length) {
      const yasayan = new Set(downloads.map((d) => d.id));
      for (const id of [...sonDurum.current.keys()]) {
        if (!yasayan.has(id)) sonDurum.current.delete(id);
      }
    }
  }, [downloads, loading, settings, push]);

  // --- Eylemler ---
  const sarmala = useCallback(
    async (islem: () => Promise<void>, hataOneki: string) => {
      try {
        await islem();
      } catch (e) {
        push('error', `${hataOneki}: ${api.errorMessage(e)}`);
      }
    },
    [push],
  );

  const basla = useCallback(
    async (url: string, directory: string) => {
      await sarmala(async () => {
        await api.startDownload(url, directory);
        await refresh();
        setAddAcik(false);
      }, 'İndirme başlatılamadı');
    },
    [refresh, sarmala],
  );

  /**
   * Akış indirmesi — kullanıcının kalite/ses seçimiyle (karar #25).
   *
   * `basla`dan ayrı bir yol gerekiyordu çünkü taşınan bilgi farklı: adres
   * aynı ama yanında hangi parçanın indirileceği de gidiyor. Seçim
   * gönderilmezse motor ayarlardaki kalite tercihini uyguluyor.
   */
  const baslaMedya = useCallback(
    async (url: string, directory: string, selection: MediaSelection) => {
      await sarmala(async () => {
        await api.startMediaDownload(url, { directory, selection });
        await refresh();
        setAddAcik(false);
      }, 'Video indirmesi başlatılamadı');
    },
    [refresh, sarmala],
  );

  /**
   * Toplu ekleme: hepsini başlat, sonra **bir kez** yenile.
   *
   * Adres başına ayrı yenileme on bağlantıda on tur demek olurdu. Bir adres
   * hata verirse diğerleri yine de başlıyor — biri yüzünden hepsini iptal
   * etmek, kullanıcının listeyi baştan yapıştırması demek olurdu. Sonuç tek
   * bildirimde özetleniyor; on ayrı hata toast'ı kimseye yardım etmez.
   */
  const baslaCoklu = useCallback(
    async (urls: string[], directory: string) => {
      let basarili = 0;
      const hatalar: string[] = [];

      for (const adres of urls) {
        try {
          await api.startDownload(adres, directory);
          basarili += 1;
        } catch (e) {
          hatalar.push(api.errorMessage(e));
        }
      }

      await refresh();
      setAddAcik(false);

      if (basarili > 0) push('info', `${basarili} indirme kuyruğa alındı`);
      if (hatalar.length > 0) {
        push('error', `${hatalar.length} bağlantı başlatılamadı — ilki: ${hatalar[0]}`);
      }
    },
    [push, refresh],
  );

  const duraklat = useCallback(
    (id: string) => void sarmala(() => api.pauseDownload(id), 'Duraklatılamadı'),
    [sarmala],
  );

  const devamEt = useCallback(
    (id: string) => void sarmala(() => api.resumeDownload(id), 'Devam edilemedi'),
    [sarmala],
  );

  const iptalEt = useCallback(
    (id: string) => void sarmala(() => api.cancelDownload(id), 'İptal edilemedi'),
    [sarmala],
  );

  const kaldir = useCallback(
    (id: string) => {
      const indirme = downloads.find((d) => d.id === id);
      // Yarım indirmede yarım dosyayı da silmek doğru varsayılan; tamamlanmış
      // dosyaya dokunmak ise kullanıcının indirdiği şeyi silmek olurdu.
      const dosyalariSil = indirme ? indirme.status !== 'completed' : false;

      void sarmala(async () => {
        await api.removeDownload(id, dosyalariSil);
        await refresh();
      }, 'Kaldırılamadı');
    },
    [downloads, refresh, sarmala],
  );

  const klasordeGoster = useCallback(
    (path: string) => void sarmala(() => api.revealInFolder(path), 'Klasör açılamadı'),
    [sarmala],
  );

  /**
   * Aynı adresi baştan indirir.
   *
   * Klasör bilerek verilmiyor: hedef yol satırda duruyor ama ondan klasör
   * ayıklamak platforma göre ayırıcı tahmin etmek demek. Varsayılan indirme
   * klasörü hem doğru hem öngörülebilir; dosya adı çakışırsa motor zaten
   * benzersizleştiriyor.
   */
  const yenidenIndir = useCallback(
    (url: string) =>
      void sarmala(async () => {
        await api.startDownload(url);
        await refresh();
      }, 'Yeniden indirilemedi'),
    [refresh, sarmala],
  );

  const kopyala = useCallback(
    (metin: string, ne: string) => {
      void copyText(metin).then((oldu) =>
        push(oldu ? 'info' : 'error', oldu ? `${ne} kopyalandı` : `${ne} kopyalanamadı`),
      );
    },
    [push],
  );

  /**
   * Sürükle-bırak ile bağlantı ekleme.
   *
   * Tauri'nin kendi dosya-bırakma yakalayıcısı kapatıldı
   * (`tauri.conf.json` → `dragDropEnabled: false`); açıkken webview HTML5
   * sürükleme olaylarını hiç görmüyor. Bize gereken dosya değil **adres**:
   * tarayıcıdan sürüklenen bağlantı `text/uri-list` olarak geliyor.
   *
   * Bırakılan adres doğrudan indirilmiyor, yeni indirme kutusunu dolduruyor:
   * yanlışlıkla bırakılan bir bağlantının sessizce indirmeye başlaması
   * kullanıcının istemediği bir yan etki olurdu.
   */
  useEffect(() => {
    const uzerinde = (e: DragEvent) => {
      if (!e.dataTransfer) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      setSurukleniyor(true);
    };

    // `relatedTarget` boşsa imleç pencereden gerçekten çıktı; iç elemanlar
    // arasında gezinirken de `dragleave` tetikleniyor ve kaplama titrerdi.
    const ayrildi = (e: DragEvent) => {
      if (!e.relatedTarget) setSurukleniyor(false);
    };

    const birakildi = (e: DragEvent) => {
      e.preventDefault();
      setSurukleniyor(false);

      const ham =
        e.dataTransfer?.getData('text/uri-list') ||
        e.dataTransfer?.getData('text/plain') ||
        '';

      // `text/uri-list` çok satırlı olabiliyor ve `#` ile başlayan satırlar yorum.
      const adres = ham
        .split(/\r?\n/)
        .map((satir) => satir.trim())
        .find((satir) => /^https?:\/\/\S+$/i.test(satir));

      if (!adres) {
        push('error', 'Bırakılan şey bir web adresi değil');
        return;
      }

      setBirakilanUrl(adres);
      setAddAcik(true);
    };

    window.addEventListener('dragover', uzerinde);
    window.addEventListener('dragleave', ayrildi);
    window.addEventListener('drop', birakildi);

    return () => {
      window.removeEventListener('dragover', uzerinde);
      window.removeEventListener('dragleave', ayrildi);
      window.removeEventListener('drop', birakildi);
    };
  }, [push]);

  // Toplu eylemler motorda tek geçişte yapılıyor: arayüzün tek tek çağırması
  // hem N tur demek olurdu hem de araya biten bir indirme girdiğinde kuyruktan
  // yeni bir tanesi başlayıp duraklatılmadan kalabilirdi.
  const tumunuDuraklat = useCallback(
    () =>
      void sarmala(async () => {
        const sayi = await api.pauseAllDownloads();
        if (sayi > 0) push('info', `${sayi} indirme duraklatıldı`);
      }, 'Duraklatılamadı'),
    [push, sarmala],
  );

  const tumunuSurdur = useCallback(
    () =>
      void sarmala(async () => {
        const sayi = await api.resumeAllDownloads();
        if (sayi > 0) push('info', `${sayi} indirme kuyruğa alındı`);
      }, 'Devam edilemedi'),
    [push, sarmala],
  );

  const ayarlariKaydet = useCallback(
    async (yeni: AppSettings) => {
      await sarmala(async () => {
        await api.saveSettings(yeni);
        setSettings(yeni);
        setAyarlarAcik(false);
        push('success', 'Ayarlar kaydedildi');
      }, 'Ayarlar kaydedilemedi');
    },
    [push, sarmala],
  );

  /** Diyaloğu kapatmadan kaydeder — köprü kurulumu bunu kullanıyor. */
  const ayarlariSessizKaydet = useCallback(async (yeni: AppSettings) => {
    await api.saveSettings(yeni);
    setSettings(yeni);
  }, []);

  /**
   * Bir klasördeki yarım indirmeleri listeye geri yükler.
   *
   * Hata yakalanmıyor: diyalog sonucu kendi içinde gösteriyor, buradaki toast
   * tekrar olurdu.
   */
  const klasoruTara = useCallback(
    async (directory: string) => {
      const sayi = await api.rescanDownloads(directory);
      if (sayi > 0) await refresh();
      return sayi;
    },
    [refresh],
  );

  const temayiDegistir = useCallback(() => {
    if (!settings) return;
    const yeni: AppSettings = {
      ...settings,
      theme: settings.theme === 'dark' ? 'light' : 'dark',
    };
    setSettings(yeni);
    // Tema anında uygulanıyor, kaydetme arka planda; başarısız olursa bir
    // sonraki açılışta eski temaya döner — bunun için kullanıcıyı bloklamaya
    // değmez.
    api.saveSettings(yeni).catch(() => {});
  }, [settings]);

  // --- Klavye kısayolları ---
  // Diyalog açıkken kapalı: Esc ile kapanan bir diyaloğun üstüne yenisi
  // açılmasın ve arkadaki liste kısayolları araya girmesin.
  const kisayollar = useMemo(
    () => [
      { key: 'n', ctrl: true, handler: () => setAddAcik(true) },
      { key: ',', ctrl: true, handler: () => setAyarlarAcik(true) },
      { key: 'f', ctrl: true, handler: () => aramaRef.current?.select() },
      { key: '/', handler: () => aramaRef.current?.focus() },
    ],
    [],
  );
  useHotkeys(kisayollar, !diyalogAcik);

  // --- Türetilmiş veriler ---
  const sayimlar = useMemo(
    () => ({
      tumu: downloads.length,
      aktif: downloads.filter((d) => isActive(d.status)).length,
      tamamlanan: downloads.filter((d) => d.status === 'completed').length,
      // "Aktif" kuyruktakileri de sayıyor. Eşzamanlılık sınırı varken bu
      // yanıltıcı olabilir: 3 aktif görünüp yalnızca biri iniyor olabilir.
      // Durum çubuğu ikisini ayırıyor.
      sirada: downloads.filter((d) => d.status === 'queued').length,
    }),
    [downloads],
  );

  const gorunen = useMemo(() => {
    // Türkçe'de "İ".toLowerCase() araya birleşik bir nokta koyuyor ve
    // "istanbul" ile eşleşmiyor; yerel duyarlı çevrim şart.
    const aranan = arama.trim().toLocaleLowerCase('tr');

    const suzulmus = downloads.filter((d) => {
      if (filtre === 'aktif' && !isActive(d.status)) return false;
      if (filtre === 'tamamlanan' && d.status !== 'completed') return false;
      if (!aranan) return true;
      return (
        d.fileName.toLocaleLowerCase('tr').includes(aranan) ||
        d.url.toLocaleLowerCase('tr').includes(aranan)
      );
    });

    // Liste sırası eşitlik bozucu olarak taşınıyor: `createdAt` saniye
    // çözünürlüğünde ve arka arkaya eklenen indirmeler eşit çıkabiliyor.
    const sirali = suzulmus.map((d, i) => ({ d, i }));
    sirali.sort((a, b) => karsilastir(a, b, siralama));
    return sirali.map((x) => x.d);
  }, [downloads, filtre, arama, siralama]);

  const toplamHiz = useMemo(
    () => downloads.filter((d) => d.status === 'running').reduce((t, d) => t + d.speed, 0),
    [downloads],
  );

  const kalanToplam = useMemo(
    () =>
      downloads
        .filter((d) => isActive(d.status))
        .reduce((t, d) => t + Math.max(d.totalSize - d.downloaded, 0), 0),
    [downloads],
  );

  const surdurulebilir = useMemo(
    () => downloads.some((d) => isResumable(d.status)),
    [downloads],
  );

  /**
   * İnen dosyanın SHA-256 özetini hesaplayıp gösterir (karar #21).
   *
   * Büyük dosyada saniyeler sürüyor, o yüzden önce "hesaplanıyor" bildirimi
   * çıkıyor: tıklamanın hiçbir şey yapmadığı izlenimi vermemek için. Sonuç
   * kopyalanabilir; kullanıcı onu sitedeki değerle karşılaştıracak.
   */
  const ozetHesapla = useCallback(
    (indirme: DownloadSnapshot) => {
      push('info', `${indirme.fileName} — SHA-256 hesaplanıyor…`);
      api.fileChecksum(indirme.id, 'sha256').then(
        (ozet) => {
          push('success', `SHA-256: ${ozet}`, {
            label: 'Kopyala',
            onSelect: () => kopyala(ozet, 'Özet'),
          });
        },
        (e) => push('error', `Özet hesaplanamadı: ${api.errorMessage(e)}`),
      );
    },
    [kopyala, push],
  );

  /**
   * Sağ tık menüsünün içeriği.
   *
   * Menü duruma göre kısalıyor: tamamlanmış bir indirmede "Duraklat",
   * inen bir indirmede "Klasörde göster" anlamsız. Sönük ama duran maddeler
   * yerine hiç göstermemek, menüyü her durumda kısa ve okunur tutuyor.
   */
  const menuOgeleri = useMemo<MenuItem[]>(() => {
    if (!menu) return [];
    const indirme = downloads.find((d) => d.id === menu.id);
    if (!indirme) return [];

    const ogeler: MenuItem[] = [
      {
        label: 'Bağlantıyı kopyala',
        icon: <IconLink />,
        onSelect: () => kopyala(indirme.url, 'Bağlantı'),
      },
      {
        label: 'Dosya adını kopyala',
        icon: <IconCopy />,
        onSelect: () => kopyala(indirme.fileName, 'Dosya adı'),
      },
    ];

    if (isActive(indirme.status)) {
      ogeler.push({
        label: 'Duraklat',
        icon: <IconPause />,
        separated: true,
        onSelect: () => duraklat(indirme.id),
      });
    }

    if (isResumable(indirme.status)) {
      ogeler.push({
        label: 'Devam et',
        icon: <IconPlay />,
        separated: true,
        onSelect: () => devamEt(indirme.id),
      });
    }

    if (indirme.status === 'completed') {
      ogeler.push({
        label: 'Klasörde göster',
        icon: <IconFolder />,
        separated: true,
        onSelect: () => klasordeGoster(indirme.targetPath),
      });
      // Özet yalnızca tamamlanmış dosyada anlamlı: yarım dosyanın özeti,
      // kullanıcıya "indirme bozuk" dedirtirdi.
      ogeler.push({
        label: 'SHA-256 hesapla',
        icon: <IconHash />,
        onSelect: () => ozetHesapla(indirme),
      });
    }

    if (['completed', 'failed', 'cancelled'].includes(indirme.status)) {
      ogeler.push({
        label: 'Yeniden indir',
        icon: <IconRefresh />,
        onSelect: () => yenidenIndir(indirme.url),
      });
    }

    ogeler.push({
      label: 'Listeden kaldır',
      icon: <IconTrash />,
      danger: true,
      separated: true,
      onSelect: () => kaldir(indirme.id),
    });

    return ogeler;
  }, [
    menu,
    downloads,
    kopyala,
    duraklat,
    devamEt,
    klasordeGoster,
    ozetHesapla,
    yenidenIndir,
    kaldir,
  ]);

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <IconDownload />
          Muiget
        </div>

        <button
          className="button primary"
          onClick={() => setAddAcik(true)}
          title="Yeni indirme (Ctrl+N)"
        >
          <IconPlus /> Yeni indirme
        </button>

        <div className="topbar-spacer" />

        <button
          className="button icon ghost"
          onClick={temayiDegistir}
          title={settings?.theme === 'dark' ? 'Aydınlık temaya geç' : 'Koyu temaya geç'}
          aria-label="Temayı değiştir"
        >
          {settings?.theme === 'dark' ? <IconSun /> : <IconMoon />}
        </button>

        <button
          className="button icon ghost"
          onClick={() => setAyarlarAcik(true)}
          title="Ayarlar (Ctrl+,)"
          aria-label="Ayarlar"
          disabled={!settings}
        >
          <IconSettings />
        </button>
      </header>

      {/* Filtre, arama, sıralama ve toplu eylemler kendi satırında: üst çubuk
          bunların hepsini taşıyacak kadar geniş değil ve dar pencerede
          taşardı. */}
      <div className="toolbar">
        <div className="tabs" role="tablist" aria-label="İndirme filtresi">
          {(Object.keys(FILTRE_ETIKETLERI) as Filtre[]).map((secenek) => (
            <button
              key={secenek}
              role="tab"
              aria-selected={filtre === secenek}
              className={filtre === secenek ? 'is-active' : ''}
              onClick={() => setFiltre(secenek)}
            >
              {FILTRE_ETIKETLERI[secenek]}
              <span className="tab-count">{sayimlar[secenek]}</span>
            </button>
          ))}
        </div>

        <div className="search">
          <IconSearch className="search__icon" />
          <input
            ref={aramaRef}
            className="text-input"
            value={arama}
            onChange={(e) => setArama(e.target.value)}
            // Esc önce aramayı temizliyor, boşken odağı bırakıyor: ikinci Esc
            // kullanıcıyı listeye döndürsün.
            onKeyDown={(e) => {
              if (e.key !== 'Escape') return;
              if (arama) setArama('');
              else e.currentTarget.blur();
            }}
            placeholder="Dosya adı veya adreste ara"
            aria-label="İndirmelerde ara"
            title="İndirmelerde ara (Ctrl+F ya da /)"
            spellCheck={false}
            autoComplete="off"
          />
          {arama && (
            <button
              className="search__clear"
              onClick={() => {
                setArama('');
                aramaRef.current?.focus();
              }}
              aria-label="Aramayı temizle"
              title="Aramayı temizle"
            >
              <IconClose />
            </button>
          )}
        </div>

        <select
          className="select"
          value={siralama}
          onChange={(e) => setSiralama(e.target.value as Siralama)}
          aria-label="Sıralama"
          title="Sıralama"
        >
          {(Object.keys(SIRALAMA_ETIKETLERI) as Siralama[]).map((secenek) => (
            <option key={secenek} value={secenek}>
              {SIRALAMA_ETIKETLERI[secenek]}
            </option>
          ))}
        </select>

        {/* Toplu eylemler yalnızca yapacak iş varken görünüyor; hep duran ama
            çoğu zaman etkisiz iki düğme gürültü olurdu. Sarmalayıcı, dar
            pencerede ikisinin ayrı satırlara düşmesini engelliyor. */}
        <div className="toolbar__actions">
          {sayimlar.aktif > 0 && (
            <button
              className="button small"
              onClick={tumunuDuraklat}
              title="Çalışan ve kuyrukta bekleyen tüm indirmeleri duraklat"
            >
              <IconPause /> Tümünü duraklat
            </button>
          )}
          {surdurulebilir && (
            <button
              className="button small"
              onClick={tumunuSurdur}
              title="Duraklatılmış ve başarısız tüm indirmeleri kuyruğa al"
            >
              <IconPlay /> Tümünü sürdür
            </button>
          )}
        </div>
      </div>

      <main className="content">
        {loading ? (
          <p className="muted">Yükleniyor…</p>
        ) : gorunen.length === 0 ? (
          <BosDurum
            filtre={filtre}
            arama={arama.trim()}
            onEkle={() => setAddAcik(true)}
            onAramayiTemizle={() => setArama('')}
          />
        ) : (
          <div className="downloads">
            {gorunen.map((indirme: DownloadSnapshot) => (
              <DownloadRow
                key={indirme.id}
                download={indirme}
                onPause={duraklat}
                onResume={devamEt}
                onCancel={iptalEt}
                onRemove={kaldir}
                onReveal={klasordeGoster}
                onContextMenu={(e, id) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, id });
                }}
              />
            ))}
          </div>
        )}
      </main>

      <footer className="statusbar">
        <span>
          <strong>{sayimlar.aktif}</strong> aktif
        </span>
        {sayimlar.sirada > 0 && (
          <span>
            <strong>{sayimlar.sirada}</strong> sırada
          </span>
        )}
        {sayimlar.aktif > 0 && (
          <>
            <span>
              Toplam hız <strong>{formatSpeed(toplamHiz)}</strong>
            </span>
            <SpeedGraph speed={toplamHiz} />
            <span>
              Kalan <strong>{formatBytes(kalanToplam)}</strong>
            </span>
          </>
        )}
        <span className="push">
          {hizSiniri > 0 ? (
            <span className="limit-on">Hız sınırı {formatSpeed(hizSiniri)}</span>
          ) : (
            'Hız sınırı yok'
          )}
        </span>
      </footer>

      {addAcik && settings && (
        <AddDialog
          // Diyalog açıkken ikinci bir bağlantı bırakılırsa kutu yeni adresle
          // yeniden kurulsun diye anahtar adrese bağlı.
          key={birakilanUrl ?? 'bos'}
          defaultDirectory={settings.downloadDir}
          initialUrl={birakilanUrl ?? undefined}
          onClose={() => {
            setAddAcik(false);
            setBirakilanUrl(null);
          }}
          onStart={basla}
          onStartMany={baslaCoklu}
          onStartMedia={baslaMedya}
        />
      )}

      {menu && menuOgeleri.length > 0 && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuOgeleri}
          onClose={() => setMenu(null)}
        />
      )}

      {surukleniyor && (
        <div className="drop-overlay" aria-hidden>
          <div className="drop-overlay__card">
            <IconLink />
            <strong>Bağlantıyı bırak</strong>
            <span>Adres yeni indirme kutusuna düşecek</span>
          </div>
        </div>
      )}

      {ayarlarAcik && settings && (
        <SettingsDialog
          settings={settings}
          onClose={() => setAyarlarAcik(false)}
          onSave={ayarlariKaydet}
          onSaveQuiet={ayarlariSessizKaydet}
          onRescan={klasoruTara}
        />
      )}

      <Toasts toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}

/** Sıralama karşılaştırıcısı. `i` liste sırası — eşitlik bozucu olarak. */
function karsilastir(
  a: { d: DownloadSnapshot; i: number },
  b: { d: DownloadSnapshot; i: number },
  siralama: Siralama,
): number {
  switch (siralama) {
    case 'yeni':
      return b.d.createdAt - a.d.createdAt || b.i - a.i;
    case 'eski':
      return a.d.createdAt - b.d.createdAt || a.i - b.i;
    case 'ad':
      return a.d.fileName.localeCompare(b.d.fileName, 'tr') || a.i - b.i;
    case 'boyut':
      return b.d.totalSize - a.d.totalSize || a.i - b.i;
    case 'ilerleme':
      return ilerlemeOrani(a.d) - ilerlemeOrani(b.d) || a.i - b.i;
  }
}

function ilerlemeOrani(d: DownloadSnapshot): number {
  return d.totalSize > 0 ? d.downloaded / d.totalSize : 0;
}

function BosDurum({
  filtre,
  arama,
  onEkle,
  onAramayiTemizle,
}: {
  filtre: Filtre;
  arama: string;
  onEkle: () => void;
  onAramayiTemizle: () => void;
}) {
  // Arama sonuçsuzsa sebep filtre değil arama; kullanıcıya doğru çıkışı ver.
  if (arama) {
    return (
      <div className="empty">
        <IconSearch />
        <h2>Eşleşen indirme yok</h2>
        <p>
          <strong>{arama}</strong> ile eşleşen bir şey bulunamadı. Farklı bir kelime
          deneyin ya da aramayı temizleyin.
        </p>
        <button className="button" onClick={onAramayiTemizle}>
          <IconClose /> Aramayı temizle
        </button>
      </div>
    );
  }

  if (filtre === 'aktif') {
    return (
      <div className="empty">
        <IconDownload />
        <h2>Devam eden indirme yok</h2>
        <p>Biten indirmeler "Tamamlanan" sekmesinde.</p>
      </div>
    );
  }

  if (filtre === 'tamamlanan') {
    return (
      <div className="empty">
        <IconDownload />
        <h2>Henüz tamamlanan indirme yok</h2>
        <p>İlk indirmen bittiğinde burada listelenecek.</p>
      </div>
    );
  }

  return (
    <div className="empty">
      <IconDownload />
      <h2>Liste boş</h2>
      <p>
        Bir bağlantı yapıştır, Muiget dosyayı parçalara bölüp paralel indirsin.
        Bağlantı koparsa kaldığı yerden devam eder.
      </p>
      <button className="button primary" onClick={onEkle}>
        <IconPlus /> Yeni indirme
      </button>
    </div>
  );
}
