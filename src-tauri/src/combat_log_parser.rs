// Parses raw WoW 3.3.5a combat log lines into structured events.
//
// Ported from wowlogs/normalizer.py (a Python prototype in this workspace),
// fixing two indexing bugs found there by checking real WoWCombatLog.txt
// samples against the actual field layout:
//
// - SWING_DAMAGE's amount was read from the OVERKILL field (one past the
//   real amount) and its critical flag from a field well past the end of
//   the line (always false) — SWING_DAMAGE's own suffix is
//   amount,overkill,school,resisted,blocked,absorbed,critical,glancing,
//   crushing, it isn't shifted the way SPELL_* events are by the extra
//   spellId/spellName/spellSchool fields.
// - SPELL_HEAL/SPELL_PERIODIC_HEAL's critical flag was likewise read from
//   an out-of-range index (always false) — heal lines end right after
//   amount,overheal,absorbed,critical, with no trailing glancing/crushing
//   fields the way damage lines have.

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Bratislava;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatEventKind {
    Damage,
    Heal,
    Death,
    /// SPELL_SUMMON — source is the owner, dest is the pet/totem/guardian
    /// just summoned. Not damage/heal/death itself, but combat_log_session
    /// needs it to learn a pet's owner *before* that pet's own damage/heal
    /// lines show up, so a hunter/shaman/etc.'s pet contribution can be
    /// credited to them instead of silently dropped (a pet's flags carry
    /// TYPE_PET/TYPE_GUARDIAN, neither TYPE_PLAYER nor TYPE_NPC).
    Summon,
}

#[derive(Debug, Clone)]
pub struct CombatEvent {
    /// Real epoch milliseconds (UTC). The log's own timestamps are in the
    /// local time of whoever wrote them, with no timezone or year attached,
    /// so every timestamp is interpreted as Europe/Bratislava (CET/CEST,
    /// DST-aware) since this guild runs on Slovak time, regardless of
    /// which timezone the machine parsing it happens to be in; the year is
    /// guessed from today's date in that same timezone (see guess_year).
    /// Wrong only if a single log file's span is over a year old, or
    /// actually straddles a real New Year's Eve midnight (both misorder
    /// here the same way the wowlogs Python tool's own fixed-1900-year
    /// strptime does) - accepted for the same reason: rare, and this log
    /// is normally at most a few days old (see combat_log_watcher.rs's
    /// truncation-on-restart notes).
    pub timestamp_ms: i64,
    pub kind: CombatEventKind,
    pub source_guid: String,
    pub source_name: String,
    /// Blizzard's standard COMBATLOG_OBJECT_* bitflags for the source
    /// (raw log field [3]) — see is_player_flags/is_npc_flags. Kept
    /// around (not just consumed inline while parsing) since
    /// combat_log_session needs to classify both source and dest.
    pub source_flags: i64,
    pub dest_guid: String,
    pub dest_name: String,
    pub dest_flags: i64,
    // Parsed correctly (see the module doc comment's bug notes) but not
    // consumed anywhere yet — combat_log_session's aggregator only tracks
    // totals per player, not a per-ability breakdown the way wowlogs'
    // aggregator.py does. Left in since they're inherent to what a combat
    // log event actually is, ready for that breakdown to be added later
    // without re-parsing.
    #[allow(dead_code)]
    pub ability_id: i64,
    #[allow(dead_code)]
    pub ability_name: String,
    pub amount: i64,
    #[allow(dead_code)]
    pub critical: bool,
}

const DAMAGE_EVENTS: &[&str] = &[
    "SWING_DAMAGE",
    "SPELL_DAMAGE",
    "RANGE_DAMAGE",
    "SPELL_PERIODIC_DAMAGE",
    "DAMAGE_SPLIT",
];
const HEAL_EVENTS: &[&str] = &["SPELL_HEAL", "SPELL_PERIODIC_HEAL"];

