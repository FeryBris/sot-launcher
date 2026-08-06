// Encounter detection, per-player DPS/HPS aggregation, and role
// inference — ported from wowlogs/combat_detector.py, aggregator.py, and
// roles.py. Unlike the Python original (which aggregates every GUID it
// sees, NPCs included, into "player" stats), this only tracks real player
// GUIDs (see combat_log_parser::is_player_flags) and keeps each one's name
// alongside their stats — the Python version's finalized payload only ever
// had GUIDs to key by, with no name anywhere in the output.

use crate::combat_log_parser::{
    is_npc_flags, is_pet_or_guardian_flags, is_player_flags, npc_entry_id, CombatEvent, CombatEventKind,
};
use crate::combat_log_zone::{self, ContentType};
use chrono::NaiveDate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Tank,
    Healer,
    Dps,
}

/// One player's finalized stats for a completed encounter.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerReport {
    pub guid: String,
    pub name: String,
    pub role: Role,
    pub damage_done: i64,
    pub healing_done: i64,
    /// Of healing_done, how much landed on someone OTHER than the caster -
    /// see PlayerStats::finalize's own doc comment for why role inference
    /// uses this instead of healing_done itself. Sent to the backend
    /// alongside healing_done (rather than folded into it) so
    /// CombatLogImport::mergeCharacters can apply the same exclusion after
    /// max-merging multiple submissions of the same encounter.
    pub healing_done_to_others: i64,
    pub damage_taken: i64,
    pub deaths: i32,
    pub active_time_sec: f64,
    pub dps: f64,
    pub hps: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncounterReport {
    pub session_id: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub duration_sec: f64,
    pub content_type: ContentType,
    pub content_quality_score: f64,
    pub event_count: usize,
    /// Every distinct NPC name that took player damage during this
    /// encounter, most-damaged first — a boss pull's adds show up here
    /// too, not just the boss itself. Deliberately just the raw name(s)
    /// straight off the log: matching a name to an actual Boss/Instance
    /// row (or not matching, for content that doesn't have one) belongs on
    /// the backend, which already owns that reference data.
    pub npc_names: Vec<String>,
    /// Same NPC names as npc_names, mapped to the creature template
    /// ("entry") ID embedded in their GUID where extractable (see
    /// combat_log_parser::npc_entry_id) — a more reliable join key than
    /// name for the backend to match against a Boss row, since several
    /// distinct NPCs can share a display name. Not every name necessarily
    /// has an entry here (e.g. its GUID didn't match the expected layout).
    pub npc_entry_ids: std::collections::HashMap<String, i64>,
    pub players: Vec<PlayerReport>,
}

#[derive(Default)]
struct PlayerStats {
    name: String,
    damage_done: i64,
    healing_done: i64,
    /// See PlayerReport::healing_done_to_others' own doc comment.
    healing_done_to_others: i64,
    damage_taken: i64,
    deaths: i32,
    first_action_ms: Option<i64>,
    last_action_ms: Option<i64>,
}

impl PlayerStats {
    fn touch(&mut self, t: i64, name: &str) {
        if self.name.is_empty() {
            self.name = name.to_string();
        }
        if self.first_action_ms.is_none() {
            self.first_action_ms = Some(t);
        }
        self.last_action_ms = Some(t);
    }

    /// `avg_damage_taken` is the WHOLE ROSTER's mean damage_taken for this
    /// encounter (computed once by CombatSession::finalize before calling
    /// this per player) — see the role thresholds below for why a single
    /// player's own stats alone aren't enough to infer tank/healer/dps.
    fn finalize(self, guid: String, avg_damage_taken: f64) -> Option<PlayerReport> {
        let (first, last) = (self.first_action_ms?, self.last_action_ms?);
        // At least 1 second, matching wowlogs' own `max(1, ...)` — avoids a
        // division blowup for a player whose only action landed in the same
        // millisecond the encounter ended.
        let active_time_sec = ((last - first) as f64 / 1000.0).max(1.0);
        // Two thresholds, confirmed against real logs where two actual
        // tanks came back "dps" and "healer":
        //
        // - Healer uses healing_done_TO_OTHERS, not raw healing_done. A
        //   self-healing tank (Blood DK's Death Strike/Rune Tap, a Prot
        //   Paladin's Seal/Judgement procs) can rack up healing_done that
        //   rivals a real healer's, but it's all self-targeted — excluding
        //   it is what tells the two apart.
        // - Tank fires on EITHER of two signals: the original
        //   `damage_taken > own damage_done * 2` ratio (kept as-is — still
        //   the only signal available for a solo/duo session, where
        //   "relative to the roster" is meaningless since the roster IS
        //   just them), OR damage_taken clearing 1.5x the ENCOUNTER
        //   ROSTER'S average — needed because a tank who also deals heavy
        //   threat-generation damage (common for WotLK-geared tanks) can
        //   easily out-damage what they take, which made the original
        //   ratio alone nearly unreachable in a real multi-player pull. A
        //   real tank still stands out clearly against what everyone else
        //   in the same pull took.
        let role = if self.healing_done_to_others as f64 > self.damage_done as f64 * 0.5 {
            Role::Healer
        } else if self.damage_taken as f64 > self.damage_done as f64 * 2.0
            || self.damage_taken as f64 > avg_damage_taken * 1.5
        {
            Role::Tank
        } else {
            Role::Dps
        };

        Some(PlayerReport {
            guid,
            name: self.name,
            role,
            damage_done: self.damage_done,
            healing_done: self.healing_done,
            healing_done_to_others: self.healing_done_to_others,
            damage_taken: self.damage_taken,
            deaths: self.deaths,
            active_time_sec: (active_time_sec * 100.0).round() / 100.0,
            dps: ((self.damage_done as f64 / active_time_sec) * 100.0).round() / 100.0,
            hps: ((self.healing_done as f64 / active_time_sec) * 100.0).round() / 100.0,
        })
    }
}

