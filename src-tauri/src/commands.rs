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
    /// The active SavedVariables watcher (see watcher.rs) — held here so
    /// `set_game_path_windows`/`set_game_path_linux` can replace it (which
    /// drops and stops the old one) when the watched folder changes.
    pub guild_watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
    /// True while a background thread is polling for the game process to
    /// exit (see launch_game/wtf_sync::is_wow_running) — guards against
    /// stacking up a second poller if Play is clicked again while one's
    /// already watching.
    pub game_close_watching: std::sync::atomic::AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            // No timeout (reqwest::Client::new()'s default) means a
            // stalled connection hangs the calling command forever instead
            // of erroring — observed on Windows, e.g. a Discord pairing
            // exchange that just never completes and never surfaces an
            // error either. 30s is generous for the small JSON payloads
            // every command using this client sends.
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            tray_primary_item: std::sync::Mutex::new(None),
            guild_watcher: std::sync::Mutex::new(None),
            game_close_watching: std::sync::atomic::AtomicBool::new(false),
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
    if let Some(current) = s.game_path() {
        crate::watcher::watch_guild_data(&app, Path::new(&current));
    }
    Ok(s)
}

#[tauri::command]
pub fn set_game_path_linux(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path_linux = Some(path);
    settings::save_settings(&app, &s)?;
    if let Some(current) = s.game_path() {
        crate::watcher::watch_guild_data(&app, Path::new(&current));
    }
    Ok(s)
}

/// The guild site's base URL — safe to expose to the frontend (it's a
/// public address, not a secret) for building browser-open links. The
/// API key itself never leaves the Rust side.
#[tauri::command]
pub fn get_api_base_url() -> &'static str {
    crate::config::API_BASE_URL
}

/// Whether `downloadAndInstall()` can actually succeed here. Windows
/// (NSIS) and macOS (.app.tar.gz) installs always support Tauri's
/// self-updater; on Linux only an AppImage does, since it works by
/// replacing the running binary in place — a .deb/.rpm/pacman install
/// lives under /usr/bin, owned by root, so that overwrite fails with a
/// read-only/permission error instead. The frontend uses this to offer a
/// manual download link there rather than a button that's doomed to fail.
#[tauri::command]
pub fn is_self_update_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
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
    let req = state.http.get(&url)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json");
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

/// Moves a tracked addon (managed or custom) to Interface/.DeletedAddons
/// right away, without waiting for a sync — recoverable later via
/// restore_addon, or purged for good via delete_addon_forever. For managed
/// addons this also unchecks it so it doesn't silently come back on the
/// next sync.
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

/// Permanently purges a trashed addon's files and drops its tracking entry
/// — the actual point of no return. Only acts on something already in
/// Interface/.DeletedAddons.
#[tauri::command]
pub fn delete_addon_forever(app: AppHandle, manifest: Manifest, name: String) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    sync::delete_addon_forever(&game_path, &name)?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Moves a trashed addon back to Interface/AddOns. If a different addon
/// currently occupies its folder (e.g. a managed addon re-synced into that
/// name while this one sat in Trash), fails with an error naming the
/// occupant unless `force` is set — the frontend confirms with the user
/// before retrying with `force: true`, which permanently deletes the
/// occupant first.
#[tauri::command]
pub fn restore_addon(app: AppHandle, manifest: Manifest, name: String, force: bool) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    sync::restore_addon(&game_path, &name, force)?;
    Ok(sync::get_installed_state(&game_path, &manifest))
}

/// Reads the SoTCharacterSync addon's current SavedVariables snapshot for
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
/// `/unpair` already is. Every attempt — success or failure, including
/// preconditions like "not signed in" — is recorded via sync_log so the
/// Settings > Sync Logs view can explain a silent auto-sync miss.
#[tauri::command]
pub async fn sync_guild_data_to_web(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let result = sync_guild_data_to_web_inner(&app, &state).await;
    crate::sync_log::append_sync_log(&app, crate::types::SyncLogEntry {
        at: crate::sync_log::now_ms(),
        kind: "characters".to_string(),
        success: result.is_ok(),
        error: result.as_ref().err().cloned(),
        raid_id: None,
    });
    result
}

async fn sync_guild_data_to_web_inner(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    let s = settings::load_settings(app)?;
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
        .header("Accept", "application/json")
        .json(&SyncCharactersPayload { characters: &data });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while syncing character data", resp.status()));
    }
    Ok(())
}

