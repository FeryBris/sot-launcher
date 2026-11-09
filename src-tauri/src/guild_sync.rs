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

/// Every character entry in every account's SoTCharacterSync.lua, as raw
/// JSON, paired with the account it came from. Shared by the WotLK reader
/// (which converts each to GuildSyncData) and the Forever one (which keeps
/// the addon's own JSON - see read_forever_characters).
fn read_raw_characters(game_path: &Path) -> Vec<(String, String, serde_json::Value)> {
    let mut out = Vec::new();

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

        // A parse failure of one account's JSON blob (e.g. caught
        // mid-write) must not drop every other account's already-good data.
        let Ok(by_name) = serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&json) else {
            eprintln!("skipping guild sync data for account '{account}': failed to parse JSON");
            continue;
        };

        for (name, value) in by_name {
            out.push((account.clone(), name, value));
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

    // Each character converted individually — the addon's own data shape
    // evolves over time (e.g. equipment went from a flat item-ID array to a
    // richer per-item object), and a single character whose entry predates
    // a shape change (hasn't logged back in to get rewritten yet) must never
    // take every other character down with it.
    for (account, name, value) in read_raw_characters(game_path) {
        match serde_json::from_value::<GuildSyncData>(value) {
            Ok(mut data) => {
                data.account = account;
                characters.push(data);
            }
            Err(e) => eprintln!("skipping guild sync entry for '{name}' (account '{account}'): {e}"),
        }
    }

    characters.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(characters)
}

/// The Forever SoTCharacterSync export, as the addon wrote it: the website
/// accepts that JSON directly (its Forever fields - Legacy, honor rank,
/// collections, talents as traits... - have no WotLK equivalent in
/// GuildSyncData). Each entry gets `account` added for the launcher's own
/// UI; the upload strips it again. Entries without a name are skipped.
pub fn read_forever_characters(game_path: &Path) -> Vec<serde_json::Value> {
    let mut characters: Vec<serde_json::Value> = read_raw_characters(game_path)
        .into_iter()
        .filter_map(|(account, _, mut value)| {
            let obj = value.as_object_mut()?;
            obj.get("name")?.as_str()?;
            obj.insert("account".to_string(), serde_json::Value::String(account));
            Some(value)
        })
        .collect();
    characters.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    characters
}

#[cfg(test)]
mod forever_tests {
    use super::*;

    /// A game folder with one account whose SoTCharacterSync.lua holds `json`.
    fn game_with_character_json(name: &str, json: &str) -> PathBuf {
        let game = std::env::temp_dir().join(format!("sot-forever-chars-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&game);
        let sv = game.join("WTF/Account/123#4/SavedVariables");
        fs::create_dir_all(&sv).unwrap();
        let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
        fs::write(
            sv.join("SoTCharacterSync.lua"),
            format!("SoTCharacterSyncDB = {{\n[\"json\"] = \"{escaped}\",\n}}\n"),
        )
        .unwrap();
        game
    }

    #[test]
    fn forever_characters_keep_the_addons_own_fields_and_gain_the_account() {
        let game = game_with_character_json(
            "keep",
            r#"{"Blue Black":{"name":"Blue Black","class":"WARRIOR","race":"Troll","level":25,
               "client":"forever","legacy":{"trackLevel":3},"talents":{"format":"trait"}},
               "Broken":{"level":1}}"#,
        );
        let characters = read_forever_characters(&game);
        fs::remove_dir_all(&game).unwrap();

        // The entry without a name is skipped; the other is kept whole.
        assert_eq!(characters.len(), 1);
        let c = &characters[0];
        assert_eq!(c["account"], "123#4");
        assert_eq!(c["legacy"]["trackLevel"], 3);
        assert_eq!(c["talents"]["format"], "trait");
        // The WotLK reader can't parse Forever's shape at all.
        let game = game_with_character_json("wotlk", r#"{"Blue Black":{"name":"Blue Black","talents":{"format":"trait"}}}"#);
        assert!(read_guild_sync_data(&game).unwrap().is_empty());
        fs::remove_dir_all(&game).unwrap();
    }
}