/// One in-progress or just-finalized combat session — a run of events with
/// no gap between them longer than the tracker's configured timeout.
pub struct CombatSession {
    session_id: String,
    start_ms: i64,
    last_event_ms: i64,
    /// Total events recorded (parsed damage/heal/death lines only — see
    /// record_line's own doc comment for why unparsed lines don't count),
    /// tracked as a running count rather than keeping every CombatEvent
    /// around just to re-derive this at finalize() time.
    event_count: usize,
    /// Of event_count, how many were player damage landing on an NPC —
    /// combat_log_zone::classify's own "sustained, structured combat"
    /// signal. Same reasoning as event_count: tracked live instead of
    /// replaying a buffered event list.
    npc_hit_count: usize,
    players: std::collections::HashMap<String, PlayerStats>,
    /// Total player damage dealt to each distinct NPC name seen — keyed by
    /// name, not GUID, since several individual mobs (adds, trash packs)
    /// commonly share one name and that's the granularity a backend Boss
    /// row would match against anyway.
    npc_damage: std::collections::HashMap<String, i64>,
    /// First-seen creature entry ID per NPC name (see
    /// combat_log_parser::npc_entry_id) — a supplementary, more reliable
    /// join key alongside npc_damage's name keys.
    npc_entry_ids: std::collections::HashMap<String, i64>,
    /// Pet/totem/guardian GUID -> (owner guid, owner name), learned from
    /// SPELL_SUMMON lines as this session sees them — see
    /// CombatEventKind::Summon and credited_source's own doc comments for
    /// why this exists at all (a pet's own flags are neither
    /// is_player_flags nor is_npc_flags, so without this its damage/
    /// healing wouldn't count toward anyone).
    pet_owners: std::collections::HashMap<String, (String, String)>,
    /// Every raw log line seen while this session was open, unparsed lines
    /// (aura applied/removed, casts, misses, ...) included — this is the
    /// "everything that happened" record a WCL-style backend breakdown
    /// would need, which the aggregated PlayerStats above deliberately
    /// don't keep. Uploaded alongside (not as part of) EncounterReport —
    /// see SessionTracker::record_line and combat_log_watcher's
    /// combat_log_raw_logs — so the frontend's Logs page and the
    /// combat-encounter-completed event don't have to carry a
    /// potentially-multi-megabyte string just to show a summary row.
    raw_lines: Vec<String>,
}

impl CombatSession {
    fn new(session_id: String, start_ms: i64) -> Self {
        Self {
            session_id,
            start_ms,
            last_event_ms: start_ms,
            event_count: 0,
            npc_hit_count: 0,
            players: std::collections::HashMap::new(),
            npc_damage: std::collections::HashMap::new(),
            npc_entry_ids: std::collections::HashMap::new(),
            pet_owners: std::collections::HashMap::new(),
            raw_lines: Vec::new(),
        }
    }

