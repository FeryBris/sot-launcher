use crate::types::SyncLogEntry;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// Oldest entries are dropped past this so the log file never grows
/// unbounded over the launcher's lifetime — plenty to cover "why didn't
/// last night's raid sync" without needing rotation.
const MAX_ENTRIES: usize = 200;

fn sync_log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("sync_log.json"))
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Newest first. An unreadable/missing/corrupt file is treated as empty
/// rather than an error — this is a diagnostic aid, not something that
/// should ever block startup or a sync attempt.
pub fn load_sync_log(app: &AppHandle) -> Result<Vec<SyncLogEntry>, String> {
    let path = sync_log_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

/// Records one sync attempt, newest first, capped at MAX_ENTRIES. Errors
/// reading/writing the log are swallowed on purpose — a logging failure
/// must never surface as a sync failure to the caller.
pub fn append_sync_log(app: &AppHandle, entry: SyncLogEntry) {
    let Ok(path) = sync_log_path(app) else { return };
    let mut entries = load_sync_log(app).unwrap_or_default();
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
    if let Ok(raw) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(&path, raw);
    }
}
