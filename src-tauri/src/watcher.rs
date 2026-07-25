use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

/// (Re)starts watching `<game_path>/WTF/Account` for changes to either
/// guild-sync addon's SavedVariables file, replacing (and thereby
/// stopping) any previous watch. Called once at startup and again
/// whenever the configured game folder changes.
///
/// Emits `guild-data-changed` instead of the frontend polling on a
/// timer — SoTGuildSync.lua/GuildExport.lua only ever change on real
/// WoW-side events (login, logout, gear/gold changes, guild roster
/// updates), not continuously, so there's nothing to gain from checking
/// on a fixed interval versus reacting the moment the file actually
/// changes.
pub fn watch_guild_data(app: &AppHandle, game_path: &Path) {
    let accounts_dir = game_path.join("WTF").join("Account");

    let state = app.state::<AppState>();
    {
        // Dropping any previous watcher here stops it (notify unwatches
        // everything and tears down its background thread on Drop)
        // before we start a new one.
        let mut guard = state.guild_watcher.lock().unwrap();
        *guard = None;
    }

    if !accounts_dir.exists() {
        return;
    }

    let (tx, rx) = channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("failed to create guild data watcher: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(&accounts_dir, RecursiveMode::Recursive) {
        log::warn!("failed to watch {}: {e}", accounts_dir.display());
        return;
    }

    state.guild_watcher.lock().unwrap().replace(watcher);

    let app = app.clone();
    std::thread::spawn(move || {
        // WoW (or the OS) can fire several write events for one logical
        // save (e.g. a write-then-rename, or several partial flushes
        // during the bigger write a full logout does) — this needs to be
        // a trailing debounce (wait for 500ms of silence, then emit once)
        // rather than a leading throttle (emit on the first event, then
        // ignore the rest for 500ms). A throttle would fire on the FIRST
        // event in a burst, which can catch the file mid-write, and then
        // silently drop the later event for the actually-completed write
        // — leaving the frontend stuck having read a truncated file with
        // no further notification to re-read it.
        let mut pending = false;

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    if !matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    ) {
                        continue;
                    }
                    let relevant = event.paths.iter().any(|p| {
                        matches!(
                            p.file_name().and_then(|n| n.to_str()),
                            Some("SoTGuildSync.lua") | Some("GuildExport.lua")
                        )
                    });
                    if relevant {
                        pending = true;
                    }
                }
                Ok(Err(_)) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending {
                        pending = false;
                        let _ = app.emit("guild-data-changed", ());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}
