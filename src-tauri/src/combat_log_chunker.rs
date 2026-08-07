// Groups raw WoWCombatLog.txt lines into fixed-target-size, boundary-safe
// "sessions" for manual upload — the launcher's half of the redesign that
// replaced per-encounter auto-sync (see combat_log_watcher.rs's own module
// doc comment for the rest of the story). Deliberately much simpler than
// the SessionTracker/CombatSession/PlayerStats machinery this replaced:
// no event parsing into damage/heal/death, no player aggregation, no role
// or content-type inference. None of that is needed client-side anymore —
// the backend's own PullSegmenter/CombatBreakdownService now recompute all
// of it server-side from the raw chunk text once uploaded (see
// CombatLogImport::importPulls), which is real work only worth doing once,
// not duplicated here just to decide where a chunk boundary goes.
//
// Every raw line is buffered UNCONDITIONALLY, not just while a recognized
// boss pull is "open" the way the old SessionTracker gated its own raw-line
// buffering — this is the actual fix for the real bug that started this
// whole redesign: a hunter/warlock's pet is very often summoned (SPELL_SUMMON)
// well before any boss pull begins, and SessionTracker's own "no session
// open yet" gap silently dropped that summon line, permanently losing the
// only evidence of who owns that pet.

use crate::combat_log_parser::parse_line_timestamp;
use chrono::NaiveDate;
use std::time::Instant;

/// Target span of one uploadable session - not a hard cutoff (see
/// should_close_for_size below), just how far a chunk tries to grow before
/// looking for a quiet moment to actually close at.
pub const CHUNK_TARGET_MS: i64 = 30 * 60 * 1000;

/// A chunk only ever closes for SIZE once at least this long has passed
/// since the last buffered line - same reasoning as the backend's own
/// PullSegmenter::QUIET_GAP_SEC (see that class' doc comment): closing
/// mid-fight would split one real pull's raw events across two uploads,
/// each showing PullSegmenter an INCOMPLETE view (an already-open pull with
/// no death/wipe in one chunk, and a pull that looks like it started
/// mid-fight - no starting damage/miss at all - in the next), silently
/// under-reporting that pull's own stats once the two submissions merge
/// (CombatLogImport merges by taking the MAX of each stat across
/// submissions, not by stitching two partial raw logs into one longer one -
/// two genuinely partial views of the same pull would each report less
/// than the real total, and MAX of two under-counts is still an
/// under-count).
pub const CHUNK_QUIET_GAP_MS: i64 = 15_000;

/// Closes the current chunk regardless of its size once this much REAL
/// wall-clock time has passed with no new line at all - same
/// last-processed-at wall-clock trick combat_log_watcher's SessionTracker
/// predecessor used (see that module's own doc comment for why synthetic
/// log timestamps can't be used for this instead). Makes sure a raid night
/// that ends with less than 30 minutes of a chunk open still becomes
/// syncable, instead of sitting open - and unsyncable - until WoW happens
/// to write another line.
pub const CHUNK_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// One completed chunk's own metadata - the raw text itself travels
/// alongside (not inside) this, same "keep potentially-multi-megabyte text
/// out of what the frontend/event payload carries" reasoning as the old
/// EncounterReport/RawLogStore split (see combat_log_watcher::RawLogStore).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedChunk {
    pub session_id: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub line_count: usize,
}

struct ChunkBuilder {
    session_id: String,
    start_ms: i64,
    last_line_ms: i64,
    lines: Vec<String>,
}

impl ChunkBuilder {
    fn new(start_ms: i64) -> Self {
        Self {
            // Deterministic from the chunk's own start timestamp, not an
            // in-memory counter that resets every launcher restart - a
            // "sync this"/"ignore this" decision keyed by session_id needs
            // the SAME chunk to keep the same ID if this ever gets
            // reprocessed (e.g. an app restart mid-chunk, or
            // reprocess_combat_log rebuilding history from scratch).
            session_id: format!("chunk_{start_ms}"),
            start_ms,
            last_line_ms: start_ms,
            lines: Vec::new(),
        }
    }

    fn push(&mut self, line: &str, line_ms: Option<i64>) {
        self.lines.push(line.to_string());
        // A line with no parseable timestamp of its own (essentially never
        // happens for a real WoW combat log line - every one starts with
        // "M/D HH:MM:SS.mmm  ", even the ones parse_line's full event
        // parsing doesn't recognize) doesn't move last_line_ms forward -
        // it shouldn't count as "activity" for quiet-gap purposes.
        if let Some(t) = line_ms {
            self.last_line_ms = t;
        }
    }

    fn finalize(self) -> (FinishedChunk, String) {
        (
            FinishedChunk {
                session_id: self.session_id,
                start_time_ms: self.start_ms,
                end_time_ms: self.last_line_ms,
                line_count: self.lines.len(),
            },
            self.lines.join("\n"),
        )
    }
}

fn should_close_for_size(chunk: &ChunkBuilder, incoming_line_ms: i64) -> bool {
    incoming_line_ms - chunk.start_ms >= CHUNK_TARGET_MS
        && incoming_line_ms - chunk.last_line_ms >= CHUNK_QUIET_GAP_MS
}

/// Groups every raw combat log line into chunks - see this module's own
/// doc comment for why buffering is unconditional and boundaries are
/// quiet-gap-gated rather than a hard 30-minute cutoff.
pub struct ChunkTracker {
    current: Option<ChunkBuilder>,
    last_processed_at: Option<Instant>,
}

impl ChunkTracker {
    pub fn new() -> Self {
        Self {
            current: None,
            last_processed_at: None,
        }
    }

