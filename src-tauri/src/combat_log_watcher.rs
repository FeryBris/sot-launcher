// Watches <game_path>/Logs/WoWCombatLog.txt, incrementally parsing/
// aggregating its lines into encounters via combat_log_parser and
// combat_log_session — mirrors watcher.rs's SavedVariables-watching
// pattern (notify + a background thread) plus wowlogs/incremental_reader.py's
// byte-offset resume-across-restarts approach. The very first time a given
// log path is watched (no checkpoint yet), whatever's already in the file
// is processed as historical backlog before live watching begins, not just
// lines that arrive from here on — see load_offset/watch_combat_log.
// Simplified from the wowlogs original in two ways:
//
// - No line-hash dedup set: unlike wowlogs (whose checkpoint carries a
//   hash of every line ever seen, growing forever, never pruned), seeking
//   to an exact byte offset before reading is already fully deterministic
//   — the same offset always reads the same next bytes, so there's nothing
//   a hash set catches that offset-tracking doesn't already guarantee.
// - Idle detection (SessionTracker::check_idle) uses real wall-clock time
//   rather than wowlogs' approach of comparing real "now" against the
//   log's own synthetic event timestamps — confirmed empirically that
//   Python's strptime defaults a missing year to 1900, so that comparison
//   sits roughly 126 *years* in the past. Its idle check reads as "timed
//   out" on essentially every poll, meaning (unless disproven by testing
//   against the actual running tool) its encounters likely never get the
//   chance to accumulate a whole fight before being cut off.

use crate::combat_log_session::{EncounterReport, SessionTracker};
use crate::commands::AppState;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// No gap in combat activity longer than this ends the current encounter.
pub const ENCOUNTER_TIMEOUT_MS: i64 = 15_000;
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// How many just-finished encounters the frontend can look back at without
/// having missed the emitted event (e.g. it wasn't mounted yet).
const MAX_RECENT_ENCOUNTERS: usize = 20;

/// (session_id, raw log text) pairs for recently finished encounters — kept
/// separate from combat_log_recent (see EncounterReport's own npc_names doc
/// comment for the related "keep raw facts out of the aggregated report"
/// reasoning) so the frontend's Logs page and the combat-encounter-completed
/// event never have to carry a potentially-multi-megabyte string just to
/// show a summary row. Only ever read by sync_encounter_to_web.
pub type RawLogStore = Arc<Mutex<Vec<(String, String)>>>;
/// Matches wowlogs' own MIN_COMBAT_DURATION_MS default — filters out a
/// stray duel or a single tagged-then-abandoned mob, not real combat.
const MIN_ENCOUNTER_DURATION_SEC: f64 = 5.0;

/// Deliberately does NOT also require content_type to be Dungeon/Raid the
/// way wowlogs' DUNGEON_RAID_ONLY=True default does — that heuristic
/// needs a real 4+ person group just to score positively at all (see
/// combat_log_zone::classify), so solo/small-group testing (or a 2-3
/// person world escort, a duel, ...) always reads as World and would be
/// silently discarded here before ever reaching the Logs view, with
/// nothing to explain why. content_type is still computed and shown on
/// each card - filtering what's worth syncing is left to the existing
/// per-session Ignore button instead of an unreliable auto-guess.
fn should_report(report: &EncounterReport) -> bool {
    report.duration_sec >= MIN_ENCOUNTER_DURATION_SEC
}

pub fn combat_log_path(game_path: &Path) -> PathBuf {
    game_path.join("Logs").join("WoWCombatLog.txt")
}

#[derive(Serialize, Deserialize, Default)]
struct Checkpoint {
    /// Which log file this offset belongs to — a changed game folder (or
    /// no checkpoint saved yet) means "start from the current end of file
    /// forward", never reuse a stale offset against a different file.
    path: String,
    offset: u64,
}

fn checkpoint_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("combat_log.chk"))
}

