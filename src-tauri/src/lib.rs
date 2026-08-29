//! Muiget masaüstü uygulamasının giriş noktası.
//!
//! Bu dosyanın tek işi Tauri'yi kurmak: ayarları yüklemek, indirme motorunu
//! ayağa kaldırmak, ilerleme olaylarını frontend'e köprülemek ve komutları
//! kaydetmek. İş mantığı burada değil — [`download`] ve [`settings`] içinde.

pub mod commands;
pub mod download;
pub mod extension_bridge;
pub mod settings;

use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

use commands::AppState;
use download::manager::{DownloadManager, DownloadStatus};
use download::human_bytes;
use settings::AppSettings;

/// Frontend'in dinlediği ilerleme olayı.
pub const PROGRESS_EVENT: &str = "muiget://progress";

/// Zaman bazlı hız kurallarının yeniden değerlendirilme aralığı.
/// Dakikada bir yeterli: kurallar dakika çözünürlüğünde tanımlanıyor.
const SCHEDULE_INTERVAL: Duration = Duration::from_secs(60);

/// Tepsi ipucusunun yenilenme aralığı. Yarım saniyede bir güncellemek
/// (ilerleme yayınıyla aynı tempo) işletim sistemi tarafında gereksiz iş.
const TRAY_INTERVAL: Duration = Duration::from_secs(1);

