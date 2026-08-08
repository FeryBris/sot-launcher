// Groups raw WoWCombatLog.txt lines into one whole-session, boundary-safe
// upload per real play session — the launcher's half of the redesign that
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
//
// No fixed-target size boundary anymore (an earlier version targeted ~30
// minutes per chunk, closing at the next quiet moment past that target -
// still in git history if ever needed again) - a real raid night's own
// pulls/trash gaps never get split mid-night now, only a genuine
// SESSION_GAP_MS-or-longer silence does (see its own doc comment for why
// that's a much bigger threshold than the old inter-pull one). This was a
// deliberate trade made once the backend side could support one whole
// raid night as a single chunk: CombatLogSyncController now hands the
// actual parse/segment/persist work to a queued job instead of running it
// inline in the request (see that controller's own doc comment), and
// RawLogParser was made meaningfully more memory-efficient (streamed line
// reads + string interning - see its own doc comment) after benchmarking
// confirmed a whole-raid-night-scale raw log (~190MB/1.3M lines) would
// otherwise peak well over a gigabyte just to parse. The real trade this
// makes: nothing is syncable mid-raid anymore (only once a real gap that
// size is seen), and set_combat_session_ignored's own per-session ignore
// granularity (see commands.rs, surfaced in Logs.vue) now applies to a
// whole night at a time, not ~30 minutes of it - accepted as worth it for
// one upload per raid night instead of ~10.
//
// The gap check still matters, not just check_idle's real-wall-clock one -
// confirmed by a real bug this rewrite introduced and then had to fix:
// reprocess_combat_log replays a WHOLE historical WoWCombatLog.txt (often
// spanning many real, separate past play sessions across different days)
// through record_line in one tight synchronous loop - essentially zero
// real wall-clock time elapses between the FIRST and LAST line of that
// replay, so check_idle alone would never fire DURING it, and the entire
// backlog would sit open - and unsyncable - as one giant unclosed chunk
// until 5 real minutes later. Using the log's OWN embedded timestamps
// (not real elapsed processing time) for the big-gap check means backlog
// replay closes chunks at the SAME real day-to-day boundaries a live
// watch would have closed them at, regardless of how fast the replay
// itself runs.

use crate::combat_log_parser::parse_line_timestamp;
use chrono::NaiveDate;
use std::time::Instant;

/// A gap this big between two consecutive lines' own LOG timestamps means
/// the player clearly stopped playing in between (logged off, closed WoW,
/// or this is simply a different real day entirely in a replayed backlog -
/// see this module's own doc comment) - not just a normal raid-night lull
/// between pulls, trash, or a bio break, which real production logs
/// confirmed can run several minutes without being a real session
/// boundary. Deliberately the SAME magnitude as CHUNK_IDLE_TIMEOUT (five
/// minutes) - they're the same real-world threshold measured on two
/// different clocks (log time here, since this must also work correctly
/// against a backlog replayed in a tight loop with no real time elapsing
/// between lines; wall-clock time in CHUNK_IDLE_TIMEOUT, for the tail
/// chunk of an actively-live-watched file that has no "next line" yet to
/// compare against) - keep them in sync if either ever changes.
pub const SESSION_GAP_MS: i64 = 5 * 60 * 1000;

/// Closes the current chunk once this much REAL wall-clock time has passed
/// with no new line at all - see SESSION_GAP_MS's own doc comment for why
/// this alone isn't enough (a replayed backlog needs the log-time gap
/// check too) - same last-processed-at wall-clock trick combat_log_watcher's
/// SessionTracker predecessor used (see that module's own doc comment for
/// why synthetic log timestamps can't be used for THIS specific check
/// instead: there's no "next line" yet for the current tail chunk of a
/// live-watched file, so only real elapsed time can tell us the player
/// stopped). Without this, a raid night's own chunk would sit open - and
/// unsyncable - forever once the player logs off, since nothing else ever
/// closes it.
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

fn should_close_for_gap(chunk: &ChunkBuilder, incoming_line_ms: i64) -> bool {
    incoming_line_ms - chunk.last_line_ms >= SESSION_GAP_MS
}