/// Blizzard's own COMBATLOG_OBJECT_TYPE_PLAYER bit (stable across every
/// client version this log format exists in, unlike a GUID-prefix guess) —
/// verified against this module's own test fixtures: "Pzkpfwagen" (a real
/// player) carries source flags 0x512, which is exactly
/// TYPE_PLAYER(0x400) | CONTROL_PLAYER(0x100) | REACTION_FRIENDLY(0x10) |
/// AFFILIATION_PARTY(0x2).
const FLAG_TYPE_PLAYER: i64 = 0x400;
/// COMBATLOG_OBJECT_TYPE_NPC — verified the same way against "Horde
/// Ranger" (a real NPC)'s dest flags 0xa48 = TYPE_NPC(0x800) |
/// CONTROL_NPC(0x200) | REACTION_HOSTILE(0x40) | AFFILIATION_OUTSIDER(0x8).
const FLAG_TYPE_NPC: i64 = 0x800;
/// COMBATLOG_OBJECT_TYPE_PET — verified against a real SPELL_SUMMON line in
/// this guild's own WoWCombatLog.txt: a shaman's Earth Elemental Totem
/// carries dest flags 0x1112 = TYPE_PET(0x1000) | CONTROL_PLAYER(0x100) |
/// REACTION_FRIENDLY(0x10) | AFFILIATION_PARTY(0x2).
const FLAG_TYPE_PET: i64 = 0x1000;
/// COMBATLOG_OBJECT_TYPE_GUARDIAN — same bit family as TYPE_PET (Blizzard's
/// own distinction between a hunter/warlock pet and a spell-summoned
/// guardian like a mage's Water Elemental or a DK's ghoul), not separately
/// confirmed against a live example the way TYPE_PET was, but the standard
/// bit per Blizzard's own COMBATLOG_OBJECT_TYPE_MASK layout.
const FLAG_TYPE_GUARDIAN: i64 = 0x2000;

/// Real player actor, per the log's own flags field — replaces an earlier
/// GUID-hex-prefix guess ("0x0E..." = player on this specific server) with
/// the standard Blizzard bitflag every combat log line already carries,
/// which doesn't depend on any one server's own GUID numbering scheme.
pub fn is_player_flags(flags: i64) -> bool {
    flags & FLAG_TYPE_PLAYER != 0
}

/// NPC (boss/trash) actor, per the log's own flags field — see
/// is_player_flags. A hunter pet/totem/guardian is neither this nor
/// is_player_flags (COMBATLOG_OBJECT_TYPE_PET/TYPE_GUARDIAN are their own
/// distinct bits), same as they fell through the old GUID-prefix guess too.
pub fn is_npc_flags(flags: i64) -> bool {
    flags & FLAG_TYPE_NPC != 0
}

/// A player's (or, per the Cat/Wolf/etc. example seen live, an NPC's) own
/// pet, totem, or guardian — combat_log_session uses this to redirect a
/// pet's damage/healing to whichever player it learned owns that pet from
/// an earlier SPELL_SUMMON line, instead of it silently not counting
/// toward anyone (neither is_player_flags nor is_npc_flags matches a pet).
pub fn is_pet_or_guardian_flags(flags: i64) -> bool {
    flags & (FLAG_TYPE_PET | FLAG_TYPE_GUARDIAN) != 0
}

/// Extracts the creature template ("entry") ID embedded in a WotLK-style
/// low GUID — 0xF130<entry:6 hex><counter:6 hex>, the standard 3.3.5a unit
/// GUID layout, confirmed against this module's own "Horde Ranger" test
/// fixture (0xF1300068B1000064 -> entry 0x0068B1). A stable numeric key
/// for "which NPC template is this" that doesn't depend on display name —
/// several distinct NPCs can share a name, and a boss's title can vary —
/// unlike EncounterReport::npc_names, which is intentionally still
/// name-keyed (see its own doc comment for why). None for anything that
/// doesn't match this exact shape: players, game objects, or (since this
/// is inferred from the standard layout, not confirmed against this
/// particular server's own GUID generation) a server using a nonstandard
/// variant.
pub fn npc_entry_id(guid: &str) -> Option<i64> {
    let hex = guid.strip_prefix("0xF130")?;
    if hex.len() < 6 {
        return None;
    }
    i64::from_str_radix(&hex[..6], 16).ok()
}

/// "Today" in the log's own Bratislava timezone — see CombatEvent's doc
/// comment. Computed once per batch by the caller (combat_log_watcher's
/// process_lines) and threaded through parse_line/parse_timestamp rather
/// than recomputed per line: a real timezone-aware `Utc::now()` call on
/// every one of a raid log's hundreds of thousands of lines is pure waste
/// when a whole batch is processed within milliseconds of each other.
pub fn today_reference() -> NaiveDate {
    Utc::now().with_timezone(&Bratislava).date_naive()
}

