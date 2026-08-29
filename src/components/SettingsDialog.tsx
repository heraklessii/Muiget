import { useEffect, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';

import { errorMessage, installNativeHost } from '../lib/api';

import {
  bytesToMbps,
  clockToMinute,
  mbpsToBytes,
  minuteToClock,
} from '../lib/format';
import type { AppSettings, BandwidthRule } from '../lib/types';
import { IconClose, IconFolder, IconPlus, IconTrash } from './Icons';

interface Props {
  settings: AppSettings;
  onClose: () => void;
  /** Kaydet düğmesi: kaydeder, bildirir ve diyaloğu kapatır. */
  onSave: (settings: AppSettings) => Promise<void>;
  /**
   * Diyaloğu kapatmadan kaydeder. Köprü kurulumu için gerekli: manifest
   * kimliğe göre yazılıyor, dolayısıyla kimlik o an kalıcı olmalı. Ayrıca
   * üst bileşenin durumu taslakla senkron kalıyor.
   */
  onSaveQuiet: (settings: AppSettings) => Promise<void>;
  /**
   * Verilen klasörü yarım indirmeler için tarar ve kaç tane eklendiğini döner.
   * Açılışta zaten bir kez çalışıyor; buradaki düğme, indirme klasörü
   * değiştikten sonra taramayı yeniden açılışı beklemeden tetiklemek için.
   */
  onRescan: (directory: string) => Promise<number>;
}

/**
 * Chrome uzantı kimliği: 32 karakter, yalnızca a–p arası küçük harf.
 * Aynı kural Rust tarafında da uygulanıyor (`settings::gecerli_uzanti_kimligi`);
 * burada olması kullanıcıyı yazarken uyarmak için.
 */
function gecerliKimlik(id: string): boolean {
  return /^[a-p]{32}$/.test(id);
}

/**
 * Ayarlar diyaloğu.
 *
 * Taslak üzerinde çalışıyor: kullanıcı "Kaydet" demeden hiçbir değişiklik
 * motora gitmiyor. Yarım bırakılan bir segment sayısı düzenlemesinin akan
 * indirmeleri etkilemesi istenmiyor.
 */
export function SettingsDialog({ settings, onClose, onSave, onSaveQuiet, onRescan }: Props) {
  const [taslak, setTaslak] = useState<AppSettings>(settings);
  const [kaydediliyor, setKaydediliyor] = useState(false);
  const [kopruKuruluyor, setKopruKuruluyor] = useState(false);
  const [kopruSonucu, setKopruSonucu] = useState<string | null>(null);
  const [taraniyor, setTaraniyor] = useState(false);
  const [taramaSonucu, setTaramaSonucu] = useState<string | null>(null);

  /** Klasördeki `.muiget` dosyalarını listeye geri yükler. */
  async function klasoruTara() {
    setTaraniyor(true);
    setTaramaSonucu(null);
    try {
      const sayi = await onRescan(taslak.downloadDir);
      setTaramaSonucu(
        sayi === 0
          ? 'Listede olmayan yarım indirme bulunamadı.'
          : `${sayi} yarım indirme listeye eklendi.`,
      );
    } catch (e) {
      setTaramaSonucu(`Klasör taranamadı: ${errorMessage(e)}`);
    } finally {
      setTaraniyor(false);
    }
  }

  const kimlikGecerli = gecerliKimlik(taslak.extensionIds[0] ?? '');

  /**
   * Köprüyü kurmak ayarları da kaydediyor: manifest kimliğe göre yazılıyor ve
   * kimlik kaydedilmemişse bir sonraki açılışta kutu boş görünürdü.
   */
  async function kopruyuKur() {
    setKopruKuruluyor(true);
    setKopruSonucu(null);
    try {
      await onSaveQuiet(taslak);
      const yol = await installNativeHost(taslak.extensionIds);
      setKopruSonucu(`Köprü kuruldu. Manifest: ${yol}`);
    } catch (e) {
      setKopruSonucu(`Köprü kurulamadı: ${errorMessage(e)}`);
    } finally {
      setKopruKuruluyor(false);
    }
  }

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  function motoruGuncelle(yama: Partial<AppSettings['engine']>) {
    setTaslak((o) => ({ ...o, engine: { ...o.engine, ...yama } }));
  }

  function kuraliGuncelle(sira: number, yama: Partial<BandwidthRule>) {
    setTaslak((o) => ({
      ...o,
      engine: {
        ...o.engine,
        bandwidthRules: o.engine.bandwidthRules.map((k, i) =>
          i === sira ? { ...k, ...yama } : k,
        ),
      },
    }));
  }

  function kuralEkle() {
    motoruGuncelle({
      bandwidthRules: [
        ...taslak.engine.bandwidthRules,
        // Varsayılan: gece 02:00-08:00 sınırsız — en sık istenen kural.
        { startMinute: 120, endMinute: 480, limitBytes: 0, enabled: true },
      ],
    });
  }

  function kuralSil(sira: number) {
    motoruGuncelle({
      bandwidthRules: taslak.engine.bandwidthRules.filter((_, i) => i !== sira),
    });
  }

  async function klasorSec() {
    const secilen = await openDialog({ directory: true, defaultPath: taslak.downloadDir });
    if (typeof secilen === 'string') setTaslak((o) => ({ ...o, downloadDir: secilen }));
  }

  async function kaydet() {
    setKaydediliyor(true);
    try {
      await onSave(taslak);
    } finally {
      setKaydediliyor(false);
    }
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal aria-label="Ayarlar">
        <div className="dialog__head">
          <h2>Ayarlar</h2>
          <button className="button icon ghost" onClick={onClose} aria-label="Kapat">
            <IconClose />
          </button>
        </div>

        <div className="dialog__body">
          {/* ---- Genel ---- */}
          <h3 className="section-title">Genel</h3>

          <label className="field">
            <span>Varsayılan indirme klasörü</span>
            <div className="row">
              <input
                className="text-input grow"
                value={taslak.downloadDir}
                onChange={(e) => setTaslak((o) => ({ ...o, downloadDir: e.target.value }))}
                spellCheck={false}
              />
              <button className="button icon" onClick={klasorSec} title="Klasör seç">
                <IconFolder />
              </button>
              <button
                className="button"
                onClick={klasoruTara}
                disabled={taraniyor}
                title="Bu klasördeki yarım indirmeleri listeye ekle"
              >
                {taraniyor ? 'Taranıyor…' : 'Klasörü tara'}
              </button>
            </div>
            <span className="field-hint">
              Yarım indirmeler açılışta bu klasörden geri yükleniyor. Klasörü
              değiştirdiysen ya da dosyaları elle taşıdıysan yeniden taratabilirsin.
            </span>
          </label>

          {taramaSonucu && <p className="field-hint">{taramaSonucu}</p>}

          <div className="field row">
            <span>
              Açılışta yarım indirmeleri sürdür
              <br />
              <span className="field-hint">
                Kapalıyken liste yine dolu gelir, indirmeler duraklatılmış bekler
              </span>
            </span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.resumeOnStart}
              aria-label="Açılışta yarım indirmeleri sürdür"
              onClick={() => setTaslak((o) => ({ ...o, resumeOnStart: !o.resumeOnStart }))}
            />
          </div>

          <div className="field row">
            <span>Kapatınca sistem tepsisinde çalışmaya devam et</span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.minimizeToTray}
              aria-label="Sistem tepsisinde çalışmaya devam et"
              onClick={() => setTaslak((o) => ({ ...o, minimizeToTray: !o.minimizeToTray }))}
            />
          </div>

          <div className="field row">
            <span>İndirme bitince bildir</span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.notifyOnComplete}
              aria-label="İndirme bitince bildir"
              onClick={() => setTaslak((o) => ({ ...o, notifyOnComplete: !o.notifyOnComplete }))}
            />
          </div>

          <div className="field row">
            <span>
              Panodaki bağlantıları yakala
              <br />
              <span className="field-hint">
                Kopyaladığınız adres indirilebilir bir dosyaya işaret ediyorsa Muiget
                sorar. <strong>Açıkken uygulama panonuzu saniyede bir okur</strong> —
                indirme bağlantısı olmayan içerik anında elenir, hiçbir yere yazılmaz.
              </span>
            </span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.clipboardWatch}
              aria-label="Panodaki bağlantıları yakala"
              onClick={() => setTaslak((o) => ({ ...o, clipboardWatch: !o.clipboardWatch }))}
            />
          </div>

          <div className="field row">
            <span>
              Açılışta yeni sürüme bak
              <br />
              <span className="field-hint">
                Uygulamanın kendiliğinden yaptığı tek dış istek: GitHub'daki son yayın
                numarası. Kullanıcı verisi gönderilmez.
              </span>
            </span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.checkUpdates}
              aria-label="Açılışta yeni sürüme bak"
              onClick={() => setTaslak((o) => ({ ...o, checkUpdates: !o.checkUpdates }))}
            />
          </div>

          {/* ---- Bağlantı ---- */}
          <h3 className="section-title">Bağlantı</h3>

          <label className="field row">
            <span>
              Segment sayısı
              <br />
              <span className="field-hint">Dosya kaç paralel parçaya bölünsün (1–32)</span>
            </span>
            <input
              className="text-input"
              style={{ width: 76 }}
              type="number"
              min={1}
              max={32}
              value={taslak.engine.segments}
              onChange={(e) => motoruGuncelle({ segments: Number(e.target.value) })}
            />
          </label>

          <label className="field row">
            <span>
              Sunucu başına bağlantı
              <br />
              <span className="field-hint">
                Aynı siteye aynı anda açılacak en fazla bağlantı — sunucuya nazik
                davranmak için
              </span>
            </span>
            <input
              className="text-input"
              style={{ width: 76 }}
              type="number"
              min={1}
              max={64}
              value={taslak.engine.maxConnectionsPerHost}
              onChange={(e) =>
                motoruGuncelle({ maxConnectionsPerHost: Number(e.target.value) })
              }
            />
          </label>

          <label className="field row">
            <span>
              Aynı anda indirme
              <br />
              <span className="field-hint">
                Fazlası kuyrukta bekler; 0 = sınırsız. Hepsini birden başlatmak
                toplam süreyi kısaltmaz, yalnızca ilk dosyanın bitişini geciktirir.
              </span>
            </span>
            <input
              className="text-input"
              style={{ width: 76 }}
              type="number"
              min={0}
              max={64}
              value={taslak.engine.maxConcurrentDownloads}
              onChange={(e) =>
                motoruGuncelle({ maxConcurrentDownloads: Number(e.target.value) })
              }
            />
          </label>

          <label className="field row">
            <span>
              Yeniden deneme sayısı
              <br />
              <span className="field-hint">Kesilen bir parça kaç kez yeniden denensin</span>
            </span>
            <input
              className="text-input"
              style={{ width: 76 }}
              type="number"
              min={0}
              max={20}
              value={taslak.engine.maxRetries}
              onChange={(e) => motoruGuncelle({ maxRetries: Number(e.target.value) })}
            />
          </label>

          <div className="field row">
            <span>
              Adaptif parça bölme
              <br />
              <span className="field-hint">
                Bir parça bitince en yavaş parçanın kalanı ikiye bölünüp devralınır
              </span>
            </span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.engine.adaptive}
              aria-label="Adaptif parça bölme"
              onClick={() => motoruGuncelle({ adaptive: !taslak.engine.adaptive })}
            />
          </div>

          <div className="field row">
            <span>
              Dosyaları türüne göre klasörle
              <br />
              <span className="field-hint">
                İnen dosya <code>Video</code>, <code>Müzik</code>, <code>Belgeler</code>,{' '}
                <code>Arşivler</code>, <code>Programlar</code>, <code>Resimler</code>{' '}
                alt klasörlerine ayrılır. Tanınmayan türler klasörün kökünde kalır.
                Sürmekte olan indirmeler taşınmaz.
              </span>
            </span>
            <button
              className="switch"
              role="switch"
              aria-checked={taslak.engine.categorize}
              aria-label="Dosyaları türüne göre klasörle"
              onClick={() => motoruGuncelle({ categorize: !taslak.engine.categorize })}
            />
          </div>

          <label className="field row">
            <span>
              Vekil sunucu (proxy)
              <br />
              <span className="field-hint">
                Boş bırakılırsa doğrudan bağlanılır. Örnek:{' '}
                <code>http://10.0.0.1:8080</code>, <code>socks5://127.0.0.1:1080</code>.
                Şema yazılmazsa <code>http://</code> varsayılır. Gerekiyorsa kimlik:{' '}
                <code>http://kullanıcı:parola@sunucu:port</code>.
              </span>
            </span>
            <input
              className="text-input"
              style={{ width: 240 }}
              value={taslak.engine.proxy}
              onChange={(e) => motoruGuncelle({ proxy: e.target.value })}
              placeholder="doğrudan bağlantı"
              spellCheck={false}
              autoComplete="off"
            />
          </label>

          {/* ---- Hız ---- */}
          <h3 className="section-title">Hız sınırı</h3>

          <label className="field row">
            <span>
              Genel sınır (MB/s)
              <br />
              <span className="field-hint">0 = sınırsız</span>
            </span>
            <input
              className="text-input"
              style={{ width: 92 }}
              type="number"
              min={0}
              step={0.5}
              value={bytesToMbps(taslak.engine.globalSpeedLimit)}
              onChange={(e) =>
                motoruGuncelle({ globalSpeedLimit: mbpsToBytes(Number(e.target.value)) })
              }
            />
          </label>

          <div className="field">
            <span>
              Zaman kuralları
              <br />
              <span className="field-hint">
                Belirli saat aralıklarında farklı sınır. İlk eşleşen kural geçerli;
                hiçbiri tutmazsa genel sınır kullanılır.
              </span>
            </span>

            <div className="rules">
              {taslak.engine.bandwidthRules.map((kural, sira) => (
                <div className="rule" key={sira}>
                  <button
                    className="switch"
                    role="switch"
                    aria-checked={kural.enabled}
                    aria-label={`${sira + 1}. kuralı etkinleştir`}
                    onClick={() => kuraliGuncelle(sira, { enabled: !kural.enabled })}
                  />

                  <input
                    className="text-input time"
                    value={minuteToClock(kural.startMinute)}
                    onChange={(e) => {
                      const dakika = clockToMinute(e.target.value);
                      if (dakika !== null) kuraliGuncelle(sira, { startMinute: dakika });
                    }}
                    aria-label="Başlangıç saati"
                  />
                  <span className="rule__sep">–</span>
                  <input
                    className="text-input time"
                    value={minuteToClock(kural.endMinute)}
                    onChange={(e) => {
                      const dakika = clockToMinute(e.target.value);
                      if (dakika !== null) kuraliGuncelle(sira, { endMinute: dakika });
                    }}
                    aria-label="Bitiş saati"
                  />

                  <input
                    className="text-input limit grow"
                    type="number"
                    min={0}
                    step={0.5}
                    value={bytesToMbps(kural.limitBytes)}
                    onChange={(e) =>
                      kuraliGuncelle(sira, { limitBytes: mbpsToBytes(Number(e.target.value)) })
                    }
                    aria-label="Sınır (MB/s)"
                  />
                  <span className="rule__sep">MB/s</span>

                  <button
                    className="button small icon ghost danger"
                    onClick={() => kuralSil(sira)}
                    aria-label={`${sira + 1}. kuralı sil`}
                  >
                    <IconTrash />
                  </button>
                </div>
              ))}

              <button className="button small" onClick={kuralEkle}>
                <IconPlus /> Kural ekle
              </button>
            </div>
          </div>

          {/* ---- Tarayıcı uzantısı ---- */}
          <h3 className="section-title">Tarayıcı uzantısı</h3>

          <label className="field">
            <span>
              Chrome uzantısı kimliği
              <br />
              <span className="field-hint">
                <code>chrome://extensions</code> sayfasında uzantı kartında yazan 32
                harflik kimlik. Yalnızca buradaki uzantı köprüyü kullanabilir.
              </span>
            </span>
            <div className="row">
              <input
                className="text-input grow mono"
                value={taslak.extensionIds[0] ?? ''}
                onChange={(e) =>
                  setTaslak((o) => ({
                    ...o,
                    extensionIds: e.target.value.trim() ? [e.target.value.trim()] : [],
                  }))
                }
                placeholder="abcdefghijklmnopabcdefghijklmnop"
                spellCheck={false}
              />
              <button
                className="button"
                onClick={kopruyuKur}
                disabled={!kimlikGecerli || kopruKuruluyor}
              >
                {kopruKuruluyor ? 'Kuruluyor…' : 'Köprüyü kur'}
              </button>
            </div>
          </label>

          {kopruSonucu && <p className="field-hint">{kopruSonucu}</p>}
        </div>

        <div className="dialog__foot">
          <button className="button ghost" onClick={onClose}>
            Vazgeç
          </button>
          <button className="button primary" onClick={kaydet} disabled={kaydediliyor}>
            {kaydediliyor ? 'Kaydediliyor…' : 'Kaydet'}
          </button>
        </div>
      </div>
    </div>
  );
}