/// Groups every raw combat log line into one whole-session chunk per real
/// play session - see this module's own doc comment for why buffering is
/// unconditional and there's no fixed-target-size boundary anymore, only
/// a genuine SESSION_GAP_MS-or-longer gap (record_line) or real wall-clock
/// idle time (check_idle).
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
    /// if this line's own log timestamp shows a real SESSION_GAP_MS-or-
    /// longer silence since the chunk's last buffered line (see
    /// should_close_for_gap/SESSION_GAP_MS's own doc comments) - the
    /// closed chunk never includes this line; it starts the next one
    /// instead, same "check before recording" shape the old SessionTracker
    /// used for its own timeout check.
    pub fn record_line(
        &mut self,
        line: &str,
        reference: NaiveDate,
    ) -> Option<(FinishedChunk, String)> {
        self.last_processed_at = Some(Instant::now());
        let line_ms = parse_line_timestamp(line, reference);

        let mut finished = None;
        if let (Some(chunk), Some(t)) = (self.current.as_ref(), line_ms) {
            if should_close_for_gap(chunk, t) {
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
    /// (check_idle) - called periodically so the current TAIL chunk (the
    /// one still open because no NEXT line has arrived yet to trigger
    /// record_line's own gap check against - see SESSION_GAP_MS's own doc
    /// comment) closes promptly once the player's actually done for the
    /// session, rather than sitting open (and unsyncable) forever.
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
    fn a_whole_multi_hour_raid_night_with_only_normal_lulls_stays_one_chunk() {
        // The real behavior change this module was rewritten for: real
        // pulls/trash gaps (well under SESSION_GAP_MS, even a longer-than-
        // usual bio break) must NOT split a raid night into several
        // chunks the way the OLD ~30-minute target + 15s quiet gap used to
        // - only a genuine SESSION_GAP_MS-or-longer silence (or real
        // wall-clock idle, via check_idle) is allowed to close a chunk now.
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();

        let mut t_ms = 0i64;
        // 4 minutes apart - comfortably under SESSION_GAP_MS (5 min), well
        // past the OLD 15s quiet gap.
        let step_ms = 4 * 60 * 1000;
        let five_hours_ms = 5 * 60 * 60 * 1000;
        while t_ms < five_hours_ms {
            let secs = t_ms / 1000;
            let hms = format!(
                "{:02}:{:02}:{:02}.000",
                (14 + secs / 3600) % 24,
                (secs / 60) % 60,
                secs % 60
            );
            let line = line_at(
                &hms,
                r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,100,0,1,0,0,0,nil,nil,nil"#,
            );
            assert!(
                tracker.record_line(&line, reference).is_none(),
                "a normal raid-night lull under SESSION_GAP_MS must not close a chunk"
            );
            t_ms += step_ms;
        }

        let (finished, _raw) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("the whole 5-hour span should still be one open chunk, closeable by check_idle");
        assert_eq!(finished.line_count, (five_hours_ms / step_ms) as usize);
    }

    #[test]
    fn a_replayed_backlog_spanning_two_real_days_splits_on_the_logs_own_gap_not_wall_clock() {
        // The real regression this module hit and had to fix: reprocess_combat_log
        // (see combat_log_watcher.rs) replays a whole historical file through
        // record_line in one tight loop - real wall-clock time barely
        // elapses between the two lines below despite them being a full day
        // apart in LOG time, so check_idle alone would never split them.
        // The gap check in record_line must use the LOG's own timestamps for
        // exactly this reason.
        let mut tracker = ChunkTracker::new();
        let reference = today_reference();

        let day_one = line_at(
            "14:00:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,100,0,1,0,0,0,nil,nil,nil"#,
        );
        assert!(tracker.record_line(&day_one, reference).is_none());

        // 6 hours later in log time - stands in for "a real different play
        // session" (whether later the same day or, since parse_line_timestamp
        // only ever tracks time-of-day, a real different calendar day
        // entirely - either way the gap is unmistakably a session boundary,
        // well past SESSION_GAP_MS).
        let much_later = line_at(
            "20:00:00.000",
            r#"SWING_DAMAGE,0x0E1,"Alice",0x512,0xF1,"Boss",0xa48,50,0,1,0,0,0,nil,nil,nil"#,
        );
        let (finished, raw) = tracker
            .record_line(&much_later, reference)
            .expect("a SESSION_GAP_MS-or-longer log-time gap should close the chunk");
        assert_eq!(finished.line_count, 1);
        assert!(raw.contains("100,0,1"));
        assert!(
            !raw.contains("50,0,1"),
            "the closing line belongs to the NEXT chunk, not this one"
        );

        // The second chunk is still open, buffering the new line - confirm
        // it's there and correctly closeable, not silently dropped.
        let (second, raw2) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("the second chunk should still be open and closeable");
        assert_eq!(second.line_count, 1);
        assert!(raw2.contains("50,0,1"));
    }

    #[test]
    fn check_idle_closes_a_chunk_after_real_wall_clock_idle_time() {
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