    /// Buffers one raw line, closing and returning the CURRENT chunk first
    /// if this line's own timestamp lands at or past both the target size
    /// and a quiet moment relative to the chunk's own last buffered line
    /// (see should_close_for_size) - the closed chunk never includes this
    /// line; it starts the next one instead, same "check before recording"
    /// shape the old SessionTracker used for its own timeout check.
    pub fn record_line(
        &mut self,
        line: &str,
        reference: NaiveDate,
    ) -> Option<(FinishedChunk, String)> {
        self.last_processed_at = Some(Instant::now());
        let line_ms = parse_line_timestamp(line, reference);

        let mut finished = None;
        if let (Some(chunk), Some(t)) = (self.current.as_ref(), line_ms) {
            if should_close_for_size(chunk, t) {
                finished = self.current.take().map(ChunkBuilder::finalize);
            }
        }

        // A line with no parseable timestamp starting a brand new chunk
        // (only possible if the very first line this tracker ever sees is
        // malformed - vanishingly rare) falls back to real wall-clock now,
        // rather than silently dropping it the way "before any session is
        // open" used to for an unrecognized event type.
        let chunk = self.current.get_or_insert_with(|| {
            ChunkBuilder::new(line_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis()))
        });
        chunk.push(line, line_ms);

        finished
    }

    /// Same real-wall-clock idle detection the old SessionTracker used
    /// (check_idle) - called periodically so a chunk under the 30-minute
    /// target still closes promptly once the player's actually done for
    /// the session, rather than sitting open (and unsyncable) until more
    /// lines eventually arrive.
    pub fn check_idle(&mut self, timeout: std::time::Duration) -> Option<(FinishedChunk, String)> {
        let idle = self
            .last_processed_at
            .is_some_and(|t| t.elapsed() > timeout);
        if !idle {
            return None;
        }
        self.current.take().map(ChunkBuilder::finalize)
    }
}

impl Default for ChunkTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_log_parser::today_reference;

    fn line_at(hms: &str, suffix: &str) -> String {
        format!("2/28 {hms}  {suffix}")
    }

    #[test]
    fn buffers_a_line_unconditionally_even_before_any_recognized_combat_event() {
        // SPELL_SUMMON on its own used to be dropped entirely by the old
        // SessionTracker if no encounter had opened yet (see this module's
        // own doc comment) - the whole point of this rewrite is that this
        // no longer happens.
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();
        let summon = line_at(
            "14:00:00.000",
            r#"SPELL_SUMMON,0x0E1,"Alice",0x512,0xF9,"Wolf",0x1112,883,"Call Pet",0x8"#,
        );
        assert!(tracker.record_line(&summon, reference).is_none());

        let (_chunk, raw) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a chunk to close");
        assert!(raw.contains("SPELL_SUMMON"));
    }

    #[test]
    fn does_not_close_mid_pull_even_past_the_target_size() {
        // Continuous combat (no gap ever exceeding CHUNK_QUIET_GAP_MS)
        // spanning well past the 30-minute target must NOT get cut - see
        // CHUNK_QUIET_GAP_MS's own doc comment for why a mid-pull cut is a
        // real correctness bug, not just a cosmetic one.
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();

        let mut t_ms = 0i64;
        let step_ms = 10_000; // 10s apart - well under the 15s quiet gap
        while t_ms < CHUNK_TARGET_MS + 5 * 60 * 1000 {
            let secs = t_ms / 1000;
            let hms = format!(
                "{:02}:{:02}:{:02}.000",
                14 + secs / 3600,
                (secs / 60) % 60,
                secs % 60
            );
            let line = line_at(
                &hms,
                r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,100,0,1,0,0,0,nil,nil,nil"#,
            );
            assert!(
                tracker.record_line(&line, reference).is_none(),
                "must not close mid-pull despite exceeding the target size"
            );
            t_ms += step_ms;
        }
    }

    #[test]
    fn closes_once_both_the_size_target_and_a_quiet_moment_are_reached() {
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();

        let first = line_at(
            "14:00:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,100,0,1,0,0,0,nil,nil,nil"#,
        );
        assert!(tracker.record_line(&first, reference).is_none());

        // 31 minutes later, with a real quiet gap since the last line -
        // this line itself starts the NEXT chunk, closing the first.
        let after_target = line_at(
            "14:31:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,50,0,1,0,0,0,nil,nil,nil"#,
        );
        let (finished, raw) = tracker
            .record_line(&after_target, reference)
            .expect("size target + quiet gap should close the chunk");
        assert_eq!(finished.line_count, 1);
        assert!(raw.contains("100,0,1"));
        assert!(
            !raw.contains("50,0,1"),
            "the closing line belongs to the NEXT chunk, not this one"
        );
    }

    #[test]
    fn check_idle_closes_a_chunk_under_the_target_after_real_wall_clock_idle_time() {
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();
        let line = line_at(
            "14:00:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,100,0,1,0,0,0,nil,nil,nil"#,
        );
        tracker.record_line(&line, reference);

        assert!(tracker
            .check_idle(std::time::Duration::from_millis(500))
            .is_none());
        std::thread::sleep(std::time::Duration::from_millis(50));
        let (finished, _raw) = tracker
            .check_idle(std::time::Duration::from_millis(10))
            .expect("should have gone idle by now");
        assert_eq!(finished.line_count, 1);
    }

    #[test]
    fn session_id_is_deterministic_from_the_chunk_start_timestamp() {
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();
        let line = line_at(
            "14:00:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,1,0,1,0,0,0,nil,nil,nil"#,
        );
        tracker.record_line(&line, reference);
        let (finished, _) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .unwrap();
        assert_eq!(
            finished.session_id,
            format!("chunk_{}", finished.start_time_ms)
        );
    }
}