/// Where to resume reading from. Defaults to 0 (process the WHOLE current
/// log as historical backlog — see watch_combat_log) whenever there's no
/// usable checkpoint to resume from instead: the very first time this
/// file's ever watched, a corrupt/missing checkpoint, or the configured
/// game folder having changed to a different install's log. Only a valid,
/// matching, in-range checkpoint skips ahead of that.
fn load_offset(app: &AppHandle, log_path: &Path, current_len: u64) -> u64 {
    let Some(path) = checkpoint_path(app) else {
        return 0;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return 0;
    };
    let Ok(checkpoint) = serde_json::from_str::<Checkpoint>(&raw) else {
        return 0;
    };
    if checkpoint.path == log_path.to_string_lossy() && checkpoint.offset <= current_len {
        checkpoint.offset
    } else {
        0
    }
}

fn save_checkpoint(app: &AppHandle, log_path: &Path, offset: u64) {
    let Some(path) = checkpoint_path(app) else {
        return;
    };
    let checkpoint = Checkpoint {
        path: log_path.to_string_lossy().to_string(),
        offset,
    };
    if let Ok(raw) = serde_json::to_string(&checkpoint) {
        let _ = std::fs::write(path, raw);
    }
}

/// Reads whatever's new since `offset`, returning the lines read and the
/// offset to resume from next time. Detects truncation (file shrank below
/// the saved offset — WoW replaces this file on client restart) by
/// starting over from 0 rather than seeking past EOF.
fn read_new_lines(log_path: &Path, offset: u64) -> (Vec<String>, u64) {
    let Ok(file) = File::open(log_path) else {
        return (Vec::new(), offset);
    };
    let Ok(metadata) = file.metadata() else {
        return (Vec::new(), offset);
    };

    let start_offset = if metadata.len() < offset { 0 } else { offset };

    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(start_offset)).is_err() {
        return (Vec::new(), start_offset);
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut bytes_read: u64 = 0;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // caught up to EOF
            Ok(n) => {
                // A line with no trailing newline yet is still being
                // written — leave it for the next poll rather than
                // processing a partial line and advancing past it.
                if !line.ends_with('\n') {
                    break;
                }
                bytes_read += n as u64;
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }
            Err(_) => break,
        }
    }

    (lines, start_offset + bytes_read)
}

fn process_lines(
    app: &AppHandle,
    tracker: &Arc<Mutex<SessionTracker>>,
    recent: &Arc<Mutex<Vec<EncounterReport>>>,
    raw_logs: &RawLogStore,
    lines: Vec<String>,
) {
    let mut finished = Vec::new();
    {
        let mut tracker = tracker.lock().unwrap();
        for line in lines {
            if let Some(result) = tracker.record_line(&line) {
                finished.push(result);
            }
        }
    }

    for (report, raw_log) in finished {
        if should_report(&report) {
            store_and_emit(app, recent, raw_logs, report, raw_log);
        }
    }
}

fn store_and_emit(
    app: &AppHandle,
    recent: &Arc<Mutex<Vec<EncounterReport>>>,
    raw_logs: &RawLogStore,
    report: EncounterReport,
    raw_log: String,
) {
    {
        let mut recent = recent.lock().unwrap();
        recent.push(report.clone());
        if recent.len() > MAX_RECENT_ENCOUNTERS {
            let overflow = recent.len() - MAX_RECENT_ENCOUNTERS;
            recent.drain(0..overflow);
        }
    }
    {
        let mut raw_logs = raw_logs.lock().unwrap();
        raw_logs.push((report.session_id.clone(), raw_log));
        if raw_logs.len() > MAX_RECENT_ENCOUNTERS {
            let overflow = raw_logs.len() - MAX_RECENT_ENCOUNTERS;
            raw_logs.drain(0..overflow);
        }
    }
    let _ = app.emit("combat-encounter-completed", &report);

    maybe_auto_sync(app, report);
}

