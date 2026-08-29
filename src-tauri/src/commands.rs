//! Tauri komutları — frontend'in backend'e açılan tek kapısı.
//!
//! Buradaki her fonksiyon ince bir sarmalayıcı: iş mantığı [`crate::download`]
//! içinde, dosya sistemi ayarları [`crate::settings`] içinde. Böylece motor
//! Tauri'den bağımsız kalıyor ve testleri pencere açmadan çalışıyor.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;

use crate::download::http::ServerCapabilities;
use crate::download::manager::{DownloadManager, DownloadSnapshot, ManagerConfig};
use crate::download::{DownloadError, Result};
use crate::settings::AppSettings;

pub struct AppState {
    pub manager: DownloadManager,
    pub settings: Mutex<AppSettings>,
    pub config_dir: PathBuf,
}

impl AppState {
    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.lock().unwrap().clone()
    }
}

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// İndirmeye başlamadan önce sunucuyu yoklar. Arayüz bunu "URL yapıştır"
/// diyaloğunda dosya adını ve boyutunu önizlemek için kullanıyor.
#[tauri::command]
pub async fn probe_url(state: State<'_, AppState>, url: String) -> Result<ServerCapabilities> {
    let ayarlar = state.settings_snapshot();
    let client = crate::download::http::build_client(
        &ayarlar.engine.user_agent,
        std::time::Duration::from_secs(ayarlar.engine.connect_timeout_secs),
    )?;
    crate::download::http::probe(&client, &url).await
}

#[tauri::command]
pub fn start_download(
    state: State<'_, AppState>,
    url: String,
    directory: Option<String>,
) -> Result<String> {
    let hedef = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.settings_snapshot().download_dir);
    state.manager.start(url, hedef)
}

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Vec<DownloadSnapshot> {
    state.manager.list()
}

/// İndirme klasörünü tarar, listede olmayan yarım indirmeleri geri yükler ve
/// kaç tane bulunduğunu döner.
///
/// Açılışta zaten bir kez çalışıyor. Bu komut, kullanıcı indirme klasörünü
/// değiştirdiğinde ya da dosyaları elle taşıdığında yeniden taratabilsin diye
/// var — açılışı beklemek gerekmiyor.
#[tauri::command]
pub async fn rescan_downloads(
    state: State<'_, AppState>,
    directory: Option<String>,
) -> Result<usize> {
    let hedef = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.settings_snapshot().download_dir);
    Ok(state.manager.restore(&hedef).await)
}

#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, id: String) -> Result<()> {
    state.manager.pause(&id)
}

#[tauri::command]
pub fn resume_download(state: State<'_, AppState>, id: String) -> Result<()> {
    state.manager.resume(&id)
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) -> Result<()> {
    state.manager.cancel(&id)
}

/// Çalışan ve kuyrukta bekleyen tüm indirmeleri duraklatır; etkilenen sayıyı
/// döner. Arayüzdeki "Tümünü duraklat" düğmesi bunu çağırıyor.
#[tauri::command]
pub fn pause_all_downloads(state: State<'_, AppState>) -> usize {
    state.manager.pause_all()
}

/// Duraklatılmış ve başarısız tüm indirmeleri kuyruğa alır; etkilenen sayıyı
/// döner. Eşzamanlılık sınırı geçerli kalıyor.
#[tauri::command]
pub fn resume_all_downloads(state: State<'_, AppState>) -> usize {
    state.manager.resume_all()
}

#[tauri::command]
pub async fn remove_download(
    state: State<'_, AppState>,
    id: String,
    delete_files: bool,
) -> Result<()> {
    state.manager.remove(&id, delete_files).await
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings_snapshot()
}

/// Ayarları kaydeder ve motora anında uygular.
#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<()> {
    let mut yeni = settings;
    yeni.normalize();

    // Kilit `await`ten önce bırakılıyor: std Mutex bir await noktasını
    // geçemez (Send değil) ve geçse bile tüm komutları bloklardı.
    {
        let mut mevcut = state.settings.lock().unwrap();
        *mevcut = yeni.clone();
    }

    state.manager.update_config(yeni.engine.clone())?;
    yeni.save(&state.config_dir).await
}

/// O an geçerli hız sınırı (zaman kuralları uygulandıktan sonra).
/// Arayüz durum çubuğunda "gece kuralı etkin" göstermek için kullanıyor.
#[tauri::command]
pub fn effective_speed_limit(state: State<'_, AppState>) -> u64 {
    state.manager.effective_speed_limit()
}

#[tauri::command]
pub fn engine_defaults() -> ManagerConfig {
    ManagerConfig::default()
}

/// Chrome/Edge'e native messaging host'unu tanıtır.
///
/// Kullanıcı ayarlardan açıkça istediğinde çağrılıyor — kurulumda sessizce
/// tarayıcıya kaydolmak, bir indirme yöneticisi için fazla ileri giderdi.
/// Dönen değer manifest dosyasının yolu: otomatik registry kaydı başarısız
/// olursa kullanıcı bu yolu elle tanıtabilir.
#[tauri::command]
pub fn install_native_host(extension_ids: Vec<String>, state: State<'_, AppState>) -> Result<String> {
    let exe = std::env::current_exe()
        .map_err(|e| DownloadError::Other(format!("uygulama yolu bulunamadı: {e}")))?;

    let yol = crate::extension_bridge::install_host(&state.config_dir, &exe, &extension_ids)
        .map_err(DownloadError::Io)?;

    Ok(yol.to_string_lossy().into_owned())
}

/// Dosyayı işletim sisteminin dosya yöneticisinde gösterir.
#[tauri::command]
pub fn reveal_in_folder(app: tauri::AppHandle, path: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;

    let hedef = PathBuf::from(&path);
    // İndirme yarım kaldıysa nihai dosya henüz yok; klasörü açmak yine de
    // kullanıcının istediği şeyi yapar.
    let acilacak = if hedef.exists() {
        hedef
    } else {
        hedef.parent().map(PathBuf::from).ok_or_else(|| {
            DownloadError::Other(format!("{path} için üst klasör bulunamadı"))
        })?
    };

    app.opener()
        .reveal_item_in_dir(&acilacak)
        .map_err(|e| DownloadError::Other(format!("klasör açılamadı: {e}")))
}
