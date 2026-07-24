use crate::settings;
use crate::sync;
use crate::types::{InstalledState, Manifest, Settings, SyncResult};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedSyncResult {
    pub addons: SyncResult,
    pub wtf_files: SyncResult,
}

pub struct AppState {
    pub http: reqwest::Client,
    /// The tray menu's first item ("Play"/"Update") — set once during
    /// `.setup()` in lib.rs, mutated later via `set_tray_primary_label`
    /// whenever the frontend's update-available state changes.
    pub tray_primary_item: std::sync::Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
            tray_primary_item: std::sync::Mutex::new(None),
        }
    }
}

/// Relabels the tray's first menu item ("Play" <-> "Update") — the
/// frontend already knows whether an update is available (it does the
/// actual update check), this just mirrors that onto the native tray menu.
#[tauri::command]
pub fn set_tray_primary_label(state: State<'_, AppState>, label: String) -> Result<(), String> {
    let guard = state.tray_primary_item.lock().map_err(|e| e.to_string())?;
    if let Some(item) = guard.as_ref() {
        item.set_text(label).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load_settings(&app)
}

#[tauri::command]
pub fn set_game_path_windows(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path_windows = Some(path);
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_game_path_linux(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path_linux = Some(path);
    settings::save_settings(&app, &s)?;
    Ok(s)
}

/// The guild site's base URL — safe to expose to the frontend (it's a
/// public address, not a secret) for building browser-open links. The
/// API key itself never leaves the Rust side.
#[tauri::command]
pub fn get_api_base_url() -> &'static str {
    crate::config::API_BASE_URL
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
pub fn set_hide_on_launch(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.hide_on_launch = enabled;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_close_to_tray(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.close_to_tray = enabled;
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
pub async fn fetch_manifest(state: State<'_, AppState>) -> Result<Manifest, String> {
    let url = format!("{}/manifest", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.get(&url).header("X-API-KEY", crate::config::API_KEY);
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Unauthorized: the launcher's guild API key is invalid — contact an officer".to_string());
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
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Immediate disable/enable for a single addon (managed or custom). Managed
/// addons normally go through the batched Sync flow via addon_enabled, but
/// custom addons have no manifest entry to sync against, so this is the only
/// mechanism available for them.
#[tauri::command]
pub fn set_addon_disabled(app: AppHandle, manifest: Manifest, name: String, disabled: bool) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    sync::set_addon_disabled(&game_path, &name, disabled)?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Permanently deletes a tracked addon (managed or custom) right away,
/// without waiting for a sync. For managed addons this also unchecks it so
/// it doesn't silently come back on the next sync.
#[tauri::command]
pub fn delete_addon(app: AppHandle, manifest: Manifest, name: String) -> Result<InstalledState, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;

    if let Some(removed) = sync::delete_addon(&game_path, &name)? {
        if removed.kind != "custom" {
            s.addon_enabled.insert(name, false);
            settings::save_settings(&app, &s)?;
        }
    }

    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Reads the SoTGuildSync addon's current SavedVariables snapshot for
/// local display — one entry per guild character logged into on this WoW
/// account. Empty covers both "no game folder set" and "addon hasn't
/// written anything yet", neither of which is an error state for the UI.
#[tauri::command]
pub fn get_guild_sync_data(app: AppHandle) -> Result<Vec<crate::types::GuildSyncData>, String> {
    let s = settings::load_settings(&app)?;
    let Some(game_path) = s.game_path() else {
        return Ok(Vec::new());
    };
    crate::guild_sync::read_guild_sync_data(Path::new(&game_path))
}

#[derive(serde::Serialize)]
struct SyncCharactersPayload<'a> {
    characters: &'a [crate::types::GuildSyncData],
}

/// Reads the same snapshot `get_guild_sync_data` does and POSTs every
/// known guild character in one batch, authenticated the same way
/// `/unpair` already is.
#[tauri::command]
pub async fn sync_guild_data_to_web(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let data = crate::guild_sync::read_guild_sync_data(Path::new(&game_path))?;
    if data.is_empty() {
        return Err("no character data found yet — log in as a Stalkers of Time member first".to_string());
    }

    let url = format!("{}/characters/sync", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .json(&SyncCharactersPayload { characters: &data });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while syncing character data", resp.status()));
    }
    Ok(())
}

/// Reads the GuildExport addon's current full-guild-roster snapshot, if
/// anyone in the guild has run that addon yet. Unlike get_guild_sync_data
/// this isn't scoped to "characters logged into on this WoW account" — one
/// person running GuildExport covers the whole roster.
#[tauri::command]
pub fn get_guild_roster(app: AppHandle) -> Result<Option<crate::guild_roster::GuildRosterData>, String> {
    let s = settings::load_settings(&app)?;
    let Some(game_path) = s.game_path() else {
        return Ok(None);
    };
    crate::guild_roster::read_guild_roster(Path::new(&game_path))
}

#[derive(serde::Serialize)]
struct SyncRosterPayload<'a> {
    members: &'a [crate::guild_roster::GuildRosterMember],
}

/// Reads the same snapshot `get_guild_roster` does and POSTs the full
/// roster in one batch, authenticated the same way character sync is.
#[tauri::command]
pub async fn sync_guild_roster_to_web(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let data = crate::guild_roster::read_guild_roster(Path::new(&game_path))?
        .ok_or_else(|| "no guild roster data found yet — run GuildExport first".to_string())?;

    if data.members.is_empty() {
        return Err("guild roster is empty".to_string());
    }

    let url = format!("{}/guild-roster/sync", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .json(&SyncRosterPayload { members: &data.members });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while syncing guild roster", resp.status()));
    }
    Ok(())
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
    let url = format!("{}/pair", crate::config::API_BASE_URL.trim_end_matches('/'));
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
        let url = format!("{}/unpair", crate::config::API_BASE_URL.trim_end_matches('/'));
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
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;

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
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
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

    if s.hide_on_launch {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
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

/// Flatpak's app ID for Lutris, used both to detect a Flatpak install and to
/// build the `flatpak run` prefix for launch commands.
const LUTRIS_FLATPAK_ID: &str = "net.lutris.Lutris";
const LUTRIS_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Runs `cmd`, polling instead of a plain blocking wait so a Lutris that
/// hangs on first-run setup (Flatpak permission prompts, etc.) can't freeze
/// the app indefinitely - it's killed and reported as a timeout instead.
/// Returns a plain io::Error (not yet stringified) so the caller can tell a
/// missing binary (NotFound, to fall back to Flatpak) apart from any other
/// failure.
fn run_with_timeout(mut cmd: std::process::Command, timeout: std::time::Duration) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => break,
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out waiting for Lutris"));
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
    child.wait_with_output()
}

#[derive(serde::Deserialize)]
struct RawLutrisGame {
    id: u64,
    slug: String,
    name: String,
    runner: String,
}

/// Lists games from Lutris's own library (tries a native `lutris` binary
/// first, then the Flatpak), building a ready-to-save launch command for
/// each so Settings can offer a picker instead of asking the user to find
/// and type a Lutris URI by hand.
#[tauri::command]
pub fn list_lutris_games() -> Result<Vec<crate::types::LutrisGame>, String> {
    let mut native_cmd = std::process::Command::new("lutris");
    native_cmd.args(["--list-games", "--json"]);

    let (output, prefix) = match run_with_timeout(native_cmd, LUTRIS_LIST_TIMEOUT) {
        Ok(output) => (output, String::new()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut flatpak_cmd = std::process::Command::new("flatpak");
            flatpak_cmd.args(["run", LUTRIS_FLATPAK_ID, "--list-games", "--json"]);
            let output = run_with_timeout(flatpak_cmd, LUTRIS_LIST_TIMEOUT).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Lutris wasn't found — checked for a native `lutris` command and the \
                     net.lutris.Lutris Flatpak. Enter your launch command manually instead."
                        .to_string()
                } else {
                    format!("couldn't run Lutris: {e}")
                }
            })?;
            (output, format!("flatpak run {LUTRIS_FLATPAK_ID} "))
        }
        Err(e) => return Err(format!("couldn't run Lutris: {e}")),
    };

    if !output.status.success() {
        return Err(format!(
            "lutris --list-games failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Err(
            "Lutris returned no game list — if Lutris is already open, close it and try again."
                .to_string(),
        );
    }

    let raw: Vec<RawLutrisGame> = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("couldn't parse Lutris's game list: {e}"))?;

    Ok(raw
        .into_iter()
        .map(|g| crate::types::LutrisGame {
            launch_command: format!("{prefix}lutris:rungameid/{}", g.id),
            id: g.id,
            slug: g.slug,
            name: g.name,
            runner: g.runner,
        })
        .collect())
}