/// Reads the SoTGuildSync addon's current full-guild-roster snapshot, if
/// anyone in the guild has run that addon yet. Unlike get_guild_sync_data
/// this isn't scoped to "characters logged into on this WoW account" — one
/// person running SoTGuildSync covers the whole roster.
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
/// Same sync_log recording as sync_guild_data_to_web, see there.
#[tauri::command]
pub async fn sync_guild_roster_to_web(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let result = sync_guild_roster_to_web_inner(&app, &state).await;
    crate::sync_log::append_sync_log(&app, crate::types::SyncLogEntry {
        at: crate::sync_log::now_ms(),
        kind: "guild-roster".to_string(),
        success: result.is_ok(),
        error: result.as_ref().err().cloned(),
        raid_id: None,
    });
    result
}

async fn sync_guild_roster_to_web_inner(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let data = crate::guild_roster::read_guild_roster(Path::new(&game_path))?
        .ok_or_else(|| "no guild roster data found yet — run SoTGuildSync first".to_string())?;

    if data.members.is_empty() {
        return Err("guild roster is empty".to_string());
    }

    let url = format!("{}/guild-roster/sync", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .json(&SyncRosterPayload { members: &data.members });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while syncing guild roster", resp.status()));
    }
    Ok(())
}

/// Reads the recorded sync history (newest first) for Settings > Sync Logs.
#[tauri::command]
pub fn get_sync_log(app: AppHandle) -> Result<Vec<crate::types::SyncLogEntry>, String> {
    crate::sync_log::load_sync_log(&app)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncableRaid {
    pub raid_id: String,
    pub captured_at: i64,
    pub raid_json: String,
    pub loot_json: Option<String>,
    /// True if a successful "raid" sync_log entry already exists for this
    /// raid_id — lets the Raids view show it as already-synced instead of
    /// silently vanishing (SoTRaidLootSyncDB itself has no notion of sync
    /// status, only the launcher's own sync_log does).
    pub already_synced: bool,
    /// True if the user has explicitly hidden this raid (see
    /// ignored_raids) — e.g. a pug/alt run HeadCount happened to finalize
    /// that isn't worth syncing.
    pub is_ignored: bool,
}

/// Lists raids the SoTRaidLootSync addon has captured (recent ones only —
/// see that addon's own age window), each annotated with whether this
/// launcher has already successfully synced it. Empty covers "addon not
/// installed" and "nothing captured yet" alike — neither is an error for
/// the Raids view, which just wants to know what's available.
#[tauri::command]
pub fn get_captured_raids(app: AppHandle) -> Result<Vec<SyncableRaid>, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;

    let raids = crate::raid_loot_sync::read_captured_raids(Path::new(&game_path))?;
    let log = crate::sync_log::load_sync_log(&app).unwrap_or_default();
    let ignored_ids = crate::ignored_raids::load_ignored_raids(&app).unwrap_or_default();

    let synced_ids: std::collections::HashSet<String> = log
        .iter()
        .filter(|e| e.kind == "raid" && e.success)
        .filter_map(|e| e.raid_id.clone())
        .collect();

    Ok(raids
        .into_iter()
        .map(|r| SyncableRaid {
            already_synced: synced_ids.contains(&r.raid_id),
            is_ignored: ignored_ids.contains(&r.raid_id),
            raid_id: r.raid_id,
            captured_at: r.captured_at,
            raid_json: r.raid_json,
            loot_json: r.loot_json,
        })
        .collect())
}

/// Marks (or unmarks) a captured raid as ignored — purely local to this
/// launcher install, doesn't touch anything on the website.
#[tauri::command]
pub fn set_raid_ignored(app: AppHandle, raid_id: String, ignored: bool) -> Result<(), String> {
    crate::ignored_raids::set_raid_ignored(&app, &raid_id, ignored)
}

#[derive(serde::Serialize)]
struct SyncRaidPayload<'a> {
    #[serde(rename = "raidJson")]
    raid_json: &'a str,
    #[serde(rename = "lootJson")]
    loot_json: Option<&'a str>,
    #[serde(rename = "isMain")]
    is_main: bool,
}

