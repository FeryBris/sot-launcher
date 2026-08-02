// Heuristic dungeon/raid/world classification, ported from
// wowlogs/zone_detector.py. 3.3.5a's combat log carries no zone/instance
// data at all (no ENCOUNTER_START/ZONE_CHANGE-style events exist in this
// client version), so group size + NPC presence + event volume are the
// only signals available — this is an approximation, same as the Python
// original, not a reliable instance/boss identification.

use crate::combat_log_parser::{is_npc_guid, CombatEventKind};
use crate::combat_log_session::{PlayerReport, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentType {
    Unknown,
    Dungeon,
    Raid,
    World,
}

/// A minimal view of one recorded event — only what the classifier needs,
/// so it doesn't have to depend on the full CombatEvent/session module
/// beyond this.
pub struct ClassifierEvent<'a> {
    pub kind: CombatEventKind,
    pub dest_guid: &'a str,
}

pub struct ContentAnalysis {
    pub content_type: ContentType,
    pub quality_score: f64,
}

/// Scores a completed encounter's events + finalized player stats against
/// the same heuristics wowlogs uses: NPC hit volume, group size buckets
/// (5 = dungeon, 10/25/40 = raid), and event count as a rough proxy for
/// "sustained, structured combat" vs. a stray swing or two.
pub fn classify(events: &[ClassifierEvent], players: &[PlayerReport]) -> ContentAnalysis {
    if events.is_empty() || players.is_empty() {
        return ContentAnalysis {
            content_type: ContentType::Unknown,
            quality_score: 0.0,
        };
    }

    let mut score = 0.0;

    let npc_hits = events
        .iter()
        .filter(|e| e.kind == CombatEventKind::Damage && is_npc_guid(e.dest_guid))
        .count();
    if npc_hits > 5 {
        score += 0.3;
    }

    let player_count = players.len();
    match player_count {
        4..=6 => score += 0.4,
        9..=11 => score += 0.5,
        24..=26 => score += 0.5,
        n if n >= 40 => score += 0.4,
        0..=3 => score -= 0.3,
        _ => {}
    }

    let tanks = players.iter().filter(|p| p.role == Role::Tank).count();
    let healers = players.iter().filter(|p| p.role == Role::Healer).count();
    let dps = players.iter().filter(|p| p.role == Role::Dps).count();
    if tanks > 0 && healers > 0 && dps >= 2 {
        score += 0.2;
    }

    let event_count = events.len();
    if event_count > 100 {
        score += 0.1;
    } else if event_count < 20 {
        score -= 0.1;
    }

    let content_type = if score >= 0.6 {
        // wowlogs treats 0.6 and 0.3 as two separate "dungeon" bands with no
        // distinct raid-only band below 0.8 in practice (its own
        // _classify_content falls through 0.8/0.6/0.3 into DUNGEON twice) —
        // kept as one >=0.6 => raid-or-dungeon-favoring-raid-at-the-top
        // band here since group size above already separates 5-mans from
        // 10/25/40-mans via the score bonus itself.
        if player_count >= 9 {
            ContentType::Raid
        } else {
            ContentType::Dungeon
        }
    } else if score >= 0.3 {
        ContentType::Dungeon
    } else {
        ContentType::World
    };

    ContentAnalysis {
        content_type,
        quality_score: score,
    }
}
