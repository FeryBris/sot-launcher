use crate::settings;
use crate::steam_launch;
use crate::sync;
use crate::types::{GameClient, InstalledState, Manifest, Settings, SyncResult};
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
    /// The tray menu and its "Update" item - set once during `.setup()` in
    /// lib.rs. `set_tray_update_available` puts the item at the top of the
    /// menu while an update is available and takes it out otherwise.
    pub tray_menu: std::sync::Mutex<Option<(tauri::menu::Menu<tauri::Wry>, tauri::menu::MenuItem<tauri::Wry>)>>,
    /// The active SavedVariables watcher (see watcher.rs) — held here so
    /// `set_game_path_windows`/`set_game_path_linux` can replace it (which
    /// drops and stops the old one) when the watched folder changes.
    pub guild_watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
    /// True while a background thread is polling for the game process to
    /// exit (see launch_game/wtf_sync::is_wow_running) — guards against
    /// stacking up a second poller if Play is clicked again while one's
    /// already watching.
    pub game_close_watching: std::sync::atomic::AtomicBool,
    /// The active combat-log watcher (see combat_log_watcher.rs) — same
    /// replace-to-restart lifecycle as guild_watcher above.
    pub combat_log_watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
    /// Shared with the combat-log watcher's background thread (via Arc, not
    /// just Mutex like guild_watcher) since both that thread and this
    /// state's owner need live access to the same in-progress chunk, not
    /// just to swap the whole thing out.
    pub combat_log_tracker:
        std::sync::Arc<std::sync::Mutex<crate::combat_log_chunker::ChunkTracker>>,
    /// Recently completed session chunks, for get_captured_combat_sessions
    /// to hand the frontend whatever it missed by not having been mounted
    /// yet when combat-log-session-completed fired.
    pub combat_log_recent:
        std::sync::Arc<std::sync::Mutex<Vec<crate::combat_log_chunker::FinishedChunk>>>,
    /// (session_id, raw log text) for the same recently completed chunks as
    /// combat_log_recent above, kept separate so the frontend never has to
    /// receive this (potentially multi-megabyte) text just to show a
    /// summary row - see combat_log_watcher::RawLogStore's own doc comment.
    pub combat_log_raw_logs: crate::combat_log_watcher::RawLogStore,
    /// Bumped by every watch_combat_log call - lets a still-polling "wait
    /// for WoWCombatLog.txt to be created" background thread (see
    /// combat_log_watcher.rs) notice it's been superseded by a newer call
    /// (e.g. the game folder changed again) and exit, instead of stacking
    /// up a second poller running against a stale path forever.
    pub combat_log_watch_epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
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
            tray_menu: std::sync::Mutex::new(None),
            guild_watcher: std::sync::Mutex::new(None),
            game_close_watching: std::sync::atomic::AtomicBool::new(false),
            combat_log_watcher: std::sync::Mutex::new(None),
            combat_log_tracker: std::sync::Arc::new(std::sync::Mutex::new(
                crate::combat_log_chunker::ChunkTracker::new(),
            )),
            combat_log_recent: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            combat_log_raw_logs: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            combat_log_watch_epoch: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncableCombatLogSession {
    #[serde(flatten)]
    pub chunk: crate::combat_log_chunker::FinishedChunk,
    /// True if a successful "combat_session" sync_log entry already exists
    /// for this session_id — same convention as SyncableRaid::already_synced.
    pub already_synced: bool,
    /// True if the user has explicitly hidden this session (see
    /// ignored_combat_sessions).
    pub is_ignored: bool,
}

/// Whatever completed session chunks the combat-log watcher has captured
/// since the launcher started (or since the watched game folder last
/// changed), each annotated with sync/ignore status — covers the frontend
/// having missed the combat-log-session-completed event (e.g. it wasn't
/// mounted yet), same as get_captured_raids does for raids.
#[tauri::command]
pub fn get_captured_combat_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SyncableCombatLogSession>, String> {
    let chunks = state.combat_log_recent.lock().unwrap().clone();
    let log = crate::sync_log::load_sync_log(&app).unwrap_or_default();
    let ignored_ids =
        crate::ignored_combat_sessions::load_ignored_sessions(&app).unwrap_or_default();

    let synced_ids: std::collections::HashSet<String> = log
        .iter()
        .filter(|e| e.kind == "combat_session" && e.success)
        .filter_map(|e| e.session_id.clone())
        .collect();

    Ok(chunks
        .into_iter()
        .map(|chunk| SyncableCombatLogSession {
            already_synced: synced_ids.contains(&chunk.session_id),
            is_ignored: ignored_ids.contains(&chunk.session_id),
            chunk,
        })
        .collect())
}

/// Marks (or unmarks) a captured combat session as ignored — purely local
/// to this launcher install, doesn't touch anything on the website. Mirrors
/// set_raid_ignored.
#[tauri::command]
pub fn set_combat_session_ignored(
    app: AppHandle,
    session_id: String,
    ignored: bool,
) -> Result<(), String> {
    let result = crate::ignored_combat_sessions::set_session_ignored(&app, &session_id, ignored);
    if result.is_ok() {
        crate::debug_log::verbose(
            &app,
            "set_combat_session_ignored",
            &format!("session {session_id}: ignored={ignored}"),
        );
    }
    result
}