    /// Who a damage/heal event's SOURCE should be credited to, or None if
    /// it shouldn't count toward any player at all (an NPC's own action,
    /// or a pet/guardian whose owner this session never learned about from
    /// a SPELL_SUMMON line - graceful degradation, not a fake "pet as
    /// player" entry). A pet/guardian with a KNOWN owner resolves to the
    /// OWNER's guid/name instead of the pet's own, so e.g. a hunter pet's
    /// damage credits the hunter rather than silently not counting toward
    /// anyone (a pet's own flags are neither is_player_flags nor
    /// is_npc_flags).
    fn credited_source(&self, guid: &str, name: &str, flags: i64) -> Option<(String, String)> {
        if is_player_flags(flags) {
            return Some((guid.to_string(), name.to_string()));
        }
        if is_pet_or_guardian_flags(flags) {
            return self.pet_owners.get(guid).cloned();
        }
        None
    }

    fn record(&mut self, event: CombatEvent, raw_line: &str) {
        self.raw_lines.push(raw_line.to_string());
        self.last_event_ms = event.timestamp_ms;
        self.event_count += 1;

        match event.kind {
            CombatEventKind::Damage => {
                if let Some((guid, name)) =
                    self.credited_source(&event.source_guid, &event.source_name, event.source_flags)
                {
                    let p = self.players.entry(guid).or_default();
                    p.damage_done += event.amount;
                    p.touch(event.timestamp_ms, &name);
                }
                if is_player_flags(event.dest_flags) {
                    let p = self.players.entry(event.dest_guid.clone()).or_default();
                    p.damage_taken += event.amount;
                    if p.name.is_empty() {
                        p.name = event.dest_name.clone();
                    }
                } else if is_npc_flags(event.dest_flags) && !event.dest_name.is_empty() {
                    // Not gated to player-sourced hits only - matches
                    // combat_log_zone::classify's original semantics
                    // (any damage-to-NPC event counts toward its "sustained,
                    // structured combat" signal, NPC-vs-NPC included), now
                    // tracked incrementally instead of replayed from a
                    // buffered event list at finalize() time.
                    self.npc_hit_count += 1;
                    *self.npc_damage.entry(event.dest_name.clone()).or_insert(0) += event.amount;
                    if let Some(entry_id) = npc_entry_id(&event.dest_guid) {
                        self.npc_entry_ids.entry(event.dest_name.clone()).or_insert(entry_id);
                    }
                }
            }
            CombatEventKind::Heal => {
                if let Some((guid, name)) =
                    self.credited_source(&event.source_guid, &event.source_name, event.source_flags)
                {
                    let p = self.players.entry(guid).or_default();
                    p.healing_done += event.amount;
                    // Compared against the RAW source/dest guids, not the
                    // credited (pet-resolved) source guid above — this asks
                    // "did the caster target themselves", which is what it
                    // means regardless of whether that caster's damage/
                    // healing ends up credited to a pet owner.
                    if event.dest_guid != event.source_guid {
                        p.healing_done_to_others += event.amount;
                    }
                    p.touch(event.timestamp_ms, &name);
                }
            }
            CombatEventKind::Death => {
                if is_player_flags(event.dest_flags) {
                    if let Some(p) = self.players.get_mut(&event.dest_guid) {
                        p.deaths += 1;
                    }
                }
            }
            CombatEventKind::Summon => {
                if is_pet_or_guardian_flags(event.dest_flags) {
                    self.pet_owners.insert(
                        event.dest_guid.clone(),
                        (event.source_guid.clone(), event.source_name.clone()),
                    );
                }
            }
        }
    }

    fn is_timed_out(&self, now_ms: i64, timeout_ms: i64) -> bool {
        now_ms - self.last_event_ms > timeout_ms
    }

    /// Consumes the session, producing its final report plus the raw log
    /// text accumulated alongside it (see raw_lines' doc comment) — None if
    /// no player ever actually did anything (e.g. a session that only ever
    /// saw NPCs fighting each other, or nothing but aura events before
    /// this got filtered out upstream).
    fn finalize(self) -> Option<(EncounterReport, String)> {
        let duration_sec = ((self.last_event_ms - self.start_ms) as f64 / 1000.0).max(0.0);

        if self.players.is_empty() {
            return None;
        }
        // Computed once, across the WHOLE roster, before any individual
        // player's role is decided — see PlayerStats::finalize's own doc
        // comment for why tank inference needs this instead of comparing a
        // player only against their own damage dealt.
        let total_damage_taken: i64 = self.players.values().map(|p| p.damage_taken).sum();
        let avg_damage_taken = total_damage_taken as f64 / self.players.len() as f64;

        let mut players: Vec<PlayerReport> = self
            .players
            .into_iter()
            .filter_map(|(guid, stats)| stats.finalize(guid, avg_damage_taken))
            .collect();
        if players.is_empty() {
            return None;
        }
        players.sort_by_key(|p| std::cmp::Reverse(p.damage_done));

        let analysis = combat_log_zone::classify(self.npc_hit_count, self.event_count, &players);

        let mut npc_names: Vec<(String, i64)> = self.npc_damage.into_iter().collect();
        npc_names.sort_by_key(|(_, damage)| std::cmp::Reverse(*damage));

        let report = EncounterReport {
            session_id: self.session_id,
            start_time_ms: self.start_ms,
            end_time_ms: self.last_event_ms,
            duration_sec: (duration_sec * 100.0).round() / 100.0,
            content_type: analysis.content_type,
            content_quality_score: (analysis.quality_score * 100.0).round() / 100.0,
            event_count: self.event_count,
            npc_names: npc_names.into_iter().map(|(name, _)| name).collect(),
            npc_entry_ids: self.npc_entry_ids,
            players,
        };
        Some((report, self.raw_lines.join("\n")))
    }
}

