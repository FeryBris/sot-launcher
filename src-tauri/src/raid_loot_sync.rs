use crate::guild_sync::{extract_json_string, find_saved_variables_file};
use serde::Serialize;
use std::fs;
use std::path::Path;

/// One raid the SoTRaidLootSync addon has captured (see that addon's
/// own age-based capture window — this is never the addon's full
/// history, just recent raids). `loot_json` is None for a raid night
/// with no RCLootCouncil data at all, which is a normal, common case
/// (see RaidSyncController on the website side for why that's still
/// safe to sync).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedRaid {
    pub raid_id: String,
    pub captured_at: i64,
    pub raid_json: String,
    pub loot_json: Option<String>,
}

/// Given text starting right after an opening `{` (i.e. already at depth
/// 1), finds the matching closing `}` and returns everything before it.
/// String-aware: raidJson/lootJson are themselves JSON text embedded as
/// Lua string values, so the raw file is full of literal `{`/`}`
/// characters that belong to an embedded string, not real Lua table
/// nesting — those must be skipped rather than counted.
fn find_balanced_block(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut depth: i32 = 1;
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' {
                i += 1; // skip whatever's escaped, don't inspect it
            } else if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[..i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    None
}

/// Finds the `{ ... }` table value assigned to `["key"]` and returns its
/// inner text (contents between the outer braces).
fn extract_table_block<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("[\"{key}\"] = {{");
    let start = raw.find(&marker)? + marker.len();
    find_balanced_block(&raw[start..])
}

/// Pulls a bare (unquoted) Lua number out of `["field"] = 123`.
fn extract_number(raw: &str, field: &str) -> Option<i64> {
    let marker = format!("[\"{field}\"] = ");
    let start = raw.find(&marker)? + marker.len();
    let rest = &raw[start..];
    let end = rest.find(|c: char| c == ',' || c == '\n' || c == '}')?;
    rest[..end].trim().parse::<i64>().ok()
}

/// Splits the raids block into each top-level `["<raidId>"] = { ... }`
/// entry — raidId is always a bare numeric string (a Unix timestamp, see
/// the addon), which is what distinguishes it from the `raidJson`/
/// `lootJson`/`capturedAt` keys inside each entry.
fn extract_raid_entries(raids_block: &str) -> Vec<(String, String)> {
    let bytes = raids_block.as_bytes();
    let mut entries = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'[' && bytes.get(i + 1) == Some(&b'"') {
            let key_start = i + 2;
            let mut key_end = key_start;
            while bytes.get(key_end).is_some_and(u8::is_ascii_digit) {
                key_end += 1;
            }

            let is_numeric_key = key_end > key_start;
            let rest = &raids_block[key_end..];

            if is_numeric_key && rest.starts_with("\"] = {") {
                let raid_id = raids_block[key_start..key_end].to_string();
                let block_start = key_end + "\"] = {".len();

                if let Some(inner) = find_balanced_block(&raids_block[block_start..]) {
                    entries.push((raid_id, inner.to_string()));
                    i = block_start + inner.len();
                    continue;
                }
            }
        }
        i += 1;
    }

    entries
}

/// Reads every raid the SoTRaidLootSync addon has captured for this WoW
/// install. Empty (not an error) covers both "addon not installed" and
/// "nothing captured yet" — neither is a failure state for the caller,
/// which just wants to know what's available to sync.
pub fn read_captured_raids(game_path: &Path) -> Result<Vec<CapturedRaid>, String> {
    let Some(path) = find_saved_variables_file(game_path, "SoTRaidLootSync.lua") else {
        return Ok(Vec::new());
    };

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;

    let Some(raids_block) = extract_table_block(&raw, "raids") else {
        return Ok(Vec::new());
    };

    let mut raids = Vec::new();
    for (raid_id, entry_block) in extract_raid_entries(raids_block) {
        let Some(raid_json) = extract_json_string(entry_block.as_str(), "raidJson") else {
            continue; // no usable data without this — skip rather than fail the whole read
        };
        let loot_json = extract_json_string(entry_block.as_str(), "lootJson");
        let captured_at = extract_number(entry_block.as_str(), "capturedAt").unwrap_or(0);

        raids.push(CapturedRaid {
            raid_id,
            captured_at,
            raid_json,
            loot_json,
        });
    }

    raids.sort_by(|a, b| b.raid_id.cmp(&a.raid_id)); // newest first
    Ok(raids)
}