/// Pushes one captured session chunk (see get_captured_combat_sessions) to
/// the website - unlike the old per-encounter design, there's no separate
/// auto-sync path calling into this anymore (sync is manual only, matching
/// Raids - see Settings' own doc comment history for why that toggle was
/// removed).
#[tauri::command]
pub async fn sync_combat_session_to_web(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let chunk = state
        .combat_log_recent
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.session_id == session_id)
        .cloned()
        .ok_or_else(|| {
            "session not found — it may have aged out of the recent-sessions buffer".to_string()
        })?;

    sync_chunk_to_web(&app, &state.http, &chunk).await
}

/// Posts one session chunk to the website and records the outcome in the
/// sync log.
async fn sync_chunk_to_web(
    app: &AppHandle,
    http: &reqwest::Client,
    chunk: &crate::combat_log_chunker::FinishedChunk,
) -> Result<(), String> {
    let result = sync_chunk_to_web_inner(app, http, chunk).await;

    crate::sync_log::append_sync_log(
        app,
        crate::types::SyncLogEntry {
            at: crate::sync_log::now_ms(),
            kind: "combat_session".to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().cloned(),
            raid_id: None,
            session_id: Some(chunk.session_id.clone()),
        },
    );

    if result.is_ok() {
        crate::debug_log::verbose(
            app,
            "sync_combat_session_to_web",
            &format!("synced combat log session {} to web", chunk.session_id),
        );
    }
    crate::debug_log::log_on_err(app, "sync_combat_session_to_web", result)
}

/// What actually travels to the backend - just enough to identify and
/// bound the raw log (see CombatLogSyncController's own doc comment for
/// why the rest of the old payload - players/npcNames/contentType - isn't
/// needed anymore: with a raw log attached, CombatLogImport's own
/// PullSegmenter/CombatBreakdownService recompute all of that server-side
/// from the raw text, which this chunk never tried to duplicate client-side
/// in the first place - see combat_log_chunker's own module doc comment).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkSyncPayload {
    session_id: String,
    start_time_ms: i64,
    end_time_ms: i64,
}

/// Sent as multipart (not plain JSON) so the raw log - easily tens of
/// megabytes for a whole raid night's own chunk (see combat_log_chunker's
/// own module doc comment) - can go alongside the summary as a compressed
/// file part instead of bloating a JSON body.
async fn sync_chunk_to_web_inner(
    app: &AppHandle,
    http: &reqwest::Client,
    chunk: &crate::combat_log_chunker::FinishedChunk,
) -> Result<(), String> {
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let raw_log = app
        .state::<AppState>()
        .combat_log_raw_logs
        .lock()
        .unwrap()
        .iter()
        .find(|(session_id, _)| session_id == &chunk.session_id)
        .map(|(_, raw_log)| raw_log.clone());

    let payload = serde_json::to_string(&ChunkSyncPayload {
        session_id: chunk.session_id.clone(),
        start_time_ms: chunk.start_time_ms,
        end_time_ms: chunk.end_time_ms,
    })
    .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new().text("payload", payload);

    if let Some(raw_log) = raw_log.filter(|text| !text.is_empty()) {
        let gzipped = gzip_text(&raw_log)?;
        let part = reqwest::multipart::Part::bytes(gzipped)
            .file_name("combat.log.gz")
            .mime_str("application/gzip")
            .map_err(|e| e.to_string())?;
        form = form.part("rawLog", part);
    }

    let url = format!(
        "{}/combat-log/sync",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let resp = http
        .post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .multipart(form)
        // Overrides AppState.http's own 30s default (see its own doc
        // comment: sized for the small JSON payloads every OTHER command
        // using this shared client sends) for just this one request -
        // this upload can now run to tens of megabytes for a whole raid
        // night's own chunk (see combat_log_chunker's own module doc
        // comment), which 30s is nowhere near generous enough for on a
        // slower upload connection.
        .timeout(std::time::Duration::from_secs(10 * 60))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {} while syncing combat session",
            resp.status()
        ));
    }
    Ok(())
}

fn gzip_text(text: &str) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

/// Forces the whole current combat log to be reprocessed as historical
/// backlog, discarding any saved checkpoint first — see
/// combat_log_watcher::reprocess_combat_log for why this needs to exist as
/// its own action rather than only ever happening automatically once.
#[tauri::command]
pub fn reprocess_combat_log(app: AppHandle) -> Result<(), String> {
    let s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    crate::combat_log_watcher::reprocess_combat_log(&app, Path::new(&game_path));
    crate::debug_log::verbose(
        &app,
        "reprocess_combat_log",
        "reprocessing the whole combat log from scratch",
    );
    Ok(())
}

