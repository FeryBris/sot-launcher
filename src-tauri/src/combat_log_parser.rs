// Extracts a WoW 3.3.5a combat log line's own timestamp - the only piece
// of a raw line combat_log_chunker actually needs (see its own module doc
// comment for why: full event parsing/classification into damage/heal/
// death/player-vs-NPC now happens entirely server-side, from the raw chunk
// text, once uploaded).
//
// This used to be a much bigger module - full per-line event parsing
// (SWING_DAMAGE/SPELL_DAMAGE/SPELL_HEAL/UNIT_DIED/SPELL_SUMMON field
// layouts, COMBATLOG_OBJECT_TYPE_* flag classification, GUID entry-ID
// extraction) backing a client-side aggregator (combat_log_session.rs) that
// computed live DPS/role/content-type per pull. That whole pipeline was
// replaced by raw-chunk-and-upload (see combat_log_watcher.rs's own module
// doc comment) - the real parsing/classification logic it contained (with
// real bug-fix history: SWING_DAMAGE's amount/critical field offsets,
// Bratislava-not-UTC timestamps) still exists in git history if a future
// client-side feature ever needs it again, rather than kept here unused.

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Bratislava;

/// "Today" in the log's own Bratislava timezone - the log's own
/// timestamps carry no year (see parse_timestamp), so resolving one needs
/// today's real date in the same timezone the log itself is written in
/// (this guild runs on Slovak time, regardless of which timezone the
/// machine parsing it happens to be in). Computed once per batch by the
/// caller (combat_log_watcher's process_lines) and threaded through rather
/// than recomputed per line: a real timezone-aware `Utc::now()` call on
/// every one of a raid log's hundreds of thousands of lines is pure waste
/// when a whole batch is processed within milliseconds of each other.
pub fn today_reference() -> NaiveDate {
    Utc::now().with_timezone(&Bratislava).date_naive()
}

/// This line's own timestamp, parsed from its leading "M/D HH:MM:SS.mmm  "
/// prefix - every line has one, regardless of whether it's an event type
/// anything downstream specifically recognizes (aura/cast/buff lines
/// included). `reference` is "today" for resolving the year-less date -
/// see today_reference's own doc comment.
pub fn parse_line_timestamp(line: &str, reference: NaiveDate) -> Option<i64> {
    let (timestamp_str, _rest) = line.split_once("  ")?;
    parse_timestamp(timestamp_str.trim(), reference)
}

/// This log format has no year - guessed from `reference` (real "today")
/// under the assumption the log is recent: a log month AFTER today's own
/// month must be from last year (e.g. today is January, log line is month
/// 12 - that's last December, not 11 months in the future), never further
/// back than that. Wrong for a log file spanning more than one such
/// rollover (i.e. genuinely over a year old).
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
    fn parse_line_timestamp_extracts_from_a_real_damage_line() {
        let line = r#"2/28 14:36:59.336  SWING_DAMAGE,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,342,0,1,0,0,0,nil,nil,nil"#;
        assert!(parse_line_timestamp(line, today_reference()).is_some());
    }

    #[test]
    fn parse_line_timestamp_extracts_from_an_event_type_nothing_else_would_recognize() {
        // The whole point of this being timestamp-only, not full event
        // parsing: an aura line matters to chunk boundaries even though no
        // downstream consumer needs its full structure anymore.
        let line = r#"2/28 14:36:59.500  SPELL_AURA_APPLIED,0x0E000000001B5352,"Pzkpfwagen",0x512,0xF1300068B1000064,"Horde Ranger",0xa48,12345,"Some Debuff",0x1,DEBUFF"#;
        assert_eq!(
            parse_line_timestamp(line, today_reference()),
            parse_timestamp("2/28 14:36:59.500", today_reference()),
        );
    }

    #[test]
    fn parse_line_timestamp_is_none_for_a_line_with_no_timestamp_prefix() {
        assert!(parse_line_timestamp("not a combat log line", today_reference()).is_none());
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
