use crate::settings;
use crate::sync;
use crate::types::{InstalledState, Manifest, Settings, SyncResult};
use std::path::{Path, PathBuf};
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
pub fn set_api_key(app: AppHandle, key: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    let trimmed = key.trim();
    s.api_key = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_launch_command(app: AppHandle, command: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    let trimmed = command.trim();
    s.launch_command = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_close_on_launch(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.close_on_launch = enabled;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_auto_launch(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.auto_launch = enabled;
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
pub async fn fetch_manifest(
    app: AppHandle,
    state: State<'_, AppState>,
    api_base_url: String,
) -> Result<Manifest, String> {
    let s = settings::load_settings(&app)?;
    let url = format!("{}/manifest", api_base_url.trim_end_matches('/'));
    let mut req = state.http.get(&url);
    if let Some(key) = s.api_key.as_deref().filter(|k| !k.is_empty()) {
        req = req.header("X-API-KEY", key);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Unauthorized: check the API Key in Settings".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while fetching manifest", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str::<Manifest>(&body).map_err(|e| {
        let snippet: String = body.chars().take(300).collect();
        format!("couldn't parse manifest response ({e}). Server returned: {snippet}")
    })
}

#[tauri::command]
pub fn get_installed_state(app: AppHandle, manifest: Manifest) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path.ok_or_else(|| "no game folder configured".to_string())?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Immediate disable/enable for a single addon (managed or custom). Managed
/// addons normally go through the batched Sync flow via addon_enabled, but
/// custom addons have no manifest entry to sync against, so this is the only
/// mechanism available for them.
#[tauri::command]
pub fn set_addon_disabled(app: AppHandle, manifest: Manifest, name: String, disabled: bool) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path.ok_or_else(|| "no game folder configured".to_string())?;
    sync::set_addon_disabled(&game_path, &name, disabled)?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Permanently deletes a tracked addon (managed or custom) right away,
/// without waiting for a sync. For managed addons this also unchecks it so
/// it doesn't silently come back on the next sync.
#[tauri::command]
pub fn delete_addon(app: AppHandle, manifest: Manifest, name: String) -> Result<InstalledState, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s.game_path.clone().ok_or_else(|| "no game folder configured".to_string())?;

    if let Some(removed) = sync::delete_addon(&game_path, &name)? {
        if removed.kind != "custom" {
            s.addon_enabled.insert(name, false);
            settings::save_settings(&app, &s)?;
        }
    }

    Ok(sync::get_installed_state(&game_path, &manifest))
}

#[derive(serde::Deserialize)]
struct PairResponse {
    token: String,
    user: PairUser,
}

#[derive(serde::Deserialize)]
struct PairUser {
    #[serde(rename = "discordUsername")]
    discord_username: Option<String>,
    #[serde(rename = "discordAvatar")]
    discord_avatar: Option<String>,
}

/// Exchanges a one-time pairing code (from the sot-launcher://pair deep
/// link) for a personal API token. Shared by the pair_with_code command and
/// the deep-link handler in lib.rs, since the OS can deliver the URL either
/// way depending on whether the app was already running.
pub async fn exchange_pair_code(
    app: &AppHandle,
    http: &reqwest::Client,
    code: String,
) -> Result<Settings, String> {
    let mut s = settings::load_settings(app)?;
    let url = format!("{}/pair", s.api_base_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .header("Accept", "application/json")
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} while pairing", resp.status()));
    }

    let body: PairResponse = resp.json().await.map_err(|e| e.to_string())?;

    s.discord_token = Some(body.token);
    s.discord_username = body.user.discord_username;
    s.discord_avatar = body.user.discord_avatar;
    settings::save_settings(app, &s)?;
    Ok(s)
}

#[tauri::command]
pub async fn pair_with_code(app: AppHandle, state: State<'_, AppState>, code: String) -> Result<Settings, String> {
    exchange_pair_code(&app, &state.http, code).await
}

/// Best-effort server-side revoke, then always clears the local credential
/// regardless of whether the server call succeeded (e.g. offline).
#[tauri::command]
pub async fn clear_discord_login(app: AppHandle, state: State<'_, AppState>) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;

    if let Some(token) = s.discord_token.as_deref().filter(|t| !t.is_empty()) {
        let url = format!("{}/unpair", s.api_base_url.trim_end_matches('/'));
        let _ = state
            .http
            .post(&url)
            .header("Accept", "application/json")
            .bearer_auth(token)
            .send()
            .await;
    }

    s.discord_token = None;
    s.discord_username = None;
    s.discord_avatar = None;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub async fn run_sync(
    app: AppHandle,
    manifest: Manifest,
    addon_enabled: std::collections::HashMap<String, bool>,
) -> Result<CombinedSyncResult, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s.game_path.clone().ok_or_else(|| "no game folder configured".to_string())?;

    let addons = sync::sync_addons(&app, &game_path, &manifest.addons, &addon_enabled).await;
    let wtf_files = sync::sync_wtf_files(&app, &game_path, &manifest.wtf_files).await;

    s.addon_enabled = addon_enabled;
    settings::save_settings(&app, &s)?;

    Ok(CombinedSyncResult { addons, wtf_files })
}

/// Known WoW executable names, checked case-insensitively.
const WOW_EXE_NAMES: &[&str] = &["wow.exe", "wowclassic.exe"];

/// Looks in `game_root` itself and one level of subfolders (covers both an
/// exe directly in the selected folder and the modern `_retail_`/`_classic_`/
/// `_classic_era_` layout).
fn find_wow_exe(game_root: &Path) -> Option<PathBuf> {
    if let Some(p) = scan_dir_for_exe(game_root) {
        return Some(p);
    }
    let entries = std::fs::read_dir(game_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(p) = scan_dir_for_exe(&path) {
                return Some(p);
            }
        }
    }
    None
}

fn scan_dir_for_exe(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_str()?.to_lowercase();
        if WOW_EXE_NAMES.contains(&name.as_str()) {
            return Some(path);
        }
    }
    None
}

#[tauri::command]
pub fn launch_game(app: AppHandle) -> Result<(), String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path.clone().ok_or_else(|| "no game folder configured".to_string())?;
    let game_root = PathBuf::from(&game_path);

    if let Some(custom) = s.launch_command.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        let resolved = custom.replace("{game_path}", &game_path);
        run_shell_command(&resolved, &game_root)?;
    } else {
        let exe_path = find_wow_exe(&game_root).ok_or_else(|| {
            format!(
                "Could not find Wow.exe under {game_path}. If you launch via Lutris, Bottles, or a \
                 custom Wine prefix, set a Custom Launch Command in Settings instead."
            )
        })?;
        let exe_dir = exe_path.parent().unwrap_or(&game_root);

        let mut cmd = if cfg!(target_os = "windows") {
            std::process::Command::new(&exe_path)
        } else {
            let mut c = std::process::Command::new("wine");
            c.arg(&exe_path);
            c
        };

        cmd.current_dir(exe_dir).spawn().map_err(|e| {
            if cfg!(target_os = "windows") {
                format!("failed to launch game: {e}")
            } else {
                format!(
                    "failed to launch via wine ({e}). If you use Lutris, Bottles, or a custom Wine \
                     prefix, set a Custom Launch Command in Settings instead."
                )
            }
        })?;
    }

    if s.close_on_launch {
        app.exit(0);
    }
    Ok(())
}

fn run_shell_command(command: &str, cwd: &Path) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(cwd)
        .spawn()
        .map_err(|e| format!("failed to run custom launch command: {e}"))?;
    Ok(())
}