/// Pushes one captured raid (see get_captured_raids) to the website —
/// RaidSyncController runs RaidImport and (always) LootImport server-side
/// against it. Officer-gated server-side, not just in this UI. Every
/// attempt is recorded via sync_log the same way character/roster sync
/// is, which is also what get_captured_raids checks to mark a raid as
/// already synced.
#[tauri::command]
pub async fn sync_raid_to_web(
    app: AppHandle,
    state: State<'_, AppState>,
    raid_id: String,
    is_main: bool,
) -> Result<(), String> {
    let result = sync_raid_to_web_inner(&app, &state, &raid_id, is_main).await;

    crate::sync_log::append_sync_log(&app, crate::types::SyncLogEntry {
        at: crate::sync_log::now_ms(),
        kind: "raid".to_string(),
        success: result.is_ok(),
        error: result.as_ref().err().cloned(),
        raid_id: Some(raid_id),
    });

    result
}

async fn sync_raid_to_web_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    raid_id: &str,
    is_main: bool,
) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let raids = crate::raid_loot_sync::read_captured_raids(Path::new(&game_path))?;
    let raid = raids
        .into_iter()
        .find(|r| r.raid_id == raid_id)
        .ok_or_else(|| "raid not found — it may have aged out of the addon's capture window".to_string())?;

    let url = format!("{}/raids/sync", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .json(&SyncRaidPayload {
            raid_json: &raid.raid_json,
            loot_json: raid.loot_json.as_deref(),
            is_main,
        });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while syncing raid", resp.status()));
    }
    Ok(())
}