/// Shows or hides the tray's "Update" item - the frontend does the actual
/// update check, this just mirrors its result onto the native tray menu.
#[tauri::command]
pub fn set_tray_update_available(state: State<'_, AppState>, available: bool) -> Result<(), String> {
    let guard = state.tray_menu.lock().map_err(|e| e.to_string())?;
    let Some((menu, update_item)) = guard.as_ref() else {
        return Ok(());
    };
    let shown = menu.get(update_item.id()).is_some();
    if available && !shown {
        menu.insert(update_item, 0).map_err(|e| e.to_string())?;
    } else if !available && shown {
        menu.remove(update_item).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load_settings(&app)
}

/// Points the file watchers at the active game's folder. The combat log
/// watcher only runs for WotLK: Forever's log format differs and the site has
/// no Forever parser yet (docs/FOREVER_PLAN.md, Launcher), so for Forever it
/// is stopped rather than left reading the wrong file.
pub fn restart_watchers(app: &AppHandle, s: &Settings) {
    match s.game_path() {
        Some(path) => {
            crate::watcher::watch_guild_data(app, Path::new(&path));
            if s.active_client == GameClient::Wotlk {
                crate::combat_log_watcher::watch_combat_log(app, Path::new(&path));
            } else {
                crate::combat_log_watcher::stop_combat_log_watch(app);
            }
        }
        None => {
            crate::watcher::stop_guild_watch(app);
            crate::combat_log_watcher::stop_combat_log_watch(app);
        }
    }
}

/// Switches the whole launcher to `client` (see Settings::switch_client) and
/// re-points the watchers at that game's folder.
#[tauri::command]
pub fn set_active_client(app: AppHandle, client: GameClient) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.switch_client(client);
    // Picking a game on the first-open picker comes through here too, even
    // when it is the game already active.
    s.client_chosen = true;
    settings::save_settings(&app, &s)?;
    restart_watchers(&app, &s);
    crate::debug_log::verbose(&app, "set_active_client", &format!("switched to {client:?}"));
    Ok(s)
}

/// Which game a folder belongs to, so picking the wrong one can be caught.
/// Blizzard's modern clients write `.flavor.info` next to the executable;
/// Forever's contains `wowf` (measured 2026-11-09). WotLK 3.3.5 clients have
/// no such file. The flavor file is read next to whichever executable
/// find_wow_exe finds, since that also looks one folder down (a picked
/// "World of Warcraft" folder holding `_forever_`). Returns None when there is
/// no WoW executable at all.
///
/// `folder` is the directory that actually holds the executable - the one to
/// save as the game folder, since addons install into its Interface/AddOns
/// (picking the parent "World of Warcraft" folder would put them where the
/// game never reads them).
#[tauri::command]
pub fn detect_game_client(path: String) -> Option<DetectedGame> {
    let exe = find_wow_exe(Path::new(&path))?;
    let dir = exe.parent()?;
    let flavor = std::fs::read_to_string(dir.join(".flavor.info")).ok();
    Some(DetectedGame {
        client: match flavor {
            Some(flavor) if flavor.contains("wowf") => GameClient::Forever,
            _ => GameClient::Wotlk,
        },
        folder: dir.to_string_lossy().into_owned(),
    })
}

#[derive(serde::Serialize)]
pub struct DetectedGame {
    pub client: GameClient,
    pub folder: String,
}

#[tauri::command]
pub fn set_game_path_windows(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path_windows = Some(path.clone());
    settings::save_settings(&app, &s)?;
    restart_watchers(&app, &s);
    crate::debug_log::verbose(
        &app,
        "set_game_path_windows",
        &format!("game folder set to {path}"),
    );
    Ok(s)
}

#[tauri::command]
pub fn set_game_path_linux(app: AppHandle, path: String) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.game_path_linux = Some(path.clone());
    settings::save_settings(&app, &s)?;
    restart_watchers(&app, &s);
    crate::debug_log::verbose(
        &app,
        "set_game_path_linux",
        &format!("game folder set to {path}"),
    );
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
    s.launch_command = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(
        &app,
        "set_launch_command",
        &format!("custom launch command set to {:?}", s.launch_command),
    );
    Ok(s)
}

#[tauri::command]
pub fn set_hide_on_launch(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.hide_on_launch = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "set_hide_on_launch", &format!("set to {enabled}"));
    Ok(s)
}

#[tauri::command]
pub fn set_close_to_tray(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.close_to_tray = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "set_close_to_tray", &format!("set to {enabled}"));
    Ok(s)
}

#[tauri::command]
pub fn set_auto_launch(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.auto_launch = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "set_auto_launch", &format!("set to {enabled}"));
    Ok(s)
}

#[tauri::command]
pub fn set_verbose_logging(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.verbose_logging = enabled;
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
    let picked = picked.map(|p| p.to_string());
    match &picked {
        Some(p) => crate::debug_log::verbose(&app, "pick_game_folder", &format!("picked {p}")),
        None => crate::debug_log::verbose(&app, "pick_game_folder", "picker cancelled"),
    }
    Ok(picked)
}

