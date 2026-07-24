use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::guild_sync::{extract_json_string, find_saved_variables_file};

/// One guild member as reported by GetGuildRosterInfo() via the separate
/// GuildExport addon — visible for the WHOLE guild from any single
/// member's client, unlike SoTGuildSync's own-character-only data.
/// Deliberately a SEPARATE addon rather than folded into SoTGuildSync:
/// every guild member runs SoTGuildSync, but a full-roster sync touches
/// every OTHER member's rank/level/class in one batch, so only whoever
/// installs GuildExport (an officer, in practice) can submit it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildRosterMember {
    pub name: String,
    pub class: Option<String>,
    pub level: Option<i64>,
    pub guild_rank: Option<String>,
    pub guild_rank_index: Option<i64>,
    pub zone: Option<String>,
    pub public_note: Option<String>,
    pub officer_note: Option<String>,
    pub is_online: Option<bool>,
    /// The only source of this for a character that never runs
    /// SoTGuildSync itself.
    pub achievement_points: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildRosterData {
    pub guild_name: Option<String>,
    pub realm: Option<String>,
    pub updated_at: i64,
    #[serde(default)]
    pub members: Vec<GuildRosterMember>,
}

/// Reads the GuildExport addon's current full-roster snapshot, if any.
/// `None` covers both "no game folder" and "nobody's run the addon yet" —
/// neither is an error, just "no roster data available right now".
pub fn read_guild_roster(game_path: &Path) -> Result<Option<GuildRosterData>, String> {
    let Some(path) = find_saved_variables_file(game_path, "GuildExport.lua") else {
        return Ok(None);
    };

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let Some(json) = extract_json_string(&raw, "json") else {
        return Ok(None);
    };

    let data: GuildRosterData = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse guild roster data: {e}"))?;

    Ok(Some(data))
}
