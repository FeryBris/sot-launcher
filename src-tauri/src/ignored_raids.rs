use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Raid IDs the user has explicitly chosen to hide from the Raids view
/// (e.g. a pug/alt run HeadCount happened to finalize) — a permanent
/// decision, unlike sync_log's rolling diagnostic entries, so this is its
/// own small file rather than reusing that (capped, evictable) log.
fn ignored_raids_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("ignored_raids.json"))
}

/// An unreadable/missing/corrupt file is treated as empty rather than an
/// error — same reasoning as sync_log: this is local bookkeeping, never
/// something that should block the Raids view from loading.
pub fn load_ignored_raids(app: &AppHandle) -> Result<HashSet<String>, String> {
    let path = ignored_raids_path(app)?;
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_ignored_raids(app: &AppHandle, ids: &HashSet<String>) -> Result<(), String> {
    let path = ignored_raids_path(app)?;
    let raw = serde_json::to_string_pretty(ids).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}

/// Adds or removes a single raid ID from the ignored set.
pub fn set_raid_ignored(app: &AppHandle, raid_id: &str, ignored: bool) -> Result<(), String> {
    let mut ids = load_ignored_raids(app)?;
    if ignored {
        ids.insert(raid_id.to_string());
    } else {
        ids.remove(raid_id);
    }
    save_ignored_raids(app, &ids)
}
