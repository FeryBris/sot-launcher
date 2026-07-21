use crate::settings;
use crate::sync;
use crate::types::{LockFile, Manifest, Settings, SyncResult};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedSyncResult {
    pub addons: SyncResult,
    pub wtf_files: SyncResult,
}

pub struct AppState {
    pub http: reqwest::Client,
}

impl Default for AppState {
    fn default() -> Self {
        Self { http: reqwest::Client::new() }
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load_settings(&app)
}

#[tauri::command]
pub fn set_game_path(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path = Some(path);
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_api_base_url(app: AppHandle, url: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.api_base_url = url;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub async fn pick_game_folder(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title("Select your World of Warcraft folder")
        .pick_folder(move |folder| {
            let _ = tx.send(folder);
        });
    let picked = rx.recv().map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn fetch_manifest(state: State<'_, AppState>, api_base_url: String) -> Result<Manifest, String> {
    let url = format!("{}/manifest", api_base_url.trim_end_matches('/'));
    let resp = state.http.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while fetching manifest", resp.status()));
    }
    resp.json::<Manifest>().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_lock_state(app: AppHandle) -> Result<LockFile, String> {
    settings::load_lock(&app)
}

#[tauri::command]
pub async fn run_sync(
    app: AppHandle,
    manifest: Manifest,
    selected_optional: Vec<String>,
) -> Result<CombinedSyncResult, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path.ok_or_else(|| "no game folder configured".to_string())?;

    let mut lock = settings::load_lock(&app)?;
    let addons = sync::sync_addons(&app, &game_path, &manifest.addons, &selected_optional, &mut lock).await;
    let wtf_files = sync::sync_wtf_files(&app, &game_path, &manifest.wtf_files, &mut lock).await;
    settings::save_lock(&app, &lock)?;

    Ok(CombinedSyncResult { addons, wtf_files })
}

#[tauri::command]
pub fn launch_game(app: AppHandle) -> Result<(), String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path.ok_or_else(|| "no game folder configured".to_string())?;

    let exe_name = if cfg!(target_os = "windows") { "WoW.exe" } else { "WoW" };
    let exe_path = std::path::PathBuf::from(&game_path).join(exe_name);
    if !exe_path.exists() {
        return Err(format!("{} not found in {}", exe_name, game_path));
    }

    std::process::Command::new(&exe_path)
        .current_dir(&game_path)
        .spawn()
        .map_err(|e| format!("failed to launch game: {e}"))?;
    Ok(())
}
