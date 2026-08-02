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
    pub dest_guid: String,
    pub dest_name: String,
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

/// NPC (boss/trash) GUIDs on this server start "0xF..." or "0x2..." —
/// players are always "0x0E...". Matches wowlogs' own is_boss_guid heuristic.
pub fn is_npc_guid(guid: &str) -> bool {
    guid.starts_with("0xF") || guid.starts_with("0x2")
}

/// Real player GUIDs, as opposed to NPCs or the all-zero "no source"
/// placeholder UNIT_DIED uses for environmental deaths.
pub fn is_player_guid(guid: &str) -> bool {
    guid.starts_with("0x0E")
}

/// Parses one raw line (including its leading "M/D HH:MM:SS.mmm  "
/// timestamp) into a structured event, or None for event types this
/// doesn't track (aura applications, casts, resurrects, etc. — only
/// damage/heal/death feed the aggregator) or a line that doesn't parse.
pub fn parse_line(line: &str) -> Option<CombatEvent> {
    let (timestamp_str, rest) = line.split_once("  ")?;
    let timestamp_ms = parse_timestamp(timestamp_str.trim())?;

    let fields: Vec<&str> = rest.split(',').collect();
    if fields.len() < 7 {
        return None;
    }

    let event_type = fields[0];
    let source_guid = unquote(fields[1]);
    let source_name = unquote(fields[2]);
    let dest_guid = unquote(fields[4]);
    let dest_name = unquote(fields[5]);

    if DAMAGE_EVENTS.contains(&event_type) {
        parse_damage(
            event_type,
            &fields,
            timestamp_ms,
            source_guid,
            source_name,
            dest_guid,
            dest_name,
        )
    } else if HEAL_EVENTS.contains(&event_type) {
        parse_heal(
            &fields,
            timestamp_ms,
            source_guid,
            source_name,
            dest_guid,
            dest_name,
        )
    } else if event_type == "UNIT_DIED" {
        Some(CombatEvent {
            timestamp_ms,
            kind: CombatEventKind::Death,
            source_guid,
            source_name,
            dest_guid,
            dest_name,
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
    dest_guid: String,
    dest_name: String,
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
            dest_guid,
            dest_name,
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
            dest_guid,
            dest_name,
            ability_id,
            ability_name,
            amount,
            critical,
        })
    }
}

fn parse_heal(
    fields: &[&str],
    timestamp_ms: i64,
    source_guid: String,
    source_name: String,
    dest_guid: String,
    dest_name: String,
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
        dest_guid,
        dest_name,
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

fn parse_timestamp(s: &str) -> Option<i64> {
    // "Today" in the same timezone every timestamp itself is interpreted
    // in below - not the machine's own local date, which could be a
    // different calendar day right around midnight if this ever runs
    // somewhere other than Slovakia.
    let reference = Utc::now().with_timezone(&Bratislava).date_naive();
    parse_timestamp_relative_to(s, reference)
}

/// Split out from parse_timestamp so guess_year's date-guessing logic can
/// be tested against a fixed reference date instead of the real "today".
fn parse_timestamp_relative_to(s: &str, reference: NaiveDate) -> Option<i64> {
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
        let event = parse_line(line).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Damage);
        assert_eq!(event.amount, 342);
        assert!(!event.critical);
        assert_eq!(event.source_name, "Pzkpfwagen");
        assert_eq!(event.dest_name, "Horde Ranger");
    }

    #[test]
    fn parses_spell_damage() {
        let line = r#"2/28 14:36:59.472  SPELL_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,53595,"Hammer of the Righteous",0x2,786,0,2,0,0,0,nil,nil,nil"#;
        let event = parse_line(line).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Damage);
        assert_eq!(event.ability_id, 53595);
        assert_eq!(event.ability_name, "Hammer of the Righteous");
        assert_eq!(event.amount, 786);
        assert!(!event.critical);
    }

    #[test]
    fn parses_spell_heal_with_critical() {
        let line = r#"2/28 14:36:28.966  SPELL_HEAL,0x0E000000000C2704,"Mamkedy",0x511,0x0E000000001B5352,"Pzkpfwagen",0x512,56160,"Glyph of Power Word: Shield",0x2,1036,598,0,1"#;
        let event = parse_line(line).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Heal);
        assert_eq!(event.amount, 1036);
        assert!(event.critical);
    }

    #[test]
    fn parses_unit_died() {
        let line = r#"2/28 14:37:31.038  UNIT_DIED,0x0000000000000000,nil,0x80000000,0xF1300068B1000065,"Horde Ranger",0xa48"#;
        let event = parse_line(line).expect("should parse");
        assert_eq!(event.kind, CombatEventKind::Death);
        assert_eq!(event.dest_name, "Horde Ranger");
    }

    #[test]
    fn ignores_untracked_event_types() {
        let line = r#"2/28 14:36:09.732  SPELL_CAST_SUCCESS,0x0E000000001AD363,"Luciernagaxd",0x512,0x0E00000000105932,"Aogus",0x512,27126,"Arcane Intellect",0x40"#;
        assert!(parse_line(line).is_none());
    }

    #[test]
    fn guid_classification() {
        assert!(is_player_guid("0x0E000000001B5352"));
        assert!(!is_npc_guid("0x0E000000001B5352"));
        assert!(is_npc_guid("0xF1300068B1000064"));
        assert!(!is_player_guid("0xF1300068B1000064"));
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
    fn parse_timestamp_relative_to_uses_the_guessed_year() {
        let reference = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let ms = parse_timestamp_relative_to("12/31 23:59:59.500", reference).unwrap();
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
        let ms = parse_timestamp_relative_to("7/28 14:36:59.000", reference).unwrap();
        let expected_utc = NaiveDate::from_ymd_opt(2026, 7, 28)
            .unwrap()
            .and_hms_opt(12, 36, 59)
            .unwrap()
            .and_utc();
        assert_eq!(ms, expected_utc.timestamp_millis());
    }
}