#[tauri::command]
pub async fn fetch_manifest(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Manifest, String> {
    let result = fetch_manifest_inner(&state).await;
    if let Ok(m) = &result {
        crate::debug_log::verbose(
            &app,
            "fetch_manifest",
            &format!(
                "fetched {} addons, {} wtf files",
                m.addons.len(),
                m.wtf_files.len()
            ),
        );
    }
    crate::debug_log::log_on_err(&app, "fetch_manifest", result)
}

async fn fetch_manifest_inner(state: &State<'_, AppState>) -> Result<Manifest, String> {
    let url = format!(
        "{}/manifest",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let req = state
        .http
        .get(&url)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json");
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(
            "Unauthorized: the launcher's guild API key is invalid — contact an officer"
                .to_string(),
        );
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
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    Ok(sync::get_installed_state(Some(&app), &game_path, &manifest))
}

/// Immediate disable/enable for a single addon (managed or custom). Managed
/// addons normally go through the batched Sync flow via addon_enabled, but
/// custom addons have no manifest entry to sync against, so this is the only
/// mechanism available for them.
#[tauri::command]
pub fn set_addon_disabled(
    app: AppHandle,
    manifest: Manifest,
    name: String,
    disabled: bool,
) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    sync::set_addon_disabled(&game_path, &name, disabled)?;
    crate::debug_log::verbose(
        &app,
        "set_addon_disabled",
        &format!("{name}: disabled={disabled}"),
    );
    Ok(sync::get_installed_state(Some(&app), &game_path, &manifest))
}

/// Moves a tracked addon (managed or custom) to Interface/.DeletedAddons
/// right away, without waiting for a sync — recoverable later via
/// restore_addon, or purged for good via delete_addon_forever. For managed
/// addons this also unchecks it so it doesn't silently come back on the
/// next sync.
#[tauri::command]
pub fn delete_addon(
    app: AppHandle,
    manifest: Manifest,
    name: String,
) -> Result<InstalledState, String> {
    let mut s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;

    if let Some(removed) = sync::delete_addon(&game_path, &name)? {
        if removed.kind != "custom" {
            s.addon_enabled.insert(name.clone(), false);
            settings::save_settings(&app, &s)?;
        }
    }

    crate::debug_log::verbose(&app, "delete_addon", &format!("{name}: moved to trash"));
    Ok(sync::get_installed_state(Some(&app), &game_path, &manifest))
}

/// Permanently purges a trashed addon's files and drops its tracking entry
/// — the actual point of no return. Only acts on something already in
/// Interface/.DeletedAddons.
#[tauri::command]
pub fn delete_addon_forever(
    app: AppHandle,
    manifest: Manifest,
    name: String,
) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    sync::delete_addon_forever(&game_path, &name)?;
    crate::debug_log::verbose(&app, "delete_addon_forever", &format!("{name}: purged"));
    Ok(sync::get_installed_state(Some(&app), &game_path, &manifest))
}

/// Moves a trashed addon back to Interface/AddOns. If a different addon
/// currently occupies its folder (e.g. a managed addon re-synced into that
/// name while this one sat in Trash), fails with an error naming the
/// occupant unless `force` is set — the frontend confirms with the user
/// before retrying with `force: true`, which permanently deletes the
/// occupant first.
#[tauri::command]
pub fn restore_addon(
    app: AppHandle,
    manifest: Manifest,
    name: String,
    force: bool,
) -> Result<InstalledState, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    sync::restore_addon(&game_path, &name, force)?;
    crate::debug_log::verbose(
        &app,
        "restore_addon",
        &format!("{name}: restored (force={force})"),
    );
    Ok(sync::get_installed_state(Some(&app), &game_path, &manifest))
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
struct SyncCharactersPayload {
    characters: Vec<serde_json::Value>,
}

/// Reads the same snapshot `get_guild_sync_data` does and POSTs every
/// known guild character in one batch, authenticated the same way
/// `/unpair` already is. Every attempt — success or failure, including
/// preconditions like "not signed in" — is recorded via sync_log so the
/// Settings > Sync Logs view can explain a silent auto-sync miss.
#[tauri::command]
pub async fn sync_guild_data_to_web(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let result = sync_guild_data_to_web_inner(&app, &state).await;
    crate::sync_log::append_sync_log(
        &app,
        crate::types::SyncLogEntry {
            at: crate::sync_log::now_ms(),
            kind: "characters".to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().cloned(),
            raid_id: None,
            session_id: None,
        },
    );
    if result.is_ok() {
        crate::debug_log::verbose(
            &app,
            "sync_guild_data_to_web",
            "synced character data to web",
        );
    }
    crate::debug_log::log_on_err(&app, "sync_guild_data_to_web", result)
}

async fn sync_guild_data_to_web_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let data = crate::guild_sync::read_guild_sync_data(Path::new(&game_path))?;
    if data.is_empty() {
        return Err(
            "no character data found yet — log in as a Stalkers of Time member first".to_string(),
        );
    }

    // `account` is a local-only field (which WTF/Account/<name> folder this
    // character came from) - strip it before sending so the website's
    // payload shape is unchanged by it.
    let characters: Vec<serde_json::Value> = data
        .iter()
        .filter_map(|c| {
            let mut v = serde_json::to_value(c).ok()?;
            v.as_object_mut()?.remove("account");
            Some(v)
        })
        .collect();

    let url = format!(
        "{}/characters/sync",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let req = state
        .http
        .post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .json(&SyncCharactersPayload { characters });

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {} while syncing character data",
            resp.status()
        ));
    }
    Ok(())
}

