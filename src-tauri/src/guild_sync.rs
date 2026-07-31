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
///
/// Both the account list and each candidate file go through virtualstore.rs
/// - on Windows, an account installed under Program Files and run without
/// admin rights has its actual writes silently redirected by UAC to a
/// hidden per-user shadow copy instead of showing up at the path below, so
/// resolving only the literal configured path would see stale or missing
/// data despite the game genuinely having saved something moments ago.
pub fn find_saved_variables_file(game_path: &Path, file_name: &str) -> Option<PathBuf> {
    let accounts_dir = game_path.join("WTF").join("Account");

    let mut newest: Option<(PathBuf, SystemTime)> = None;
    for account_dir in crate::virtualstore::list_dirs_merged(&accounts_dir) {
        let candidate = crate::virtualstore::resolve_read_path(&account_dir.join("SavedVariables").join(file_name));
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
/// is deliberately simple enough not to need one.
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

/// Every `WTF/Account/<NAME>/SavedVariables/<file_name>` that actually
/// exists, paired with the account name it belongs to — unlike
/// `find_saved_variables_file` (which picks a single "most recently
/// written" winner, appropriate for guild-wide data like SoTGuildSync),
/// this is for SoTCharacterSync specifically, where each WoW account's
/// SavedVariables only ever lists the characters logged into *on that
/// account*, so every account's file has to be read, not just the newest.
fn all_saved_variables_files(game_path: &Path, file_name: &str) -> Vec<(String, PathBuf)> {
    let accounts_dir = game_path.join("WTF").join("Account");
    let mut out = Vec::new();
    for account_dir in crate::virtualstore::list_dirs_merged(&accounts_dir) {
        let Some(account) = account_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let candidate = crate::virtualstore::resolve_read_path(&account_dir.join("SavedVariables").join(file_name));
        if candidate.is_file() {
            out.push((account.to_string(), candidate));
        }
    }
    out
}

/// Reads and parses every guild character the addon currently knows about
/// across EVERY WoW account under this install — one entry per alt that's
/// been logged into while a member of the guild, keyed by name within each
/// account's own SavedVariables so logging into one doesn't drop another's
/// data on the same account. A player with several WoW accounts (each its
/// own WTF/Account/<name> folder) gets characters from all of them, not
/// just whichever account was played most recently — each is tagged with
/// the account it came from (see GuildSyncData::account). Returns an empty
/// list if no account has ever written anything, rather than an error —
/// that's the normal "never logged in as a guild member" case.
pub fn read_guild_sync_data(game_path: &Path) -> Result<Vec<GuildSyncData>, String> {
    let mut characters: Vec<GuildSyncData> = Vec::new();

    for (account, path) in all_saved_variables_files(game_path, "SoTCharacterSync.lua") {
        let Ok(raw) = fs::read_to_string(&path) else {
            // One account's file being unreadable (permissions, mid-write
            // race) shouldn't take every other account's characters down
            // with it.
            continue;
        };
        // The file can exist (SoTCharacterSyncDB = SoTCharacterSyncDB or {}
        // always runs on load) without `json` ever having been written yet
        // — e.g. guild membership was never confirmed true during this
        // session on this account. That's the same "nothing to report yet"
        // case as the file not existing at all, not a real parse error.
        let Some(json) = extract_json_string(&raw, "json") else {
            continue;
        };

        // The addon's own JSON encoder can't tell an empty map from an empty
        // array (Lua has no distinction), so it always writes an empty
        // `characters` table as `[]` — treat that one specific case as zero
        // characters rather than a real map-shape parse error.
        if json.trim() == "[]" {
            continue;
        }

        // Parsed generically first, then each character converted
        // individually — the addon's own data shape evolves over time (e.g.
        // equipment went from a flat item-ID array to a richer per-item
        // object), and a single character whose entry predates a shape
        // change (hasn't logged back in to get rewritten yet) must never
        // take every other character down with it. Likewise, a parse
        // failure of one account's JSON blob (e.g. caught mid-write) must
        // not drop every other account's already-good data.
        let Ok(by_name) = serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&json) else {
            eprintln!("skipping guild sync data for account '{account}': failed to parse JSON");
            continue;
        };

        for (name, value) in by_name {
            match serde_json::from_value::<GuildSyncData>(value) {
                Ok(mut data) => {
                    data.account = account.clone();
                    characters.push(data);
                }
                Err(e) => eprintln!("skipping guild sync entry for '{name}' (account '{account}'): {e}"),
            }
        }
    }

    characters.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(characters)
}