/// Groups a stream of combat events into encounters, closing (and handing
/// back) whichever session has gone quiet for longer than `timeout_ms` —
/// mirrors wowlogs' CombatDetector, minus the Python version's separate
/// `active_sessions` map that nothing there ever actually populated beyond
/// the single `current_session` field.
pub struct SessionTracker {
    timeout_ms: i64,
    current: Option<CombatSession>,
    /// Real wall-clock instant of the last `record_event` call — used by
    /// `check_idle` instead of the log's own (synthetic, year-2000)
    /// timestamps. wowlogs' Python equivalent compares real `time.time()`
    /// against its log-timestamp-derived `last_event_ts`, which — because
    /// Python's strptime defaults a missing year to 1900 — sits roughly
    /// 126 *years* in the past relative to any real "now": every idle
    /// check there reads as instantly timed out, meaning its background
    /// poll finalizes whatever's accumulated on nearly every ~1s check
    /// cycle instead of ever letting one encounter actually accumulate.
    /// Confirmed empirically (see combat_log_watcher.rs's module docs).
    last_processed_at: Option<std::time::Instant>,
}

impl SessionTracker {
    pub fn new(timeout_ms: i64) -> Self {
        Self {
            timeout_ms,
            current: None,
            last_processed_at: None,
        }
    }

    /// Feeds one already-parsed event in, closing out the previous session
    /// first if it had already gone quiet for longer than the timeout as of
    /// this event's own timestamp — this is what turns a quiet gap *within
    /// the log data itself* into an actual encounter boundary (e.g. catching
    /// up on a batch of lines written while the launcher wasn't running).
    /// Both sides of this comparison are the log's own synthetic
    /// timestamps, so unlike check_idle below, comparing them directly is
    /// fine — no real wall-clock time is involved here.
    ///
    /// Kept alongside record_line (rather than folded into it) since every
    /// existing test constructs a CombatEvent directly instead of a raw log
    /// line — this is the same core state machine record_line delegates to,
    /// just without a raw line to also buffer. Test-only: real production
    /// code (combat_log_watcher) always goes through record_line instead.
    #[cfg(test)]
    pub fn record_event(&mut self, event: CombatEvent) -> Option<EncounterReport> {
        self.record_event_with_raw(event, "")
            .map(|(report, _)| report)
    }

    /// The real entry point used by combat_log_watcher: parses the line
    /// itself (so raw-line buffering and the session state machine below
    /// see lines in the exact same order) and feeds it through. A line that
    /// doesn't parse into a recognized event (aura applied/removed, casts,
    /// misses, ...) doesn't affect session boundaries — nothing here
    /// extracts a timestamp from it — but is still appended to whichever
    /// session is currently open, since that's exactly the detail a
    /// WCL-style backend breakdown needs and the aggregated stats don't
    /// keep. A line arriving before any session is open (no recognized
    /// event has started one yet) is simply not part of any encounter's raw
    /// log — accepted as a minor gap (a pull's opening buff/cast lines,
    /// typically a couple of seconds at most) rather than adding a
    /// pre-buffering scheme for it.
    ///
    /// `reference` is "today" for resolving the line's year-less timestamp
    /// — the caller computes this once per batch of lines rather than this
    /// (or parse_line) recomputing it per line; see
    /// combat_log_parser::today_reference's doc comment.
    pub fn record_line(&mut self, line: &str, reference: NaiveDate) -> Option<(EncounterReport, String)> {
        let Some(event) = crate::combat_log_parser::parse_line(line, reference) else {
            if let Some(session) = self.current.as_mut() {
                session.raw_lines.push(line.to_string());
            }
            return None;
        };

        self.record_event_with_raw(event, line)
    }