/// Immediately syncs a freshly completed encounter to the website if the
/// user has combat_log_auto_sync enabled, the same way WTF/roster auto-sync
/// upload right after capturing something new instead of waiting for the
/// user to open Logs and click Sync by hand. Fire-and-forget: a failure
/// here (not signed in, offline, ...) still lands in the sync log via
/// sync_encounter_to_web, same as a manual sync failing would, and the
/// session stays available to retry by hand from the Logs page.
fn maybe_auto_sync(app: &AppHandle, report: EncounterReport) {
    let Ok(settings) = crate::settings::load_settings(app) else {
        return;
    };
    if !settings.combat_log_auto_sync {
        return;
    }

    let http = app.state::<AppState>().http.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::commands::sync_encounter_to_web(&app, &http, &report).await;
    });
}

/// Combat logging is off by default (`/combatlog` or Interface Options >
/// Advanced Combat Logging) — Logs/WoWCombatLog.txt may not exist yet, for
/// however long it takes the player to notice/care. This is a one-time,
/// not latency-sensitive wait, so a relaxed poll for the file's existence
/// is fine; notify takes over for actual line-reading once it's there.
const LOG_FILE_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Deletes the saved checkpoint (if any) and restarts the watch — same
/// effect as watch_combat_log's own first-ever-run behavior (full
/// historical backlog reprocessed), but callable on demand instead of only
/// ever happening automatically the very first time a log path is watched.
/// Exists because a checkpoint saved by an older build (from before
/// backlog processing existed, pointing at end-of-file) or one left over
/// from before combat logging was even turned on in-game otherwise
/// silently blocks ever reprocessing history again.
pub fn reprocess_combat_log(app: &AppHandle, game_path: &Path) {
    if let Some(path) = checkpoint_path(app) {
        let _ = std::fs::remove_file(path);
    }
    watch_combat_log(app, game_path);
}