const TRAY_ID: &str = "muiget-tray";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Tek örnek eklentisi EN ÖNCE kayıtlı olmalı: ikinci bir süreç
        // başlatıldığında argümanları buraya taşıyıp kendisi kapanıyor.
        // Chrome uzantısı köprüsü tam olarak bunu kullanıyor — köprü süreci
        // `muiget --add <yük>` çağırıyor, istek açık pencereye düşüyor.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            pencereyi_getir(app);
            argumanlari_isle(app, &argv);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Pencere kapalıyken/odakta değilken indirme bitişini duyurmak için.
        // Arayüz içi toast pencere görünmüyorken hiç kimseye ulaşmıyor.
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let handle = app.handle().clone();

            // Platformun standart dizinleri. Bulunamazsa uygulama dizinine
            // düşülüyor — açılamamaktansa alışılmadık bir yere yazmak yeğ.
            let config_dir = handle
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let download_dir = handle
                .path()
                .download_dir()
                .unwrap_or_else(|_| config_dir.clone());

            // Kurulum kancası bir Tokio çalışma zamanı bağlamında **değil**;
            // ayar okuma da motor kurulumu da bu yüzden `block_on` içinde.
            //
            // Motorun handle'ı burada alınıyor (bkz. `DownloadManager::new`).
            // Dışarıda kurulsaydı handle alınamaz, ilk indirmede "there is no
            // reactor running" paniğiyle uygulama çökerdi.
            let (ayarlar, manager) = tauri::async_runtime::block_on(async {
                let ayarlar = AppSettings::load(&config_dir, download_dir).await;

                let manager = DownloadManager::new(ayarlar.engine.clone())
                    .map_err(|e| format!("indirme motoru kurulamadı: {e}"))?;
                manager.apply_bandwidth_schedule();

                // Önceki oturumdan kalan yarım indirmeleri listeye geri yükle.
                // Pencere açılmadan **önce**: arayüz açılışta `list_downloads`
                // çağırıyor ve tarama arka planda kalsaydı liste bir an boş
                // görünüp sonra dolardı. Tek bir klasörün dizin girdilerini
                // okumak, göze çarpan bir gecikme değil.
                yarim_indirmeleri_yukle(&manager, &ayarlar).await;

                Ok::<_, String>((ayarlar, manager))
            })?;

            // Motorun ilerleme yayınını Tauri olayına köprüle. Motor Tauri'yi
            // tanımıyor; bağlantı yalnızca burada kuruluyor.
            let mut abone = manager.subscribe();
            let olay_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match abone.recv().await {
                        Ok(snapshot) => {
                            let _ = olay_handle.emit(PROGRESS_EVENT, snapshot);
                        }
                        // Yayın hızlı akarken yavaş abone geride kalabilir;
                        // kaçırılan anlık görüntüler önemli değil, bir sonraki
                        // tick zaten güncel durumu taşıyor.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            // Zaman bazlı hız kuralları için dakikalık tetikleyici.
            let zamanlayici = manager.clone();
            tauri::async_runtime::spawn(async move {
                let mut aralik = tokio::time::interval(SCHEDULE_INTERVAL);
                loop {
                    aralik.tick().await;
                    zamanlayici.apply_bandwidth_schedule();
                }
            });

            kur_tepsi(app.handle(), manager.clone())?;

            app.manage(AppState {
                manager,
                settings: Mutex::new(ayarlar),
                config_dir,
            });

            // Uygulama zaten kapalıyken köprüden istek gelirse: bu süreç ilk
            // örnek oluyor ve kendi argümanlarını işlemek zorunda.
            let baslangic_args: Vec<String> = std::env::args().collect();
            argumanlari_isle(app.handle(), &baslangic_args);

            Ok(())
        })
        .on_window_event(|window, event| {
            // Pencereyi kapatmak uygulamayı sonlandırmıyor: indirmeler arka
            // planda sürüyor ve pencere tepsiye iniyor. Ayardan kapatılabilir;
            // kapalıysa varsayılan davranış (gerçekten kapan) geçerli.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let tepsiye_in = window
                    .try_state::<AppState>()
                    .map(|state| state.settings_snapshot().minimize_to_tray)
                    .unwrap_or(false);

                if tepsiye_in {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::probe_url,
            commands::start_download,
            commands::list_downloads,
            commands::rescan_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::pause_all_downloads,
            commands::resume_all_downloads,
            commands::remove_download,
            commands::get_settings,
            commands::save_settings,
            commands::effective_speed_limit,
            commands::engine_defaults,
            commands::reveal_in_folder,
            commands::install_native_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Diskte kalan yarım indirmeleri listeye geri yükler; ayar açıksa sürdürür.
///
/// Yalnızca ayarlardaki indirme klasörüne bakılıyor. Başka bir klasöre inen
/// indirmeler açılışta gelmiyor; kullanıcı o klasörü ayarlardan "Klasörü tara"
/// ile taratabilir. Her hedef klasörü kalıcı olarak izlemek, motorun bilmesi
/// gerekmeyen bir defter tutmak demek olurdu.
async fn yarim_indirmeleri_yukle(manager: &DownloadManager, ayarlar: &AppSettings) {
    let sayi = manager.restore(&ayarlar.download_dir).await;
    if sayi == 0 {
        return;
    }
    log::info!("{sayi} yarım indirme listeye geri yüklendi");

    if !ayarlar.resume_on_start {
        return;
    }
    for indirme in manager.list() {
        if indirme.status == DownloadStatus::Paused {
            if let Err(e) = manager.resume(&indirme.id) {
                log::warn!("{} sürdürülemedi: {e}", indirme.file_name);
            }
        }
    }
}

/// Sistem tepsisi ikonu: menü, tıklama davranışı ve canlı hız ipucu.
fn kur_tepsi(
    handle: &tauri::AppHandle,
    manager: DownloadManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let goster = MenuItem::with_id(handle, "goster", "Muiget'i göster", true, None::<&str>)?;
    let cikis = MenuItem::with_id(handle, "cikis", "Çıkış", true, None::<&str>)?;
    let menu = Menu::with_items(handle, &[&goster, &cikis])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Muiget — indirme yok")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "goster" => pencereyi_getir(app),
            "cikis" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Sol tıkla pencereyi getir — masaüstü uygulamalarında beklenen
            // davranış bu; menü sağ tıkta zaten açılıyor.
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                pencereyi_getir(tray.app_handle());
            }
        });

    if let Some(icon) = handle.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(handle)?;

    // İpucunu canlı tut: tepsiye bakınca kaç indirme var ve toplam hız ne,
    // pencereyi açmadan görülsün.
    let ipucu_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut aralik = tokio::time::interval(TRAY_INTERVAL);
        loop {
            aralik.tick().await;

            let liste = manager.list();
            let aktif = liste.iter().filter(|d| d.status.is_active()).count();
            let hiz: f64 = liste
                .iter()
                .filter(|d| d.status == DownloadStatus::Running)
                .map(|d| d.speed)
                .sum();

            let ipucu = if aktif == 0 {
                "Muiget — indirme yok".to_string()
            } else {
                format!("Muiget — {aktif} indirme · {}/s", human_bytes(hiz as u64))
            };

            if let Some(tepsi) = ipucu_handle.tray_by_id(TRAY_ID) {
                let _ = tepsi.set_tooltip(Some(&ipucu));
            }
        }
    });

    Ok(())
}

/// Komut satırından gelen `--add <yük>` isteğini işler.
///
/// Hem ilk açılışta hem de tek örnek eklentisi ikinci bir sürecin argümanlarını
/// taşıdığında çağrılıyor; iki yol da aynı koda düşsün diye ayrı fonksiyon.
fn argumanlari_isle(app: &tauri::AppHandle, args: &[String]) {
    let Some(istek) = extension_bridge::parse_add_argument(args) else {
        return;
    };

    let Some(state) = app.try_state::<AppState>() else {
        // Kurulum bitmeden argüman gelemez; yine de sessizce çıkmak, panik
        // atmaktan iyi.
        log::warn!("uygulama durumu hazır değil, indirme isteği atlandı");
        return;
    };

    let hedef = state.settings_snapshot().download_dir;
    match extension_bridge::handle_request(&state.manager, istek, hedef) {
        Ok(id) => log::info!("uzantıdan gelen indirme başlatıldı: {id}"),
        Err(e) => log::warn!("uzantıdan gelen indirme reddedildi: {e}"),
    }
}

fn pencereyi_getir(app: &tauri::AppHandle) {
    if let Some(pencere) = app.get_webview_window("main") {
        let _ = pencere.show();
        let _ = pencere.unminimize();
        let _ = pencere.set_focus();
    }
}
