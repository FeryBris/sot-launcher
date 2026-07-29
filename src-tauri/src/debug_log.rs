//! Persistent error/warning log, written to debug.json in the app config
//! dir - unlike tauri_plugin_log (only active in debug builds, see
//! lib.rs), this runs in every build, so a shipped release actually has
//! something to inspect when "it's not working" and there's no dev
//! console to open. Mirrors sync_log.rs's own JSON-file-of-entries shape.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Oldest entries are dropped past this so the file never grows unbounded
/// over the launcher's lifetime.
const MAX_ENTRIES: usize = 500;

/// Entries older than this are dropped too, regardless of count - same
/// reasoning as sync_log.rs's own cutoff: this is a "what just went wrong"
/// aid, not a permanent history.
const MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugLogEntry {
    /// Unix epoch milliseconds.
    pub at: i64,
    /// "error" | "warn" | "info"
    pub level: String,
    /// Which command/operation this came from, e.g. "wtf_push",
    /// "sync_guild_data_to_web" - not a user-facing label, just enough to
    /// find the right code path when reading the file back.
    pub context: String,
    pub message: String,
}

fn debug_log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("debug.json"))
}

/// Newest first, with anything older than MAX_AGE_MS already dropped. An
/// unreadable/missing/corrupt file is treated as empty rather than an
/// error - this is a diagnostic aid, not something that should ever block
/// startup or a command.
pub fn load_debug_log(app: &AppHandle) -> Result<Vec<DebugLogEntry>, String> {
    let path = debug_log_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let entries: Vec<DebugLogEntry> = serde_json::from_str(&raw).unwrap_or_default();
    let cutoff = crate::sync_log::now_ms() - MAX_AGE_MS;
    Ok(entries.into_iter().filter(|e| e.at >= cutoff).collect())
}

/// Errors reading/writing the log itself are swallowed on purpose - a
/// logging failure must never surface as a command failure to the caller.
fn append(app: &AppHandle, level: &str, context: &str, message: &str) {
    let Ok(path) = debug_log_path(app) else { return };
    let mut entries = load_debug_log(app).unwrap_or_default();
    entries.insert(
        0,
        DebugLogEntry {
            at: crate::sync_log::now_ms(),
            level: level.to_string(),
            context: context.to_string(),
            message: message.to_string(),
        },
    );
    entries.truncate(MAX_ENTRIES);
    if let Ok(raw) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(&path, raw);
    }
}

pub fn error(app: &AppHandle, context: &str, message: &str) {
    log::error!("[{context}] {message}");
    append(app, "error", context, message);
}

pub fn warn(app: &AppHandle, context: &str, message: &str) {
    log::warn!("[{context}] {message}");
    append(app, "warn", context, message);
}

pub fn info(app: &AppHandle, context: &str, message: &str) {
    log::info!("[{context}] {message}");
    append(app, "info", context, message);
}

/// Logs `result`'s Err (if any) under `context` before returning it
/// unchanged - lets a command log its own outward-facing failure in one
/// place rather than duplicating a log call at every internal `?` that
/// could produce it.
pub fn log_on_err<T>(app: &AppHandle, context: &str, result: Result<T, String>) -> Result<T, String> {
    if let Err(e) = &result {
        error(app, context, e);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_newest_first() {
        let e1 = DebugLogEntry { at: 1, level: "error".into(), context: "a".into(), message: "first".into() };
        let e2 = DebugLogEntry { at: 2, level: "warn".into(), context: "b".into(), message: "second".into() };
        let mut entries = vec![e1];
        entries.insert(0, e2);
        assert_eq!(entries[0].context, "b");
        assert_eq!(entries[1].context, "a");
    }

    #[test]
    fn truncates_to_max_entries() {
        let mut entries: Vec<DebugLogEntry> = (0..10)
            .map(|i| DebugLogEntry { at: i, level: "info".into(), context: "x".into(), message: "m".into() })
            .collect();
        entries.truncate(5);
        assert_eq!(entries.len(), 5);
    }
}