/// Splits `rest` on commas, except inside a "..." quoted field — a plain
/// `rest.split(',')` desyncs every field after a name/spell string that
/// happens to contain a literal comma. This log format has no escaping for
/// a quote or comma embedded inside a quoted field, so this only needs to
/// track whether it's currently inside a quote, not handle an escaped `\"`.
fn split_fields(rest: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(&rest[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&rest[start..]);
    fields
}

/// Parses one raw line (including its leading "M/D HH:MM:SS.mmm  "
/// timestamp) into a structured event, or None for event types this
/// doesn't track (aura applications, casts, resurrects, etc. — only
/// damage/heal/death feed the aggregator) or a line that doesn't parse.
/// `reference` is "today" for resolving this line's year-less timestamp —
/// see today_reference's doc comment for why it's a parameter instead of
/// computed here.
pub fn parse_line(line: &str, reference: NaiveDate) -> Option<CombatEvent> {
    let (timestamp_str, rest) = line.split_once("  ")?;
    let timestamp_ms = parse_timestamp(timestamp_str.trim(), reference)?;

    let fields = split_fields(rest);
    if fields.len() < 7 {
        return None;
    }

    let event_type = fields[0];
    let source_guid = unquote(fields[1]);
    let source_name = unquote(fields[2]);
    let source_flags = parse_hex_flags(fields[3]);
    let dest_guid = unquote(fields[4]);
    let dest_name = unquote(fields[5]);
    let dest_flags = parse_hex_flags(fields[6]);

    if DAMAGE_EVENTS.contains(&event_type) {
        parse_damage(
            event_type,
            &fields,
            timestamp_ms,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
        )
    } else if HEAL_EVENTS.contains(&event_type) {
        parse_heal(
            &fields,
            timestamp_ms,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
        )
    } else if event_type == "UNIT_DIED" {
        Some(CombatEvent {
            timestamp_ms,
            kind: CombatEventKind::Death,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
            ability_id: 0,
            ability_name: String::new(),
            amount: 0,
            critical: false,
        })
    } else if event_type == "SPELL_SUMMON" {
        // source = owner, dest = the pet/totem/guardian just summoned -
        // see CombatEventKind::Summon's doc comment for why this is
        // tracked at all despite carrying no damage/heal amount itself.
        Some(CombatEvent {
            timestamp_ms,
            kind: CombatEventKind::Summon,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
            ability_id: 0,
            ability_name: String::new(),
            amount: 0,
            critical: false,
        })
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_damage(
    event_type: &str,
    fields: &[&str],
    timestamp_ms: i64,
    source_guid: String,
    source_name: String,
    source_flags: i64,
    dest_guid: String,
    dest_name: String,
    dest_flags: i64,
) -> Option<CombatEvent> {
    if event_type == "SWING_DAMAGE" {
        // amount,overkill,school,resisted,blocked,absorbed,critical,glancing,crushing
        let amount = get_i64(fields, 7)?;
        let critical = get(fields, 13) == Some("1");
        Some(CombatEvent {
            timestamp_ms,
            kind: CombatEventKind::Damage,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
            ability_id: 0,
            ability_name: "Melee".to_string(),
            amount,
            critical,
        })
    } else {
        // spellId,"spellName",spellSchool,amount,overkill,school,resisted,blocked,absorbed,critical,glancing,crushing
        let ability_id = get_i64(fields, 7)?;
        let ability_name = get(fields, 8).map(unquote).unwrap_or_default();
        let amount = get_i64(fields, 10)?;
        let critical = get(fields, 16) == Some("1");
        Some(CombatEvent {
            timestamp_ms,
            kind: CombatEventKind::Damage,
            source_guid,
            source_name,
            source_flags,
            dest_guid,
            dest_name,
            dest_flags,
            ability_id,
            ability_name,
            amount,
            critical,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_heal(
    fields: &[&str],
    timestamp_ms: i64,
    source_guid: String,
    source_name: String,
    source_flags: i64,
    dest_guid: String,
    dest_name: String,
    dest_flags: i64,
) -> Option<CombatEvent> {
    // spellId,"spellName",spellSchool,amount,overheal,absorbed,critical
    let ability_id = get_i64(fields, 7)?;
    let ability_name = get(fields, 8).map(unquote).unwrap_or_default();
    let amount = get_i64(fields, 10)?;
    let critical = get(fields, 13) == Some("1");
    Some(CombatEvent {
        timestamp_ms,
        kind: CombatEventKind::Heal,
        source_guid,
        source_name,
        source_flags,
        dest_guid,
        dest_name,
        dest_flags,
        ability_id,
        ability_name,
        amount,
        critical,
    })
}

fn get<'a>(fields: &[&'a str], idx: usize) -> Option<&'a str> {
    fields.get(idx).copied()
}

fn get_i64(fields: &[&str], idx: usize) -> Option<i64> {
    get(fields, idx)?.trim().parse().ok()
}

fn unquote(field: &str) -> String {
    field.trim().trim_matches('"').to_string()
}

/// Parses a flags field like "0x512" (or "0xa48") into its numeric value —
/// 0 for anything malformed rather than failing the whole line over it,
/// since a flags field only ever gates an is_player_flags/is_npc_flags
/// bit-test, and 0 safely reads as neither.
fn parse_hex_flags(field: &str) -> i64 {
    let trimmed = field.trim();
    let digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    i64::from_str_radix(digits, 16).unwrap_or(0)
}

/// This log format has no year - guessed from `reference` (real "today")
/// under the assumption the log is recent: a log month AFTER today's own
/// month must be from last year (e.g. today is January, log line is month
/// 12 - that's last December, not 11 months in the future), never further
/// back than that. Wrong for a log file spanning more than one such
/// rollover (i.e. genuinely over a year old) - see CombatEvent::timestamp_ms.
fn guess_year(log_month: u32, reference: NaiveDate) -> i32 {
    if log_month > reference.month() {
        reference.year() - 1
    } else {
        reference.year()
    }
}

fn parse_timestamp(s: &str, reference: NaiveDate) -> Option<i64> {
    let (date_part, time_part) = s.split_once(' ')?;
    let (month, day) = date_part.split_once('/')?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;

    let (hms, millis_str) = time_part.split_once('.')?;
    let mut hms_parts = hms.split(':');
    let hour: u32 = hms_parts.next()?.parse().ok()?;
    let minute: u32 = hms_parts.next()?.parse().ok()?;
    let second: u32 = hms_parts.next()?.parse().ok()?;
    let millis: i64 = millis_str.parse().ok()?;

    let year = guess_year(month, reference);
    // A Feb 29 log line landing on a guessed non-leap year fails to parse
    // (None) rather than panicking - graceful, and rare enough (needs a
    // capture spanning a leap day specifically) not to warrant more.
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let naive_datetime = date.and_hms_opt(hour, minute, second)?;

    // The log's own timestamp is Bratislava local time, not UTC - treating
    // it as UTC directly (as an earlier version of this function did) put
    // every event 1-2 hours off (CET/CEST), silently, on every single line.
    // .single() rejects the two rare cases where a local time doesn't map
    // to exactly one UTC instant: the DST "spring forward" gap (that local
    // time never happened) and the "fall back" hour (it happened twice) -
    // both only possible right at Bratislava's own DST transition, a few
    // hours twice a year.
    let localized = Bratislava.from_local_datetime(&naive_datetime).single()?;
    Some(localized.timestamp_millis() + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swing_damage_amount_and_critical_correctly() {
        let line = r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Damage);
        assert_eq!(event.amount, 342);
        assert!(!event.critical);
        assert_eq!(event.source_name, "Pzkpfwagen");
        assert_eq!(event.dest_name, "Horde Ranger");
    }

    #[test]
    fn parses_spell_damage() {
        let line = r#"2/28 14:36:59.472  SPELL_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,53595,"Hammer of the Righteous",0x2,786,0,2,0,0,0,nil,nil,nil"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Damage);
        assert_eq!(event.ability_id, 53595);
        assert_eq!(event.ability_name, "Hammer of the Righteous");
        assert_eq!(event.amount, 786);
        assert!(!event.critical);
    }

    #[test]
    fn parses_spell_heal_with_critical() {
        let line = r#"2/28 14:36:28.966  SPELL_HEAL,0x0E000000000C2704,"Mamkedy",0x511,0x0E000000001B5352,"Pzkpfwagen",0x512,56160,"Glyph of Power Word: Shield",0x2,1036,598,0,1"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Heal);
        assert_eq!(event.amount, 1036);
        assert!(event.critical);
    }

    #[test]
    fn parses_unit_died() {
        let line = r#"2/28 14:37:31.038  UNIT_DIED,0x0000000000000000,nil,0x80000000,0xF1300068B1000065,"Horde Ranger",0xa48"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Death);
        assert_eq!(event.dest_name, "Horde Ranger");
    }

    #[test]
    fn ignores_untracked_event_types() {
        let line = r#"2/28 14:36:09.732  SPELL_CAST_SUCCESS,0x0E000000001AD363,"Luciernagaxd",0x512,0x0E00000000105932,"Aogus",0x512,27126,"Arcane Intellect",0x40"#;
        assert!(parse_line(line, today_reference()).is_none());
    }

    #[test]
    fn a_literal_comma_inside_a_quoted_field_does_not_desync_later_fields() {
        // A naive rest.split(',') would treat "Bob, the Ranger"'s embedded
        // comma as a field boundary, shifting every field after the name
        // (flags, dest, amount, ...) by one - split_fields must not do that.
        let line = r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Bob, the Ranger",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.source_name, "Bob, the Ranger");
        assert_eq!(event.amount, 342);
    }

    #[test]
    fn flags_classification() {
        // Real values straight off this module's own fixture lines above:
        // Pzkpfwagen (a player) carries source flags 0x512, Horde Ranger
        // (an NPC) carries dest flags 0xa48.
        assert!(is_player_flags(0x512));
        assert!(!is_npc_flags(0x512));
        assert!(is_npc_flags(0xa48));
        assert!(!is_player_flags(0xa48));
    }

    #[test]
    fn parses_spell_summon_with_owner_as_source_and_pet_as_dest() {
        // A real line from this guild's own WoWCombatLog.txt: Bonoboltic
        // (a shaman, player flags 0x512) summoning their Earth Elemental
        // Totem (pet flags 0x1112 - includes TYPE_PET 0x1000).
        let line = r#"6/15 22:14:55.985  SPELL_SUMMON,0x0E000000000CF5AA,"Bonoboltic",0x512,0xF130003C46001B4D,"Earth Elemental Totem",0x1112,2062,"Earth Elemental Totem",0x1"#;
        let event = parse_line(line, today_reference()).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Summon);
        assert_eq!(event.source_name, "Bonoboltic");
        assert!(is_player_flags(event.source_flags));
        assert_eq!(event.dest_name, "Earth Elemental Totem");
        assert!(is_pet_or_guardian_flags(event.dest_flags));
    }

    #[test]
    fn pet_and_guardian_flags_classification() {
        // 0x1112 is the real totem dest flags from the fixture above.
        assert!(is_pet_or_guardian_flags(0x1112));
        assert!(!is_player_flags(0x1112));
        assert!(!is_npc_flags(0x1112));
        // A player and an NPC are never also a pet/guardian.
        assert!(!is_pet_or_guardian_flags(0x512));
        assert!(!is_pet_or_guardian_flags(0xa48));
    }

    #[test]
    fn npc_entry_id_extracts_from_a_standard_wotlk_unit_guid() {
        assert_eq!(npc_entry_id("0xF1300068B1000064"), Some(0x0068B1));
        assert_eq!(npc_entry_id("0x0E000000001B5352"), None); // a player GUID
    }

    #[test]
    fn guessed_year_matches_todays_when_log_month_is_not_later() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        assert_eq!(guess_year(7, today), 2026); // same month
        assert_eq!(guess_year(1, today), 2026); // earlier this year
    }

    #[test]
    fn guessed_year_rolls_back_when_log_month_is_later_than_today() {
        // Today is January - a log line from month 12 must be LAST
        // December, not 11 months in the future.
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(guess_year(12, today), 2025);
    }

    #[test]
    fn parse_timestamp_uses_the_guessed_year() {
        let reference = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let ms = parse_timestamp("12/31 23:59:59.500", reference).unwrap();
        let naive = NaiveDate::from_ymd_opt(2025, 12, 31)
            .unwrap()
            .and_hms_opt(23, 59, 59)
            .unwrap();
        let expected = Bratislava
            .from_local_datetime(&naive)
            .single()
            .unwrap()
            .timestamp_millis()
            + 500;
        assert_eq!(ms, expected);
    }

    #[test]
    fn parse_timestamp_interprets_the_log_as_bratislava_time_not_utc() {
        // 14:36:59 CEST (summer, UTC+2) must be 12:36:59 UTC, not 14:36:59 UTC -
        // this is exactly the bug an earlier version of this function had
        // (treating the log's local time as if it were already UTC).
        let reference = NaiveDate::from_ymd_opt(2026, 7, 28).unwrap();
        let ms = parse_timestamp("7/28 14:36:59.000", reference).unwrap();
        let expected_utc = NaiveDate::from_ymd_opt(2026, 7, 28)
            .unwrap()
            .and_hms_opt(12, 36, 59)
            .unwrap()
            .and_utc();
        assert_eq!(ms, expected_utc.timestamp_millis());
    }
}