/// Reads the SoTGuildSync addon's current full-guild-roster snapshot, if
/// anyone in the guild has run that addon yet. Unlike get_guild_sync_data
/// this isn't scoped to "characters logged into on this WoW account" — one
/// person running SoTGuildSync covers the whole roster.
#[tauri::command]
pub fn get_guild_roster(
    app: AppHandle,
) -> Result<Option<crate::guild_roster::GuildRosterData>, String> {
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
pub async fn sync_guild_roster_to_web(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let result = sync_guild_roster_to_web_inner(&app, &state).await;
    crate::sync_log::append_sync_log(
        &app,
        crate::types::SyncLogEntry {
            at: crate::sync_log::now_ms(),
            kind: "guild-roster".to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().cloned(),
            raid_id: None,
            session_id: None,
        },
    );
    if result.is_ok() {
        crate::debug_log::verbose(
            &app,
            "sync_guild_roster_to_web",
            "synced guild roster to web",
        );
    }
    crate::debug_log::log_on_err(&app, "sync_guild_roster_to_web", result)
}

async fn sync_guild_roster_to_web_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let data = crate::guild_roster::read_guild_roster(Path::new(&game_path))?
        .ok_or_else(|| "no guild roster data found yet — run SoTGuildSync first".to_string())?;

    if data.members.is_empty() {
        return Err("guild roster is empty".to_string());
    }

    let url = format!(
        "{}/guild-roster/sync",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let req = state
        .http
        .post(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .json(&SyncRosterPayload {
            members: &data.members,
        });

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

/// Reads the persistent error/warning log (newest first, debug.json in the
/// app config dir) - active in every build, unlike tauri_plugin_log, so
/// there's actually something to inspect after a shipped release fails.
#[tauri::command]
pub fn get_debug_log(app: AppHandle) -> Result<Vec<crate::debug_log::DebugLogEntry>, String> {
    crate::debug_log::load_debug_log(&app)
}

/// Bridges the frontend into debug_log.rs - for JS-side operations that
/// never go through a Rust command at all (the updater plugin's
/// check()/downloadAndInstall()/relaunch() are called directly from
/// App.vue), there's otherwise no way to leave a trace of what was
/// attempted. Most useful right before a step that might tear down the
/// whole process before any JS catch block gets a chance to run (Windows'
/// update handoff exits the app to let the installer replace it, whether
/// or not that installer is actually allowed to run) - if debug.json's
/// last entry is "install attempted" with nothing after it, that's
/// already a real diagnostic.
#[tauri::command]
pub fn log_client_event(
    app: AppHandle,
    level: String,
    context: String,
    message: String,
) -> Result<(), String> {
    match level.as_str() {
        "error" => crate::debug_log::error(&app, &context, &message),
        "warn" => crate::debug_log::warn(&app, &context, &message),
        _ => crate::debug_log::info(&app, &context, &message),
    }
    Ok(())
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
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;

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
    let result = crate::ignored_raids::set_raid_ignored(&app, &raid_id, ignored);
    if result.is_ok() {
        crate::debug_log::verbose(
            &app,
            "set_raid_ignored",
            &format!("raid {raid_id}: ignored={ignored}"),
        );
    }
    result
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

    crate::sync_log::append_sync_log(
        &app,
        crate::types::SyncLogEntry {
            at: crate::sync_log::now_ms(),
            kind: "raid".to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().cloned(),
            raid_id: Some(raid_id.clone()),
            session_id: None,
        },
    );

    if result.is_ok() {
        crate::debug_log::verbose(
            &app,
            "sync_raid_to_web",
            &format!("synced raid {raid_id} to web (is_main={is_main})"),
        );
    }
    crate::debug_log::log_on_err(&app, "sync_raid_to_web", result)
}

async fn sync_raid_to_web_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    raid_id: &str,
    is_main: bool,
) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let raids = crate::raid_loot_sync::read_captured_raids(Path::new(&game_path))?;
    let raid = raids
        .into_iter()
        .find(|r| r.raid_id == raid_id)
        .ok_or_else(|| {
            "raid not found — it may have aged out of the addon's capture window".to_string()
        })?;

    let url = format!(
        "{}/raids/sync",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let req = state
        .http
        .post(&url)
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
pub async fn get_my_loot(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let result = get_my_loot_inner(&app, &state).await;
    if result.is_ok() {
        crate::debug_log::verbose(&app, "get_my_loot", "fetched my loot from web");
    }
    crate::debug_log::log_on_err(&app, "get_my_loot", result)
}

async fn get_my_loot_inner(
    _app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let url = format!(
        "{}/my-loot",
        crate::config::API_BASE_URL.trim_end_matches('/')
    );
    let req = state
        .http
        .get(&url)
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

    crate::keychain::set_token(&body.token)?;
    s.discord_signed_in = true;
    s.discord_username = body.user.discord_username;
    s.discord_avatar = body.user.discord_avatar;
    s.is_officer = body.user.is_officer;
    settings::save_settings(app, &s)?;
    Ok(s)
}

#[tauri::command]
pub async fn pair_with_code(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
) -> Result<Settings, String> {
    let result = exchange_pair_code(&app, &state.http, code).await;
    if let Ok(s) = &result {
        crate::debug_log::verbose(
            &app,
            "pair_with_code",
            &format!("paired as {:?}", s.discord_username),
        );
    }
    crate::debug_log::log_on_err(&app, "pair_with_code", result)
}

/// Best-effort server-side revoke, then always clears the local credential
/// regardless of whether the server call succeeded (e.g. offline).
#[tauri::command]
pub async fn clear_discord_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;

    if let Some(token) = crate::keychain::get_token().filter(|t| !t.is_empty()) {
        let url = format!(
            "{}/unpair",
            crate::config::API_BASE_URL.trim_end_matches('/')
        );
        let _ = state
            .http
            .post(&url)
            .header("Accept", "application/json")
            .bearer_auth(token)
            .send()
            .await;
    }

    crate::keychain::delete_token();
    s.discord_signed_in = false;
    s.discord_username = None;
    s.discord_avatar = None;
    s.is_officer = false;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "clear_discord_login", "signed out of discord");
    Ok(s)
}

#[tauri::command]
pub async fn run_sync(
    app: AppHandle,
    manifest: Manifest,
    addon_enabled: std::collections::HashMap<String, bool>,
) -> Result<CombinedSyncResult, String> {
    let result = run_sync_inner(&app, manifest, addon_enabled).await;
    crate::debug_log::log_on_err(&app, "run_sync", result)
}

async fn run_sync_inner(
    app: &AppHandle,
    manifest: Manifest,
    addon_enabled: std::collections::HashMap<String, bool>,
) -> Result<CombinedSyncResult, String> {
    let mut s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;

    let addons = sync::sync_addons(app, &game_path, &manifest.addons, &addon_enabled).await;
    let wtf_files = sync::sync_wtf_files(app, &game_path, &manifest.wtf_files).await;

    // Per-item failures (a bad checksum, a dead download URL) don't fail
    // this whole command - they're already surfaced item-by-item in the
    // UI - but they're still worth a permanent record when someone reports
    // "an addon didn't update" days later.
    for (name, err) in &addons.failed {
        crate::debug_log::error(app, "sync_addons", &format!("{name}: {err}"));
    }
    for (name, err) in &wtf_files.failed {
        crate::debug_log::error(app, "sync_wtf_files", &format!("{name}: {err}"));
    }

    s.addon_enabled = addon_enabled;
    settings::save_settings(app, &s)?;

    crate::debug_log::verbose(
        app,
        "run_sync",
        &format!(
            "synced {} addons ({} failed), {} wtf files ({} failed)",
            addons.updated.len(),
            addons.failed.len(),
            wtf_files.updated.len(),
            wtf_files.failed.len(),
        ),
    );
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
    let result = launch_game_inner(&app);
    crate::debug_log::log_on_err(&app, "launch_game", result)
}

fn launch_game_inner(app: &AppHandle) -> Result<(), String> {
    let s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let game_root = PathBuf::from(&game_path);

    if let Some(custom) = s
        .launch_command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let resolved = custom.replace("{game_path}", &game_path);
        run_shell_command(&resolved, &game_root)?;
    } else if let Some(appid) =
        steam_launch::compat_appid(&game_root).filter(|_| !cfg!(target_os = "windows"))
    {
        let code = find_wow_exe(&game_root)
            .and_then(|exe| exe.parent().and_then(steam_launch::battlenet_launch_code));
        steam_launch::launch(app, appid, code)?;
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
    if s.wtf_sync_auto_push && crate::keychain::get_token().is_some_and(|t| !t.is_empty()) {
        crate::wtf_sync::watch_for_game_close(app.clone());
    }
    crate::debug_log::verbose(app, "launch_game", "launched the game");
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
fn run_with_timeout(
    mut cmd: std::process::Command,
    timeout: std::time::Duration,
) -> std::io::Result<std::process::Output> {
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
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out waiting for Lutris",
                    ));
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

/// Every `WTF/Account/*` subfolder found locally — the sync table's rows
/// are the union of this and get_wtf_cloud_accounts, since a backup pushed
/// from another device may not have a local folder here yet.
#[tauri::command]
pub fn get_wtf_accounts(app: AppHandle) -> Result<Vec<String>, String> {
    let s = settings::load_settings(&app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    Ok(crate::wtf_sync::list_wtf_accounts(&game_path))
}

#[tauri::command]
pub fn set_wtf_sync_auto_pull(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_sync_auto_pull = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "set_wtf_sync_auto_pull", &format!("set to {enabled}"));
    Ok(s)
}

#[tauri::command]
pub fn set_wtf_sync_auto_push(app: AppHandle, enabled: bool) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_sync_auto_push = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(&app, "set_wtf_sync_auto_push", &format!("set to {enabled}"));
    Ok(s)
}