/// (Re)starts watching `<game_path>/Logs/WoWCombatLog.txt`, replacing (and
/// thereby stopping) any previous watch — same lifecycle as
/// watcher::watch_guild_data, called once at startup and again whenever the
/// configured game folder changes.
pub fn watch_combat_log(app: &AppHandle, game_path: &Path) {
    let log_path = combat_log_path(game_path);
    let state = app.state::<AppState>();

    {
        let mut guard = state.combat_log_watcher.lock().unwrap();
        *guard = None;
    }
    *state.combat_log_tracker.lock().unwrap() = SessionTracker::new(ENCOUNTER_TIMEOUT_MS);
    state.combat_log_recent.lock().unwrap().clear();
    state.combat_log_raw_logs.lock().unwrap().clear();

    // Claims this call as the current one - a still-polling thread from an
    // earlier call (waiting on the file to be created) checks this and
    // exits rather than stacking up alongside this new one.
    let epoch = state
        .combat_log_watch_epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    let epoch_counter = state.combat_log_watch_epoch.clone();

    let app = app.clone();
    let tracker = state.combat_log_tracker.clone();
    let recent = state.combat_log_recent.clone();
    let raw_logs = state.combat_log_raw_logs.clone();

    std::thread::spawn(move || {
        let superseded = || epoch_counter.load(std::sync::atomic::Ordering::SeqCst) != epoch;

        while !log_path.exists() {
            if superseded() {
                return;
            }
            std::thread::sleep(LOG_FILE_POLL_INTERVAL);
        }
        if superseded() {
            return;
        }

        let Ok(metadata) = std::fs::metadata(&log_path) else {
            return;
        };
        let start_offset = load_offset(&app, &log_path, metadata.len());

        // Catches up on whatever's already in the file before this watch
        // started (the very first time this log is watched, that's the
        // whole file - see load_offset) through the exact same
        // parse/aggregate pipeline live updates use. Encounter boundaries
        // still come out right even though these lines are all read in one
        // burst just now: SessionTracker splits them by the GAPS BETWEEN
        // THE LOG'S OWN TIMESTAMPS, not by how they arrived here.
        let (backlog_lines, offset) = read_new_lines(&log_path, start_offset);
        save_checkpoint(&app, &log_path, offset);
        if !backlog_lines.is_empty() {
            process_lines(&app, &tracker, &recent, &raw_logs, backlog_lines);
        }
        let offset = Arc::new(Mutex::new(offset));

        let Some(log_dir) = log_path.parent().map(Path::to_path_buf) else {
            return;
        };

        let (tx, rx) = channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("failed to create combat log watcher: {e}");
                return;
            }
        };
        // Watches the directory, not the file itself — a fresh WoW session
        // can replace WoWCombatLog.txt outright (not just append to it),
        // and a watch on the old file handle/inode wouldn't see that.
        if let Err(e) = watcher.watch(&log_dir, RecursiveMode::NonRecursive) {
            log::warn!("failed to watch {}: {e}", log_dir.display());
            return;
        }

        {
            let state = app.state::<AppState>();
            let mut guard = state.combat_log_watcher.lock().unwrap();
            // Superseded while setting this up - drop it rather than
            // clobbering whatever the newer call has already installed.
            if superseded() {
                return;
            }
            guard.replace(watcher);
        }

        loop {
            match rx.recv_timeout(IDLE_CHECK_INTERVAL) {
                Ok(Ok(event)) => {
                    if !matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    ) {
                        continue;
                    }
                    let relevant = event.paths.iter().any(|p| {
                        p.file_name().and_then(|n| n.to_str()) == Some("WoWCombatLog.txt")
                    });
                    if !relevant {
                        continue;
                    }

                    let mut current_offset = offset.lock().unwrap();
                    let (lines, new_offset) = read_new_lines(&log_path, *current_offset);
                    *current_offset = new_offset;
                    drop(current_offset);

                    save_checkpoint(&app, &log_path, new_offset);
                    if !lines.is_empty() {
                        process_lines(&app, &tracker, &recent, &raw_logs, lines);
                    }
                }
                Ok(Err(_)) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let result = tracker
                        .lock()
                        .unwrap()
                        .check_idle(Duration::from_millis(ENCOUNTER_TIMEOUT_MS as u64));
                    if let Some((report, raw_log)) = result {
                        if should_report(&report) {
                            store_and_emit(&app, &recent, &raw_logs, report, raw_log);
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sotgl-combatlog-test-{name}-{}.txt",
            std::process::id()
        ))
    }

    #[test]
    fn reading_from_offset_zero_returns_the_whole_backlog() {
        let path = temp_log_path("backlog");
        std::fs::write(&path, "line one\nline two\nline three\n").unwrap();

        let (lines, offset) = read_new_lines(&path, 0);
        assert_eq!(lines, vec!["line one", "line two", "line three"]);
        assert_eq!(offset, std::fs::metadata(&path).unwrap().len());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn resuming_from_a_saved_offset_only_returns_lines_written_after_it() {
        let path = temp_log_path("resume");
        std::fs::write(&path, "already seen\n").unwrap();
        let (_, offset_after_first_read) = read_new_lines(&path, 0);

        std::fs::write(&path, "already seen\nbrand new\n").unwrap();
        let (lines, _) = read_new_lines(&path, offset_after_first_read);
        assert_eq!(lines, vec!["brand new"]);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_line_with_no_trailing_newline_yet_is_left_for_the_next_read() {
        let path = temp_log_path("partial");
        std::fs::write(&path, "complete line\npartial line still being written").unwrap();

        let (lines, offset) = read_new_lines(&path, 0);
        assert_eq!(lines, vec!["complete line"]);
        // Offset stops right after "complete line\n", not at EOF - the
        // partial line gets read again (now complete) on the next poll.
        assert_eq!(offset, "complete line\n".len() as u64);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_shrunk_file_is_treated_as_truncated_and_read_from_the_start() {
        let path = temp_log_path("truncated");
        std::fs::write(&path, "short\n").unwrap();

        // Simulates a checkpoint from before WoW reset the log on restart -
        // the saved offset is now past this smaller file's actual length.
        let stale_offset = 10_000u64;
        let (lines, _) = read_new_lines(&path, stale_offset);
        assert_eq!(lines, vec!["short"]);

        std::fs::remove_file(&path).ok();
    }
}
