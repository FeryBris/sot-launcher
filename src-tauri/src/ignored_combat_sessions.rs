//! Combat log session IDs the user has explicitly chosen to hide from the
//! Logs view (e.g. a wipe, or a pull they don't want on the website) — a
//! permanent decision, unlike sync_log's rolling diagnostic entries, so
//! this is its own small file. Mirrors ignored_raids.rs exactly; kept
//! separate rather than made generic since the two have no reason to ever
//! share a call site.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn ignored_sessions_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("ignored_combat_sessions.json"))
}

/// An unreadable/missing/corrupt file is treated as empty rather than an
/// error — same reasoning as ignored_raids: this is local bookkeeping,
/// never something that should block the Logs view from loading.
pub fn load_ignored_sessions(app: &AppHandle) -> Result<HashSet<String>, String> {
    let path = ignored_sessions_path(app)?;
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_ignored_sessions(app: &AppHandle, ids: &HashSet<String>) -> Result<(), String> {
    let path = ignored_sessions_path(app)?;
    let raw = serde_json::to_string_pretty(ids).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}

/// Adds or removes a single session ID from the ignored set.
pub fn set_session_ignored(app: &AppHandle, session_id: &str, ignored: bool) -> Result<(), String> {
    let mut ids = load_ignored_sessions(app)?;
    if ignored {
        ids.insert(session_id.to_string());
    } else {
        ids.remove(session_id);
    }
    save_ignored_sessions(app, &ids)
}