/// Fetches the signed-in user's own characters' loot, grouped by raid, for
/// the "My Loot" tab. Relayed as raw JSON rather than a typed struct —
/// unlike the addon-file readers, this data never gets inspected or
/// transformed on the Rust side, just fetched and handed to the frontend.
#[tauri::command]
pub async fn get_my_loot(app: AppHandle, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let s = settings::load_settings(&app)?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let url = format!("{}/my-loot", crate::config::API_BASE_URL.trim_end_matches('/'));
    let req = state.http.get(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json");

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while fetching loot", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
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
    #[serde(rename = "isOfficer", default)]
    is_officer: bool,
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
    s.is_officer = body.user.is_officer;
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
    s.is_officer = false;
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

/// Known WoW executable names, checked case-insensitively. Also used by
/// wtf_sync::is_wow_running() to guard against syncing the WTF folder
/// while the client might still be writing to it.
pub(crate) const WOW_EXE_NAMES: &[&str] = &["wow.exe", "wowclassic.exe"];

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

    // Only bother watching if there's actually something to do once the
    // game closes — no point polling in the background for a feature the
    // player hasn't opted into.
    if s.wtf_sync_auto_push && s.wtf_sync_account.is_some() && s.discord_token.as_deref().is_some_and(|t| !t.is_empty()) {
        crate::wtf_sync::watch_for_game_close(app);
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

/// Every `WTF/Account/*` subfolder, for the Settings picker — a single WoW
/// install can have several, so the player picks theirs explicitly.
#[tauri::command]
pub fn get_wtf_accounts(app: AppHandle) -> Result<Vec<String>, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    Ok(crate::wtf_sync::list_wtf_accounts(&game_path))
}

/// Switching accounts invalidates the last-synced bookkeeping — it was
/// computed against the previous folder's contents and would otherwise
/// cause a false "no local changes" or "no remote changes" read on the
/// new one.
#[tauri::command]
pub fn set_wtf_sync_account(app: AppHandle, account: Option<String>) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_sync_account = account.filter(|a| !a.is_empty());
    s.wtf_last_synced_fingerprint = None;
    s.wtf_last_known_remote_updated_at = None;
    s.wtf_last_synced_at = None;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_wtf_sync_auto_pull(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_sync_auto_pull = enabled;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[tauri::command]
pub fn set_wtf_sync_auto_push(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_sync_auto_push = enabled;
    settings::save_settings(&app, &s)?;
    Ok(s)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WtfSyncInfo {
    pub status: crate::types::WtfSyncStatus,
    /// Whether the local account folder has changed since the last
    /// successful push or pull — drives the Settings UI's "you have
    /// unsynced local changes" hint, independent of any actual push/pull
    /// attempt (which does its own check server-side-aware of the remote too).
    pub local_changed: bool,
}

/// Cloud status + local-change check, for the Settings panel to show
/// "up to date" / "local changes pending" / "cloud has newer changes"
/// without actually pushing or pulling anything.
#[tauri::command]
pub async fn get_wtf_sync_info(app: AppHandle, state: State<'_, AppState>) -> Result<WtfSyncInfo, String> {
    let s = settings::load_settings(&app)?;
    let token = s
        .discord_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let status = crate::wtf_sync::fetch_wtf_status(&state.http, token).await?;

    let local_changed = match (s.game_path(), s.wtf_sync_account.as_deref()) {
        (Some(game_path), Some(account)) => crate::wtf_sync::account_dir(&game_path, account)
            .ok()
            .and_then(|dir| crate::wtf_sync::fingerprint_account(&dir).ok())
            .map(|fp| Some(fp) != s.wtf_last_synced_fingerprint)
            .unwrap_or(false),
        _ => false,
    };

    Ok(WtfSyncInfo { status, local_changed })
}

/// Zips the selected WTF account folder and uploads it, overwriting
/// whatever this player previously had stored. Refuses (naming why) unless
/// `force` is set if the cloud copy changed since this device last touched
/// it — see wtf_sync.rs's module doc for the conflict model.
#[tauri::command]
pub async fn wtf_push(app: AppHandle, state: State<'_, AppState>, force: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let account = s
        .wtf_sync_account
        .clone()
        .ok_or_else(|| "pick your WTF account folder in Settings first".to_string())?;
    let token = s
        .discord_token
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    if crate::wtf_sync::is_wow_running() {
        return Err("close World of Warcraft before syncing your WTF folder".to_string());
    }

    let dir = crate::wtf_sync::account_dir(&game_path, &account)?;

    if !force {
        let remote = crate::wtf_sync::fetch_wtf_status(&state.http, &token).await?;
        if remote.exists && remote.updated_at != s.wtf_last_known_remote_updated_at {
            return Err(
                "Your cloud WTF backup has changed since this device last synced it - pushing now will overwrite it."
                    .to_string(),
            );
        }
    }

    let zip = crate::wtf_sync::zip_account(&dir)?;
    let fingerprint = crate::wtf_sync::fingerprint_account(&dir)?;
    let status = crate::wtf_sync::upload_wtf_zip(&state.http, &token, zip).await?;

    s.wtf_last_synced_fingerprint = Some(fingerprint);
    s.wtf_last_known_remote_updated_at = status.updated_at;
    s.wtf_last_synced_at = Some(crate::sync_log::now_ms());
    settings::save_settings(&app, &s)?;
    Ok(s)
}

/// Downloads this player's cloud WTF backup and replaces the selected
/// account folder's contents with it. Refuses (naming why) unless `force`
/// is set if the local folder changed since this device last synced —
/// see wtf_sync.rs's module doc for the conflict model.
#[tauri::command]
pub async fn wtf_pull(app: AppHandle, state: State<'_, AppState>, force: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s.game_path().ok_or_else(|| "no game folder configured".to_string())?;
    let account = s
        .wtf_sync_account
        .clone()
        .ok_or_else(|| "pick your WTF account folder in Settings first".to_string())?;
    let token = s
        .discord_token
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    if crate::wtf_sync::is_wow_running() {
        return Err("close World of Warcraft before syncing your WTF folder".to_string());
    }

    let dir = crate::wtf_sync::account_dir(&game_path, &account)?;

    if !force {
        let current_fp = crate::wtf_sync::fingerprint_account(&dir)?;
        let has_local_changes = match &s.wtf_last_synced_fingerprint {
            Some(known) => &current_fp != known,
            None => crate::wtf_sync::has_any_synced_files(&dir)?,
        };
        if has_local_changes {
            let message = if s.wtf_last_synced_fingerprint.is_none() {
                "This device has never synced this WTF folder before - pulling now will overwrite its current \
                 contents with the cloud copy."
            } else {
                "You have local WTF changes that haven't been pushed yet - pulling now will overwrite them with \
                 the cloud copy."
            };
            return Err(message.to_string());
        }
    }

    let zip = crate::wtf_sync::download_wtf_zip(&state.http, &token).await?;
    crate::wtf_sync::extract_account_zip(&zip, &dir)?;

    let fingerprint = crate::wtf_sync::fingerprint_account(&dir)?;
    let status = crate::wtf_sync::fetch_wtf_status(&state.http, &token).await?;

    s.wtf_last_synced_fingerprint = Some(fingerprint);
    s.wtf_last_known_remote_updated_at = status.updated_at;
    s.wtf_last_synced_at = Some(crate::sync_log::now_ms());
    settings::save_settings(&app, &s)?;
    Ok(s)
}