/// Per-account opt-out of wtf_sync_auto_push/auto_pull (see
/// WtfAccountSyncState::auto_sync_enabled) - only touches that one flag,
/// preserving whatever sync bookkeeping already exists for the account (or
/// using WtfAccountSyncState::default() - auto_sync_enabled: true - for one
/// that's never been synced yet).
#[tauri::command]
pub fn set_wtf_account_auto_sync_enabled(
    app: AppHandle,
    account: String,
    enabled: bool,
) -> Result<Settings, String> {
    let mut s = settings::load_settings(&app)?;
    s.wtf_account_sync
        .entry(account.clone())
        .or_default()
        .auto_sync_enabled = enabled;
    settings::save_settings(&app, &s)?;
    crate::debug_log::verbose(
        &app,
        "set_wtf_account_auto_sync_enabled",
        &format!("account {account} set to {enabled}"),
    );
    Ok(s)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WtfSyncInfo {
    pub status: crate::types::WtfSyncStatus,
    /// Whether the local account folder has changed since the last
    /// successful push or pull — drives the sync table's "you have
    /// unsynced local changes" hint, independent of any actual push/pull
    /// attempt (which does its own check server-side-aware of the remote too).
    pub local_changed: bool,
}

/// Every account this Discord user has a cloud backup for, regardless of
/// whether it exists locally on this device — see WtfCloudAccountEntry's
/// doc comment for why the sync table needs this alongside get_wtf_accounts.
#[tauri::command]
pub async fn get_wtf_cloud_accounts(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<crate::types::WtfCloudAccountEntry>, String> {
    let result = get_wtf_cloud_accounts_inner(&app, &state).await;
    crate::debug_log::log_on_err(&app, "get_wtf_cloud_accounts", result)
}

async fn get_wtf_cloud_accounts_inner(
    _app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<crate::types::WtfCloudAccountEntry>, String> {
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;
    crate::wtf_sync::fetch_wtf_cloud_accounts(&state.http, &token).await
}

/// Cloud status + local-change check for one account, for the sync table to
/// show "up to date" / "local changes pending" / "cloud has newer changes"
/// per row without actually pushing or pulling anything.
#[tauri::command]
pub async fn get_wtf_sync_info(
    app: AppHandle,
    state: State<'_, AppState>,
    account: String,
) -> Result<WtfSyncInfo, String> {
    let result = get_wtf_sync_info_inner(&app, &state, &account).await;
    crate::debug_log::log_on_err(&app, "get_wtf_sync_info", result)
}

async fn get_wtf_sync_info_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    account: &str,
) -> Result<WtfSyncInfo, String> {
    let s = settings::load_settings(app)?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    let status = crate::wtf_sync::fetch_wtf_status(&state.http, &token, account).await?;

    let known_fingerprint = s
        .wtf_account_sync
        .get(account)
        .and_then(|a| a.last_synced_fingerprint.clone());
    let local_changed = s
        .game_path()
        .and_then(|game_path| crate::wtf_sync::account_dir(&game_path, account).ok())
        .and_then(|dir| crate::wtf_sync::fingerprint_account(&dir).ok())
        .map(|fp| Some(fp) != known_fingerprint)
        .unwrap_or(false);

    Ok(WtfSyncInfo {
        status,
        local_changed,
    })
}

/// Zips `account`'s WTF/Account folder and uploads it, overwriting whatever
/// this player previously had stored for that account specifically. Refuses
/// (naming why) unless `force` is set if the cloud copy changed since this
/// device last touched it — see wtf_sync.rs's module doc for the conflict model.
#[tauri::command]
pub async fn wtf_push(
    app: AppHandle,
    state: State<'_, AppState>,
    account: String,
    force: bool,
) -> Result<Settings, String> {
    let result = wtf_push_inner(&app, &state, &account, force).await;
    crate::debug_log::log_on_err(&app, "wtf_push", result)
}

async fn wtf_push_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    account: &str,
    force: bool,
) -> Result<Settings, String> {
    let mut s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    if crate::wtf_sync::is_wow_running() {
        return Err(
            "close World of Warcraft before backing up or restoring your settings".to_string(),
        );
    }

    let dir = crate::wtf_sync::account_dir(&game_path, account)?;
    let existing = s.wtf_account_sync.get(account).cloned().unwrap_or_default();

    if !force {
        let remote = crate::wtf_sync::fetch_wtf_status(&state.http, &token, account).await?;
        if remote.exists && remote.updated_at != existing.last_known_remote_updated_at {
            return Err(
                "Your cloud backup has changed since this device last backed it up - backing up now will overwrite it."
                    .to_string(),
            );
        }
    }

    let zip = crate::wtf_sync::zip_account(&dir)?;
    let fingerprint = crate::wtf_sync::fingerprint_account(&dir)?;
    let status = crate::wtf_sync::upload_wtf_zip(&state.http, &token, account, zip).await?;

    s.wtf_account_sync.insert(
        account.to_string(),
        crate::types::WtfAccountSyncState {
            last_synced_fingerprint: Some(fingerprint),
            last_known_remote_updated_at: status.updated_at,
            last_synced_at: Some(crate::sync_log::now_ms()),
            // Carried forward, not reset - this insert replaces the whole
            // entry, and a successful sync must never silently re-enable
            // auto-sync for an account the player explicitly turned off.
            auto_sync_enabled: existing.auto_sync_enabled,
        },
    );
    settings::save_settings(app, &s)?;
    crate::debug_log::verbose(
        app,
        "wtf_push",
        &format!("pushed wtf account {account} to cloud (force={force})"),
    );
    Ok(s)
}

/// Downloads `account`'s cloud WTF backup and replaces that account
/// folder's local contents with it — creating the folder if this device
/// has never logged into that account before (see
/// wtf_sync::account_dir_for_pull). Refuses (naming why) unless `force` is
/// set if the local folder changed since this device last synced — see
/// wtf_sync.rs's module doc for the conflict model.
#[tauri::command]
pub async fn wtf_pull(
    app: AppHandle,
    state: State<'_, AppState>,
    account: String,
    force: bool,
) -> Result<Settings, String> {
    let result = wtf_pull_inner(&app, &state, &account, force).await;
    crate::debug_log::log_on_err(&app, "wtf_pull", result)
}

async fn wtf_pull_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    account: &str,
    force: bool,
) -> Result<Settings, String> {
    let mut s = settings::load_settings(app)?;
    let game_path = s
        .game_path()
        .ok_or_else(|| "no game folder configured".to_string())?;
    let token = crate::keychain::get_token()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "sign in with Discord first".to_string())?;

    if crate::wtf_sync::is_wow_running() {
        return Err(
            "close World of Warcraft before backing up or restoring your settings".to_string(),
        );
    }

    let dir = crate::wtf_sync::account_dir_for_pull(&game_path, account)?;
    let existing = s.wtf_account_sync.get(account).cloned().unwrap_or_default();

    if !force {
        let current_fp = crate::wtf_sync::fingerprint_account(&dir)?;
        let has_local_changes = match &existing.last_synced_fingerprint {
            Some(known) => &current_fp != known,
            None => crate::wtf_sync::has_any_synced_files(&dir)?,
        };
        if has_local_changes {
            let message = if existing.last_synced_fingerprint.is_none() {
                "This device has never restored this WTF folder before - restoring now will overwrite its current \
                 contents with the cloud copy."
            } else {
                "You have local changes that haven't been backed up yet - restoring now will overwrite them with \
                 the cloud copy."
            };
            return Err(message.to_string());
        }
    }

    let zip = crate::wtf_sync::download_wtf_zip(&state.http, &token, account).await?;
    crate::wtf_sync::extract_account_zip(&zip, &dir)?;

    let fingerprint = crate::wtf_sync::fingerprint_account(&dir)?;
    let status = crate::wtf_sync::fetch_wtf_status(&state.http, &token, account).await?;

    s.wtf_account_sync.insert(
        account.to_string(),
        crate::types::WtfAccountSyncState {
            last_synced_fingerprint: Some(fingerprint),
            last_known_remote_updated_at: status.updated_at,
            last_synced_at: Some(crate::sync_log::now_ms()),
            // Carried forward, not reset - see the matching comment in wtf_push_inner.
            auto_sync_enabled: existing.auto_sync_enabled,
        },
    );
    settings::save_settings(app, &s)?;
    crate::debug_log::verbose(
        app,
        "wtf_pull",
        &format!("pulled wtf account {account} from cloud (force={force})"),
    );
    Ok(s)
}