    fn record_event_with_raw(
        &mut self,
        event: CombatEvent,
        raw_line: &str,
    ) -> Option<(EncounterReport, String)> {
        self.last_processed_at = Some(std::time::Instant::now());

        let timed_out = self
            .current
            .as_ref()
            .is_some_and(|s| s.is_timed_out(event.timestamp_ms, self.timeout_ms));
        let finished = if timed_out {
            self.current.take().and_then(CombatSession::finalize)
        } else {
            None
        };

        // Deterministic from the log's own content alone (the encounter's
        // start timestamp), not an in-memory counter that resets every
        // launcher restart — a "sync this" or "ignore this" decision keyed
        // by session_id needs the SAME encounter to keep the same ID if
        // this ever gets reprocessed (e.g. an app restart mid-session).
        let session = self.current.get_or_insert_with(|| {
            CombatSession::new(
                format!("session_{}", event.timestamp_ms),
                event.timestamp_ms,
            )
        });
        session.record(event, raw_line);

        finished
    }

    /// Called periodically (not just on new events) so a session ends
    /// promptly after the player stops fighting even if the log file goes
    /// completely quiet — no more lines means record_event's own check
    /// never runs again to notice. Uses real elapsed wall-clock time since
    /// the last line was actually processed, not the log's synthetic
    /// timestamps (see last_processed_at's doc comment for why that
    /// distinction matters).
    pub fn check_idle(&mut self, timeout: std::time::Duration) -> Option<(EncounterReport, String)> {
        let idle = self
            .last_processed_at
            .is_some_and(|t| t.elapsed() > timeout);
        if !idle {
            return None;
        }
        self.current.take().and_then(CombatSession::finalize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_log_parser::{parse_line, today_reference};

    fn dmg(
        t: i64,
        source_guid: &str,
        source_name: &str,
        dest_guid: &str,
        dest_name: &str,
        amount: i64,
    ) -> CombatEvent {
        // 0x512 / 0xa48 are real flag values lifted from
        // combat_log_parser's own fixtures — player and NPC respectively
        // (see that module's flags_classification test) — every test
        // below relies on source=player, dest=NPC classification via
        // these, same as production parsing does.
        CombatEvent {
            timestamp_ms: t,
            kind: CombatEventKind::Damage,
            source_guid: source_guid.to_string(),
            source_name: source_name.to_string(),
            source_flags: 0x512,
            dest_guid: dest_guid.to_string(),
            dest_name: dest_name.to_string(),
            dest_flags: 0xa48,
            ability_id: 1,
            ability_name: "Test".to_string(),
            amount,
            critical: false,
        }
    }

    /// The reverse of dmg() above: NPC source, player dest - what a
    /// player actually TAKING damage looks like on the wire (dmg() is
    /// always player-source/NPC-dest, so it can't represent this).
    fn npc_hits_player(
        t: i64,
        source_guid: &str,
        source_name: &str,
        dest_guid: &str,
        dest_name: &str,
        amount: i64,
    ) -> CombatEvent {
        CombatEvent {
            timestamp_ms: t,
            kind: CombatEventKind::Damage,
            source_guid: source_guid.to_string(),
            source_name: source_name.to_string(),
            source_flags: 0xa48,
            dest_guid: dest_guid.to_string(),
            dest_name: dest_name.to_string(),
            dest_flags: 0x512,
            ability_id: 1,
            ability_name: "Test".to_string(),
            amount,
            critical: false,
        }
    }

    fn heal(
        t: i64,
        source_guid: &str,
        source_name: &str,
        dest_guid: &str,
        dest_name: &str,
        amount: i64,
    ) -> CombatEvent {
        CombatEvent {
            timestamp_ms: t,
            kind: CombatEventKind::Heal,
            source_guid: source_guid.to_string(),
            source_name: source_name.to_string(),
            source_flags: 0x512,
            dest_guid: dest_guid.to_string(),
            dest_name: dest_name.to_string(),
            dest_flags: 0x512,
            ability_id: 1,
            ability_name: "Test Heal".to_string(),
            amount,
            critical: false,
        }
    }

    #[test]
    fn aggregates_damage_and_computes_dps() {
        let mut tracker = SessionTracker::new(5000);
        assert!(tracker
            .record_event(dmg(0, "0x0E1", "Alice", "0xF1", "Boss", 1000))
            .is_none());
        assert!(tracker
            .record_event(dmg(1000, "0x0E1", "Alice", "0xF1", "Boss", 1000))
            .is_none());

        // A later event, well past the timeout, closes out the first
        // session (see a_quiet_gap_longer_than_timeout_splits_into_two_encounters
        // for the dedicated test of that boundary itself).
        let report = tracker
            .record_event(dmg(10_000, "0x0E1", "Alice", "0xF1", "Boss", 1))
            .expect("gap past the timeout should finalize the first session");
        assert_eq!(report.players.len(), 1);
        let alice = &report.players[0];
        assert_eq!(alice.name, "Alice");
        assert_eq!(alice.damage_done, 2000);
        assert_eq!(alice.dps, 2000.0); // 2000 damage / 1s active time
    }

    #[test]
    fn pet_damage_is_credited_to_its_owner_once_a_summon_line_reveals_it() {
        let mut tracker = SessionTracker::new(5000);
        // 0x1112 is the real totem/pet dest-flags value confirmed against
        // this guild's own WoWCombatLog.txt (see combat_log_parser's
        // parses_spell_summon_with_owner_as_source_and_pet_as_dest test).
        let summon = CombatEvent {
            timestamp_ms: 0,
            kind: CombatEventKind::Summon,
            source_guid: "0x0E1".to_string(),
            source_name: "Alice".to_string(),
            source_flags: 0x512,
            dest_guid: "0xF9".to_string(),
            dest_name: "Wolf".to_string(),
            dest_flags: 0x1112,
            ability_id: 0,
            ability_name: String::new(),
            amount: 0,
            critical: false,
        };
        assert!(tracker.record_event(summon).is_none());

        // The pet itself is the SOURCE here, not Alice - without owner
        // resolution this damage wouldn't count toward anyone at all.
        let mut pet_dmg = dmg(1000, "0xF9", "Wolf", "0xF1", "Boss", 500);
        pet_dmg.source_flags = 0x1112;
        tracker.record_event(pet_dmg);

        // The closing event only exists to push the tracker past the
        // timeout (see aggregates_damage_and_computes_dps' own note on this
        // pattern) - its own amount belongs to the NEW session it starts,
        // not the one it finalizes, so it's deliberately not part of the
        // returned report's total below.
        let report = tracker
            .record_event(dmg(10_000, "0x0E1", "Alice", "0xF1", "Boss", 1))
            .expect("gap past the timeout should finalize the session");

        assert_eq!(report.players.len(), 1, "the pet must not show up as its own player entry");
        let alice = &report.players[0];
        assert_eq!(alice.name, "Alice");
        assert_eq!(alice.damage_done, 500); // just the pet's credited damage
    }

    #[test]
    fn pet_damage_with_no_known_owner_is_dropped_not_faked_as_a_player() {
        let mut tracker = SessionTracker::new(5000);
        // No SPELL_SUMMON was ever seen for "0xF9" this session (e.g. the
        // pet was already out before the log started watching) - its
        // damage should be silently dropped, not attributed to a fake
        // "Wolf" player entry.
        let mut pet_dmg = dmg(0, "0xF9", "Wolf", "0xF1", "Boss", 500);
        pet_dmg.source_flags = 0x1112;
        assert!(tracker.record_event(pet_dmg).is_none());

        // The session that only ever saw the dropped pet damage has no
        // real player action at all, so it finalizes to nothing (see
        // CombatSession::finalize's own doc comment) rather than a report
        // with zero players - Alice's own event below starts a brand new
        // session, which is what actually gets checked here via check_idle.
        tracker.record_event(dmg(10_000, "0x0E1", "Alice", "0xF1", "Boss", 1));
        let (report, _) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");
        assert_eq!(report.players.len(), 1);
        assert_eq!(report.players[0].name, "Alice");
    }

    #[test]
    fn npc_vs_npc_damage_is_not_tracked_as_a_player() {
        let mut tracker = SessionTracker::new(5000);
        let npc_vs_npc = CombatEvent {
            timestamp_ms: 0,
            kind: CombatEventKind::Damage,
            source_guid: "0xF1".to_string(),
            source_name: "Boss".to_string(),
            source_flags: 0xa48,
            dest_guid: "0xF2".to_string(),
            dest_name: "Add".to_string(),
            dest_flags: 0xa48,
            ability_id: 1,
            ability_name: "Test".to_string(),
            amount: 500,
            critical: false,
        };
        tracker.record_event(npc_vs_npc.clone());
        let mut later = npc_vs_npc;
        later.timestamp_ms = 10_000;
        let report = tracker.record_event(later);
        // No player ever did anything - nothing to report, even once the
        // session closes out.
        assert!(report.is_none());
    }

    #[test]
    fn check_idle_finalizes_after_real_wall_clock_time_passes_with_no_new_events() {
        let mut tracker = SessionTracker::new(5000);
        tracker.record_event(dmg(0, "0x0E1", "Alice", "0xF1", "Boss", 100));

        assert!(tracker
            .check_idle(std::time::Duration::from_millis(500))
            .is_none());
        std::thread::sleep(std::time::Duration::from_millis(50));
        let (report, _raw_log) = tracker
            .check_idle(std::time::Duration::from_millis(10))
            .expect("should have gone idle by now");
        assert_eq!(report.players[0].damage_done, 100);
    }

    #[test]
    fn a_quiet_gap_longer_than_timeout_splits_into_two_encounters() {
        let mut tracker = SessionTracker::new(5000);
        tracker.record_event(dmg(0, "0x0E1", "Alice", "0xF1", "Boss", 100));
        // 10s later - well past the 5s timeout - the NEXT event should
        // close out the first session before starting a new one.
        let finished = tracker.record_event(dmg(10_000, "0x0E1", "Alice", "0xF1", "Boss", 200));
        assert!(finished.is_some());
        assert_eq!(finished.unwrap().players[0].damage_done, 100);
    }

    #[test]
    fn real_log_lines_produce_a_sensible_report() {
        let lines = [
            r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#,
            r#"2/28 14:37:00.673  SWING_DAMAGE,0xF1300068B1000064,"Horde Ranger",0xa48,0x0E000000001B5352,"Pzkpfwagen",0x512,1010,0,1,0,0,0,nil,nil,nil"#,
            r#"2/28 14:36:28.966  SPELL_HEAL,0x0E000000000C2704,"Mamkedy",0x511,0x0E000000001B5352,"Pzkpfwagen",0x512,56160,"Glyph of Power Word: Shield",0x2,1036,598,0,1"#,
            r#"2/28 14:37:31.038  UNIT_DIED,0x0000000000000000,nil,0x80000000,0xF1300068B1000065,"Horde Ranger",0xa48"#,
        ];

        let mut tracker = SessionTracker::new(5000);
        let reference = today_reference();
        let mut finished = None;
        for line in lines {
            let event = parse_line(line, reference).expect("fixture lines must parse");
            if let Some(report) = tracker.record_event(event) {
                finished = Some(report);
            }
        }
        finished = finished.or_else(|| {
            tracker
                .check_idle(std::time::Duration::from_secs(0))
                .map(|(report, _)| report)
        });

        let report = finished.expect("should have a finalized encounter");
        let pzk = report
            .players
            .iter()
            .find(|p| p.name == "Pzkpfwagen")
            .expect("Pzkpfwagen should be tracked");
        assert_eq!(pzk.damage_done, 342);
        assert_eq!(pzk.damage_taken, 1010);
        // Pzkpfwagen was healed, but healing_done credits whoever CAST the
        // heal (Mamkedy), not its target - "healing received" isn't a
        // tracked stat, matching wowlogs' own scope.
        assert_eq!(pzk.healing_done, 0);
        let mamkedy = report
            .players
            .iter()
            .find(|p| p.name == "Mamkedy")
            .expect("Mamkedy should be tracked");
        assert_eq!(mamkedy.healing_done, 1036);
        assert_eq!(mamkedy.damage_done, 0);
        assert_eq!(report.npc_names, vec!["Horde Ranger".to_string()]);
        assert_eq!(report.npc_entry_ids.get("Horde Ranger"), Some(&0x0068B1));
    }

    #[test]
    fn npc_names_are_sorted_by_damage_taken_most_first() {
        let mut tracker = SessionTracker::new(5000);
        // The add takes less total damage than the boss, despite being hit
        // first - order must come from total damage, not first-seen order.
        tracker.record_event(dmg(0, "0x0E1", "Alice", "0xF1", "Add", 100));
        tracker.record_event(dmg(1000, "0x0E1", "Alice", "0xF2", "Boss", 500));
        tracker.record_event(dmg(2000, "0x0E1", "Alice", "0xF2", "Boss", 500));

        let report = tracker
            .record_event(dmg(10_000, "0x0E1", "Alice", "0xF2", "Boss", 1))
            .expect("gap past the timeout should finalize the session");
        assert_eq!(
            report.npc_names,
            vec!["Boss".to_string(), "Add".to_string()]
        );
    }

    #[test]
    fn record_line_buffers_every_raw_line_including_unrecognized_event_types() {
        // SPELL_AURA_APPLIED isn't in DAMAGE_EVENTS/HEAL_EVENTS and isn't
        // UNIT_DIED, so parse_line returns None for it - it should still end
        // up in the raw log verbatim, since a WCL-style backend breakdown
        // needs buff/debuff lines just as much as damage/heal ones.
        let lines = [
            r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#,
            r#"2/28 14:36:59.500  SPELL_AURA_APPLIED,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,12345,"Some Debuff",0x1,DEBUFF"#,
            r#"2/28 14:37:00.673  SWING_DAMAGE,0xF1300068B1000064,"Horde Ranger",0xa48,0x0E000000001B5352,"Pzkpfwagen",0x512,1010,0,1,0,0,0,nil,nil,nil"#,
        ];

        let mut tracker = SessionTracker::new(5000);
        let reference = today_reference();
        for line in lines {
            assert!(tracker.record_line(line, reference).is_none());
        }
        let (_report, raw_log) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");

        for line in lines {
            assert!(
                raw_log.contains(line),
                "raw log should contain every line seen, including unrecognized ones: {line}"
            );
        }
    }

    #[test]
    fn record_line_before_any_session_is_open_is_not_buffered() {
        // No recognized event has opened a session yet, so this line isn't
        // part of any encounter's raw log - a documented, accepted gap (see
        // record_line's own doc comment) rather than a pre-buffering scheme.
        let unrecognized_before_session =
            r#"2/28 14:36:58.000  SPELL_AURA_APPLIED,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,12345,"Some Buff",0x1,BUFF"#;
        let first_damage_line =
            r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#;

        let mut tracker = SessionTracker::new(5000);
        let reference = today_reference();
        assert!(tracker.record_line(unrecognized_before_session, reference).is_none());
        assert!(tracker.record_line(first_damage_line, reference).is_none());

        let (_report, raw_log) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");
        assert!(!raw_log.contains("Some Buff"));
        assert!(raw_log.contains(first_damage_line));
    }

    #[test]
    fn a_tank_who_mostly_heals_themselves_is_not_misclassified_as_healer() {
        let mut tracker = SessionTracker::new(5000);
        // Blood DK-style profile: takes clearly more punishment than the
        // rest of the roster, and "heals" almost entirely itself (Death
        // Strike/Rune Tap) - exactly the combination that used to come back
        // Healer, since the old formula counted every heal cast, self-
        // targeted or not, toward the healer threshold.
        tracker.record_event(dmg(0, "0xTANK", "Grondar", "0xBoss", "Boss", 300));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xTANK", "Grondar", 150));
        tracker.record_event(heal(0, "0xTANK", "Grondar", "0xTANK", "Grondar", 100));
        tracker.record_event(dmg(0, "0xDPS1", "Alice", "0xBoss", "Boss", 200));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xDPS1", "Alice", 10));
        tracker.record_event(dmg(0, "0xDPS2", "Bob", "0xBoss", "Boss", 180));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xDPS2", "Bob", 15));

        let (report, _) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");
        let tank = report.players.iter().find(|p| p.name == "Grondar").unwrap();
        assert_eq!(tank.role, Role::Tank);
    }

    #[test]
    fn a_real_healer_whose_healing_lands_on_others_is_still_classified_as_healer() {
        let mut tracker = SessionTracker::new(5000);
        tracker.record_event(dmg(0, "0xHEALER", "Priestly", "0xBoss", "Boss", 50));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xHEALER", "Priestly", 5));
        tracker.record_event(heal(0, "0xHEALER", "Priestly", "0xTANK", "Grondar", 100));
        tracker.record_event(dmg(0, "0xTANK", "Grondar", "0xBoss", "Boss", 300));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xTANK", "Grondar", 150));

        let (report, _) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");
        let healer = report.players.iter().find(|p| p.name == "Priestly").unwrap();
        assert_eq!(healer.role, Role::Healer);
    }

    #[test]
    fn tank_is_classified_by_damage_taken_relative_to_the_roster_average_not_their_own_damage() {
        let mut tracker = SessionTracker::new(5000);
        // The tank deals more than 3x what they take - the old
        // `damage_taken > damage_done * 2` threshold would have called
        // this Dps, exactly the failure mode reported against a real log
        // (a tank whose gear/threat rotation lets them out-damage what
        // they take).
        tracker.record_event(dmg(0, "0xTANK", "Grondar", "0xBoss", "Boss", 1000));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xTANK", "Grondar", 300));
        tracker.record_event(dmg(0, "0xDPS1", "Alice", "0xBoss", "Boss", 200));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xDPS1", "Alice", 20));
        tracker.record_event(dmg(0, "0xDPS2", "Bob", "0xBoss", "Boss", 200));
        tracker.record_event(npc_hits_player(0, "0xBoss", "Boss", "0xDPS2", "Bob", 25));

        let (report, _) = tracker
            .check_idle(std::time::Duration::from_secs(0))
            .expect("should have a finalized encounter");
        let tank = report.players.iter().find(|p| p.name == "Grondar").unwrap();
        assert_eq!(tank.role, Role::Tank);
    }
}
