use crate::types::GuildSyncData;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A player can have multiple WoW accounts under one install, each with its
/// own `WTF/Account/<NAME>/SavedVariables/` folder — there's no central
/// "current account" record anywhere on disk, so this scans every account
/// folder for the given addon SavedVariables file and returns whichever was
/// written most recently (i.e. whichever account the player actually just
/// played). Shared by every addon this launcher reads SavedVariables from.
pub fn find_saved_variables_file(game_path: &Path, file_name: &str) -> Option<PathBuf> {
    let accounts_dir = game_path.join("WTF").join("Account");
    let entries = fs::read_dir(&accounts_dir).ok()?;

    let mut newest: Option<(PathBuf, SystemTime)> = None;
    for entry in entries.flatten() {
        let candidate = entry.path().join("SavedVariables").join(file_name);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let is_newer = match &newest {
            Some((_, best)) => modified > *best,
            None => true,
        };
        if is_newer {
            newest = Some((candidate, modified));
        }
    }

    newest.map(|(path, _)| path)
}

/// Pulls a single `["<field>"] = "..."` string out of an addon's
/// SavedVariables file and un-escapes it. This is not a general Lua
/// parser — it only understands the one shape every field these addons
/// write uses (`SavedVarsTable = { ["field"] = "<escaped JSON>" }`), which
/// is deliberately simple enough not to need one. SoTGuildSync writes both
/// `json` (per-character data) and `rosterJson` (the whole guild roster)
/// into the same file under different field names.
pub fn extract_json_string(raw: &str, field: &str) -> Option<String> {
    let marker = format!("[\"{field}\"] = \"");
    let start = raw.find(&marker)? + marker.len();
    let rest = &raw[start..];

    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => break,
            },
            _ => out.push(c),
        }
    }
    None
}

/// Reads and parses every guild character the addon currently knows about
/// for the given WoW install — one entry per alt that's been logged into
/// while a member of the guild, keyed by name in the addon's own
/// SavedVariables so logging into one doesn't drop another's data. Returns
/// an empty list if the addon hasn't written anything yet, rather than an
/// error — that's the normal "never logged in as a guild member" case.
pub fn read_guild_sync_data(game_path: &Path) -> Result<Vec<GuildSyncData>, String> {
    let Some(path) = find_saved_variables_file(game_path, "SoTGuildSync.lua") else {
        return Ok(Vec::new());
    };

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json = extract_json_string(&raw, "json")
        .ok_or_else(|| "could not find guild sync data in SavedVariables file".to_string())?;

    // The addon's own JSON encoder can't tell an empty map from an empty
    // array (Lua has no distinction), so it always writes an empty
    // `characters` table as `[]` — treat that one specific case as zero
    // characters rather than a real map-shape parse error.
    if json.trim() == "[]" {
        return Ok(Vec::new());
    }

    // Parsed generically first, then each character converted individually
    // — the addon's own data shape evolves over time (e.g. equipment went
    // from a flat item-ID array to a richer per-item object), and a single
    // character whose entry predates a shape change (hasn't logged back in
    // to get rewritten yet) must never take every other character down
    // with it.
    let by_name: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse guild sync data: {e}"))?;

    let mut characters: Vec<GuildSyncData> = Vec::new();
    for (name, value) in by_name {
        match serde_json::from_value::<GuildSyncData>(value) {
            Ok(data) => characters.push(data),
            Err(e) => eprintln!("skipping guild sync entry for '{name}': {e}"),
        }
    }
    characters.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(characters)
}