#[cfg(test)]
mod detect_game_client_tests {
    use super::*;

    /// A throwaway folder tree under the OS temp dir, removed on drop.
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sot-detect-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn file(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        fn path(&self, relative: &str) -> String {
            self.0.join(relative).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Battle.net layout: World of Warcraft/_forever_/{Wow.exe, .flavor.info}
    /// with "Product Flavor!STRING:0\nwowf" (measured 2026-11-09).
    fn forever_install(name: &str) -> TempTree {
        let tree = TempTree::new(name);
        tree.file("_forever_/Wow.exe", "");
        tree.file("_forever_/.flavor.info", "Product Flavor!STRING:0\nwowf\n");
        tree
    }

    #[test]
    fn the_forever_folder_is_detected_as_forever() {
        let tree = forever_install("forever");
        let found = detect_game_client(tree.path("_forever_")).unwrap();
        assert_eq!(found.client, GameClient::Forever);
        assert_eq!(found.folder, tree.path("_forever_"));
    }

    #[test]
    fn picking_the_parent_folder_still_finds_forever_and_points_at_its_folder() {
        let tree = forever_install("parent");
        let found = detect_game_client(tree.path("")).unwrap();
        assert_eq!(found.client, GameClient::Forever);
        // Addons must go into _forever_/Interface/AddOns, not the parent's.
        assert!(found.folder.ends_with("_forever_"));
    }

    #[test]
    fn a_folder_without_a_flavor_file_is_wotlk() {
        let tree = TempTree::new("wotlk");
        tree.file("Wow.exe", "");
        let found = detect_game_client(tree.path("")).unwrap();
        assert_eq!(found.client, GameClient::Wotlk);
    }

    #[test]
    fn a_folder_without_wow_exe_is_not_a_game_folder() {
        let tree = TempTree::new("empty");
        tree.file("readme.txt", "");
        assert!(detect_game_client(tree.path("")).is_none());
    }
}
