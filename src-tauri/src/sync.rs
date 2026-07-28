use crate::types::{AddonEntry, AddonStateEntry, AddonsStateFile, InstalledState, Manifest, SyncProgress, SyncResult, WtfEntry};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter};

const EVENT: &str = "sync-progress";
const STATE_FILENAME: &str = ".sotlauncher.json";
const DISABLED_DIR_NAME: &str = ".DisabledAddons";
const DELETED_DIR_NAME: &str = ".DeletedAddons";

fn addon_dir_for(game_path: &str) -> PathBuf {
    PathBuf::from(game_path).join("Interface").join("AddOns")
}

fn disabled_dir_for(game_path: &str) -> PathBuf {
    PathBuf::from(game_path).join("Interface").join(DISABLED_DIR_NAME)
}

fn deleted_dir_for(game_path: &str) -> PathBuf {
    PathBuf::from(game_path).join("Interface").join(DELETED_DIR_NAME)
}

/// Which of the three folders an entry's files actually live in right now.
fn location_dir_for<'a>(
    entry: &AddonStateEntry,
    addon_dir: &'a Path,
    disabled_dir: &'a Path,
    deleted_dir: &'a Path,
) -> &'a Path {
    if entry.deleted {
        deleted_dir
    } else if entry.disabled {
        disabled_dir
    } else {
        addon_dir
    }
}

fn emit_progress(
    app: &AppHandle,
    item: &str,
    stage: &str,
    current: usize,
    total: usize,
    message: Option<String>,
) {
    let _ = app.emit(
        EVENT,
        SyncProgress {
            item: item.to_string(),
            stage: stage.to_string(),
            current,
            total,
            message,
        },
    );
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Joins `rel` onto `base`, rejecting absolute paths and `..` traversal so a
/// malicious or malformed manifest entry can never write outside the game folder.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    let mut result = base.to_path_buf();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(c) => result.push(c),
            Component::CurDir => {}
            _ => return Err(format!("unsafe path in manifest: {rel}")),
        }
    }
    Ok(result)
}

// No timeout at all (reqwest::get()'s implicit default) lets a stalled
// connection hang forever instead of erroring — observed on Windows, where
// a download would leave the sync stuck on "downloading" with no way to
// know anything's wrong. A generous but finite timeout turns that into a
// clear error.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

async fn download_to_file(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while downloading {}", resp.status(), url));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Extracts the zip into `dest_dir` and returns the top-level file/folder
/// names it wrote (the first path component of each entry), so a later
/// removal can precisely delete only what this addon created.
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut top_level: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name() refuses entries that try to escape dest_dir (zip-slip protection)
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        if let Some(Component::Normal(first)) = name.components().next() {
            if let Some(s) = first.to_str() {
                top_level.insert(s.to_string());
            }
        }

        let outpath = dest_dir.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
        }
    }
    Ok(top_level.into_iter().collect())
}

/// Peeks a zip's top-level entry names without writing anything, so a
/// caller can check for folder collisions before committing to extract —
/// see its use in sync_addons for why that ordering matters.
fn list_zip_top_level(zip_path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut top_level: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        if let Some(Component::Normal(first)) = name.components().next() {
            if let Some(s) = first.to_str() {
                top_level.insert(s.to_string());
            }
        }
    }
    Ok(top_level.into_iter().collect())
}

/// Deletes only the top-level entries recorded for an addon, from wherever it lives.
fn remove_addon_files(base_dir: &Path, files: &[String]) {
    for name in files {
        let path = base_dir.join(name);
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Moves only the top-level entries recorded for an addon from one base
/// directory to another (AddOns <-> .DisabledAddons), used for disable/enable.
fn move_addon_files(from_dir: &Path, to_dir: &Path, files: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(to_dir).map_err(|e| e.to_string())?;
    for name in files {
        let from = from_dir.join(name);
        let to = to_dir.join(name);
        if !from.exists() {
            continue;
        }
        if to.exists() {
            if to.is_dir() {
                let _ = std::fs::remove_dir_all(&to);
            } else {
                let _ = std::fs::remove_file(&to);
            }
        }
        std::fs::rename(&from, &to).map_err(|e| format!("failed to move {name}: {e}"))?;
    }
    Ok(())
}

fn state_file_path(addon_dir: &Path) -> PathBuf {
    addon_dir.join(STATE_FILENAME)
}

/// Loads `Interface/AddOns/.sotlauncher.json` — the single file recording
/// every addon the launcher knows about, managed or custom.
fn load_addons_state(addon_dir: &Path) -> Vec<AddonStateEntry> {
    let Ok(raw) = std::fs::read_to_string(state_file_path(addon_dir)) else {
        return Vec::new();
    };
    serde_json::from_str::<AddonsStateFile>(&raw).map(|f| f.addons).unwrap_or_default()
}

fn save_addons_state(addon_dir: &Path, entries: &[AddonStateEntry]) -> Result<(), String> {
    let file = AddonsStateFile { addons: entries.to_vec() };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(state_file_path(addon_dir), json).map_err(|e| e.to_string())
}

fn is_addon_wanted(addon: &AddonEntry, addon_enabled: &HashMap<String, bool>) -> bool {
    match addon.category.as_str() {
        "mandatory" => true,
        "recommended" => *addon_enabled.get(&addon.name).unwrap_or(&true),
        _ => *addon_enabled.get(&addon.name).unwrap_or(&false),
    }
}

/// Strips WoW's inline color escape codes (|cAARRGGBB...|r) so .toc titles
/// read as plain text outside the game.
fn strip_wow_color_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '|' {
            match chars.peek() {
                Some('c') | Some('C') => {
                    chars.next();
                    for _ in 0..8 {
                        if chars.peek().is_some() {
                            chars.next();
                        }
                    }
                    continue;
                }
                Some('r') | Some('R') => {
                    chars.next();
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Reads `## Title:`, `## Version:`, `## Notes:`, and hard dependencies
/// (`## Dependencies:` / the older `## RequiredDeps:` alias — both mean the
/// same thing to WoW's addon loader) out of a .toc file's header comments.
/// `## OptionalDeps:` is deliberately not read here: those don't break
/// anything if missing, so they're not part of the "will this break X" check.
fn parse_toc(raw: &str) -> (Option<String>, Option<String>, Option<String>, Vec<String>) {
    let mut title = None;
    let mut version = None;
    let mut notes = None;
    let mut dependencies = Vec::new();
    for line in raw.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let Some(rest) = line.strip_prefix("##") else { continue };
        let rest = rest.trim();
        if let Some(v) = rest.strip_prefix("Title:") {
            title = Some(strip_wow_color_codes(v.trim()));
        } else if let Some(v) = rest.strip_prefix("Version:") {
            version = Some(v.trim().to_string());
        } else if let Some(v) = rest.strip_prefix("Notes:") {
            notes = Some(strip_wow_color_codes(v.trim()));
        } else if let Some(v) = rest.strip_prefix("Dependencies:").or_else(|| rest.strip_prefix("RequiredDeps:")) {
            dependencies.extend(v.split(',').map(str::trim).filter(|d| !d.is_empty()).map(str::to_string));
        }
    }
    (title, version, notes, dependencies)
}

/// Finds the .toc file that describes `dir` (preferring one matching the
/// folder name, since a folder can carry more than one for different WoW
/// client versions), falling back to whichever .toc is there.
fn find_toc_file(dir: &Path, folder_name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("toc")) != Some(true) {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.eq_ignore_ascii_case(folder_name) {
                return Some(path);
            }
        }
        fallback.get_or_insert(path);
    }
    fallback
}

/// Reads the live `Version:`/`Notes:`/`Dependencies:` out of whichever of an
/// addon's owned folders (under `base_dir`) has a matching .toc, so displayed
/// info always reflects what's actually on disk rather than what was last
/// recorded. Returns None only if no .toc could be found at all.
fn read_live_toc(base_dir: &Path, files: &[String]) -> Option<(Option<String>, Option<String>, Vec<String>)> {
    for f in files {
        let dir = base_dir.join(f);
        if !dir.is_dir() {
            continue;
        }
        if let Some(toc_path) = find_toc_file(&dir, f) {
            if let Ok(raw) = std::fs::read_to_string(&toc_path) {
                let (_, version, notes, dependencies) = parse_toc(&raw);
                return Some((version, notes, dependencies));
            }
        }
    }
    None
}

/// Lists every AddOns folder not claimed by any entry in `claimed_folders`,
/// reading its .toc for a human-readable title/version/notes.
fn scan_custom_addons(addon_dir: &Path, claimed_folders: &HashSet<String>) -> Vec<AddonStateEntry> {
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(addon_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(folder) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        if claimed_folders.contains(&folder) {
            continue;
        }
        let Some(toc_path) = find_toc_file(&path, &folder) else {
            continue;
        };
        let Ok(raw) = std::fs::read_to_string(&toc_path) else {
            continue;
        };
        let (title, version, notes, dependencies) = parse_toc(&raw);
        result.push(AddonStateEntry {
            name: title.unwrap_or_else(|| folder.clone()),
            kind: "custom".to_string(),
            version: version.unwrap_or_else(|| "unknown".to_string()),
            api_version: None,
            sha256: None,
            notes: notes.unwrap_or_default(),
            files: vec![folder],
            files_present: true,
            disabled: false,
            deleted: false,
            dependencies,
        });
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// Computes current state for everything in `manifest` by reading
/// `Interface/AddOns/.sotlauncher.json`, re-checking file presence in
/// whichever of AddOns/.DisabledAddons/.DeletedAddons each entry currently
/// lives in, re-reading each addon's live .toc info, refreshing
/// type/apiVersion/sha256 against the current manifest, discovering any new
/// custom addons, and rewriting the file with the result.
pub fn get_installed_state(game_path: &str, manifest: &Manifest) -> InstalledState {
    let addon_dir = addon_dir_for(game_path);
    let disabled_dir = disabled_dir_for(game_path);
    let deleted_dir = deleted_dir_for(game_path);
    let _ = std::fs::create_dir_all(&addon_dir);

    let previous = load_addons_state(&addon_dir);
    let manifest_by_name: HashMap<&str, &AddonEntry> = manifest.addons.iter().map(|a| (a.name.as_str(), a)).collect();

    let mut claimed_folders: HashSet<String> = HashSet::new();
    let mut refreshed: Vec<AddonStateEntry> = Vec::new();

    for entry in previous.into_iter() {
        // Deleted entries' folders don't block a fresh custom-addon scan or
        // a re-download of the same name — only active/disabled ones do.
        if !entry.deleted {
            for f in &entry.files {
                claimed_folders.insert(f.clone());
            }
        }
        let base = location_dir_for(&entry, &addon_dir, &disabled_dir, &deleted_dir);
        let files_present = !entry.files.is_empty() && entry.files.iter().all(|f| base.join(f).exists());
        let live = if files_present { read_live_toc(base, &entry.files) } else { None };
        let (live_version, live_notes, live_deps) = match live {
            Some((v, n, d)) => (v, n, Some(d)),
            None => (None, None, None),
        };

        let (kind, api_version, sha256) = if entry.kind == "custom" {
            (entry.kind, entry.api_version, entry.sha256)
        } else {
            match manifest_by_name.get(entry.name.as_str()) {
                Some(a) => (a.category.clone(), Some(a.version.clone()), Some(a.sha256.clone())),
                // No longer in the manifest; keep last-known info until sync cleans it up.
                None => (entry.kind, entry.api_version, entry.sha256),
            }
        };

        refreshed.push(AddonStateEntry {
            name: entry.name,
            kind,
            version: live_version.unwrap_or(entry.version),
            api_version,
            sha256,
            notes: live_notes.unwrap_or(entry.notes),
            files: entry.files,
            files_present,
            disabled: entry.disabled,
            deleted: entry.deleted,
            dependencies: live_deps.unwrap_or(entry.dependencies),
        });
    }

    refreshed.extend(scan_custom_addons(&addon_dir, &claimed_folders));
    let _ = save_addons_state(&addon_dir, &refreshed);

    let addons_map: HashMap<String, AddonStateEntry> = refreshed.into_iter().map(|e| (e.name.clone(), e)).collect();

    let game_root = PathBuf::from(game_path);
    let mut wtf_files = HashMap::new();
    for entry in &manifest.wtf_files {
        if let Ok(path) = safe_join(&game_root, &entry.target_path) {
            if let Ok(hash) = sha256_file(&path) {
                wtf_files.insert(entry.name.clone(), hash);
            }
        }
    }

    InstalledState { addons: addons_map, wtf_files }
}

/// Moves a tracked addon (managed or custom) between Interface/AddOns and
/// Interface/.DisabledAddons. Disabling never deletes anything. Errors on a
/// deleted entry — restore it from Trash first.
pub fn set_addon_disabled(game_path: &str, name: &str, disabled: bool) -> Result<(), String> {
    let addon_dir = addon_dir_for(game_path);
    let disabled_dir = disabled_dir_for(game_path);
    let mut entries = load_addons_state(&addon_dir);
    let entry = entries
        .iter_mut()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("{name} is not tracked by the launcher"))?;

    if entry.deleted {
        return Err(format!("{name} is in Trash — restore it first"));
    }
    if entry.disabled == disabled {
        return Ok(());
    }
    let (from, to) = if disabled { (&addon_dir, &disabled_dir) } else { (&disabled_dir, &addon_dir) };
    move_addon_files(from, to, &entry.files)?;
    entry.disabled = disabled;
    save_addons_state(&addon_dir, &entries)
}

/// Soft-deletes a tracked addon (managed or custom): moves its files to
/// Interface/.DeletedAddons and marks the entry `deleted`, but keeps
/// tracking it (unlike the old permanent-delete behavior) so Trash can
/// later Restore it or Delete Forever. Returns the updated entry, if any,
/// so the caller can also clear its enabled/disabled preference for managed
/// addons. A no-op returning the entry unchanged if it's already deleted.
pub fn delete_addon(game_path: &str, name: &str) -> Result<Option<AddonStateEntry>, String> {
    let addon_dir = addon_dir_for(game_path);
    let disabled_dir = disabled_dir_for(game_path);
    let deleted_dir = deleted_dir_for(game_path);
    let mut entries = load_addons_state(&addon_dir);
    let Some(pos) = entries.iter().position(|e| e.name == name) else {
        return Ok(None);
    };

    if entries[pos].deleted {
        return Ok(Some(entries[pos].clone()));
    }

    let from = if entries[pos].disabled { &disabled_dir } else { &addon_dir };
    move_addon_files(from, &deleted_dir, &entries[pos].files)?;
    entries[pos].disabled = false;
    entries[pos].deleted = true;

    let result = entries[pos].clone();
    save_addons_state(&addon_dir, &entries)?;
    Ok(Some(result))
}

/// Permanently destroys a trashed addon's files and drops its tracking
/// entry entirely — the point of no return Trash exists to delay. Only
/// acts on an entry that's actually `deleted`; anything else is a no-op.
pub fn delete_addon_forever(game_path: &str, name: &str) -> Result<(), String> {
    let addon_dir = addon_dir_for(game_path);
    let deleted_dir = deleted_dir_for(game_path);
    let mut entries = load_addons_state(&addon_dir);
    let Some(pos) = entries.iter().position(|e| e.name == name && e.deleted) else {
        return Ok(());
    };
    let entry = entries.remove(pos);
    remove_addon_files(&deleted_dir, &entry.files);
    save_addons_state(&addon_dir, &entries)
}

/// Moves a trashed addon's files back to Interface/AddOns and clears
/// `deleted` (always restoring as enabled, not back into .DisabledAddons —
/// once something's been through Trash, "restore" means "make it work
/// again"). If a different, non-deleted entry currently owns one of its
/// folders (e.g. a managed addon re-synced into that name after this one
/// was trashed), two things can't occupy one folder: with `force: false`
/// this returns an error naming the occupant instead of restoring, so the
/// caller can confirm with the user; call again with `force: true` to
/// permanently delete that occupant first, then restore.
pub fn restore_addon(game_path: &str, name: &str, force: bool) -> Result<Option<AddonStateEntry>, String> {
    let addon_dir = addon_dir_for(game_path);
    let disabled_dir = disabled_dir_for(game_path);
    let deleted_dir = deleted_dir_for(game_path);
    let mut entries = load_addons_state(&addon_dir);
    let Some(pos) = entries.iter().position(|e| e.name == name && e.deleted) else {
        return Ok(None);
    };

    let restoring_files: HashSet<String> = entries[pos].files.iter().cloned().collect();
    let occupant_pos = entries
        .iter()
        .position(|e| !e.deleted && e.name != name && e.files.iter().any(|f| restoring_files.contains(f)));

    if let Some(occupant_pos) = occupant_pos {
        if !force {
            return Err(format!(
                "\"{}\" currently occupies that folder — restoring will permanently delete it",
                entries[occupant_pos].name
            ));
        }
        let occupant = entries.remove(occupant_pos);
        let occupant_base = if occupant.disabled { &disabled_dir } else { &addon_dir };
        remove_addon_files(occupant_base, &occupant.files);
    }

    // Removing the occupant above may have shifted every later index.
    let pos = entries.iter().position(|e| e.name == name && e.deleted).unwrap();

    move_addon_files(&deleted_dir, &addon_dir, &entries[pos].files)?;
    entries[pos].disabled = false;
    entries[pos].deleted = false;
    entries[pos].files_present = true;
    if let Some((live_version, live_notes, live_deps)) = read_live_toc(&addon_dir, &entries[pos].files) {
        if let Some(v) = live_version {
            entries[pos].version = v;
        }
        if let Some(n) = live_notes {
            entries[pos].notes = n;
        }
        entries[pos].dependencies = live_deps;
    }

    let result = entries[pos].clone();
    save_addons_state(&addon_dir, &entries)?;
    Ok(Some(result))
}

pub async fn sync_addons(
    app: &AppHandle,
    game_path: &str,
    addons: &[AddonEntry],
    addon_enabled: &HashMap<String, bool>,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], removed: vec![], failed: vec![] };
    let addon_dir = addon_dir_for(game_path);
    let disabled_dir = disabled_dir_for(game_path);
    let deleted_dir = deleted_dir_for(game_path);
    let _ = std::fs::create_dir_all(&addon_dir);

    let mut by_name: HashMap<String, AddonStateEntry> =
        load_addons_state(&addon_dir).into_iter().map(|e| (e.name.clone(), e)).collect();

    let total = addons.len();
    let manifest_names: HashSet<&str> = addons.iter().map(|a| a.name.as_str()).collect();

    for (idx, addon) in addons.iter().enumerate() {
        let wanted = is_addon_wanted(addon, addon_enabled);
        let prev = by_name.get(&addon.name).cloned();

        if !wanted {
            // Already sitting in Trash — nothing to disable, leave it alone.
            if let Some(prev) = prev {
                if !prev.deleted && !prev.disabled && move_addon_files(&addon_dir, &disabled_dir, &prev.files).is_ok() {
                    let mut updated = prev;
                    updated.disabled = true;
                    by_name.insert(addon.name.clone(), updated);
                    result.removed.push(addon.name.clone());
                    emit_progress(app, &addon.name, "done", idx + 1, total, Some("disabled".to_string()));
                }
            }
            continue;
        }

        // Re-enabling something that was disabled: if the manifest hasn't
        // moved on since it was disabled, just move it back - no re-download.
        // A trashed entry (deleted) is deliberately NOT handled here — it
        // falls through to the fresh-download path below instead, same as
        // if it had never been tracked at all, leaving the actual trashed
        // copy in .DeletedAddons alone (still restorable/purgeable from
        // Trash, just no longer the entry this name's map key points at).
        if let Some(prev) = &prev {
            if prev.disabled && !prev.deleted {
                if prev.sha256.as_deref() == Some(addon.sha256.as_str())
                    && move_addon_files(&disabled_dir, &addon_dir, &prev.files).is_ok()
                {
                    let live = read_live_toc(&addon_dir, &prev.files);
                    let mut updated = prev.clone();
                    updated.disabled = false;
                    updated.files_present = true;
                    if let Some((live_version, live_notes, live_deps)) = live {
                        updated.version = live_version.unwrap_or(updated.version);
                        updated.notes = live_notes.unwrap_or(updated.notes);
                        updated.dependencies = live_deps;
                    }
                    by_name.insert(addon.name.clone(), updated);
                    result.updated.push(addon.name.clone());
                    emit_progress(app, &addon.name, "done", idx + 1, total, Some("enabled".to_string()));
                    continue;
                }
                // Stale while disabled, or the move failed: discard the old
                // disabled copy and fall through to a fresh download below.
                remove_addon_files(&disabled_dir, &prev.files);
            }
        }

        let up_to_date = prev.as_ref().is_some_and(|e| {
            !e.disabled
                && !e.deleted
                && !e.files.is_empty()
                && e.files.iter().all(|f| addon_dir.join(f).exists())
                && e.sha256.as_deref().map(|h| h.eq_ignore_ascii_case(&addon.sha256)).unwrap_or(false)
        });
        if up_to_date {
            result.skipped.push(addon.name.clone());
            continue;
        }

        // Clean up the previous enabled copy's files first, in case the new
        // zip dropped a folder the old one had (otherwise it'd be orphaned).
        if let Some(prev) = &prev {
            if !prev.disabled && !prev.deleted {
                remove_addon_files(&addon_dir, &prev.files);
            }
        }

        emit_progress(app, &addon.name, "downloading", idx + 1, total, None);
        let tmp_zip = std::env::temp_dir().join(format!("sotgl-{}.zip", sanitize_filename(&addon.name)));
        if let Err(e) = download_to_file(&addon.url, &tmp_zip).await {
            emit_progress(app, &addon.name, "error", idx + 1, total, Some(e.clone()));
            result.failed.push((addon.name.clone(), e));
            continue;
        }

        emit_progress(app, &addon.name, "verifying", idx + 1, total, None);
        match sha256_file(&tmp_zip) {
            Ok(hash) if hash.eq_ignore_ascii_case(&addon.sha256) => {}
            Ok(hash) => {
                let _ = std::fs::remove_file(&tmp_zip);
                let msg = format!("checksum mismatch: expected {}, got {hash}", addon.sha256);
                emit_progress(app, &addon.name, "error", idx + 1, total, Some(msg.clone()));
                result.failed.push((addon.name.clone(), msg));
                continue;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_zip);
                result.failed.push((addon.name.clone(), e));
                continue;
            }
        }

        // A custom addon is keyed by whatever its .toc Title happens to be,
        // which often doesn't exactly match the manifest's configured name
        // for the same addon — so without this check, extracting below
        // would silently overwrite a custom addon's files on disk while its
        // OLD state entry (a different key, same folder) never gets touched
        // by the `by_name.insert` further down. Confirmed live: checking a
        // manifest addon that collides this way with an existing custom one
        // did nothing visible, since the stale custom entry just kept
        // showing in the Custom Addons list while its real files were gone.
        // Peeking the zip's own folder names BEFORE writing anything lets
        // any other active, tracked entry that owns one of them get moved
        // to Trash first, instead of silently destroyed.
        if let Ok(incoming_names) = list_zip_top_level(&tmp_zip) {
            let incoming: HashSet<&str> = incoming_names.iter().map(String::as_str).collect();
            let colliding: Vec<String> = by_name
                .iter()
                .filter(|(n, e)| {
                    n.as_str() != addon.name.as_str()
                        && !e.disabled
                        && !e.deleted
                        && e.files.iter().any(|f| incoming.contains(f.as_str()))
                })
                .map(|(n, _)| n.clone())
                .collect();
            for n in colliding {
                if let Some(mut victim) = by_name.get(&n).cloned() {
                    if move_addon_files(&addon_dir, &deleted_dir, &victim.files).is_ok() {
                        victim.deleted = true;
                        by_name.insert(n, victim);
                    }
                }
            }
        }

        emit_progress(app, &addon.name, "extracting", idx + 1, total, None);
        let extracted_files = match extract_zip(&tmp_zip, &addon_dir) {
            Ok(files) => files,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_zip);
                emit_progress(app, &addon.name, "error", idx + 1, total, Some(e.clone()));
                result.failed.push((addon.name.clone(), e));
                continue;
            }
        };
        let _ = std::fs::remove_file(&tmp_zip);

        let (live_version, live_notes, live_deps) = read_live_toc(&addon_dir, &extracted_files).unwrap_or_default();

        by_name.insert(
            addon.name.clone(),
            AddonStateEntry {
                name: addon.name.clone(),
                kind: addon.category.clone(),
                version: live_version.unwrap_or_else(|| addon.version.clone()),
                api_version: Some(addon.version.clone()),
                sha256: Some(addon.sha256.clone()),
                notes: live_notes.unwrap_or_default(),
                files: extracted_files,
                files_present: true,
                disabled: false,
                deleted: false,
                dependencies: live_deps,
            },
        );

        result.updated.push(addon.name.clone());
        emit_progress(app, &addon.name, "done", idx + 1, total, None);
    }

    // Addons that were installed previously but no longer appear in the
    // manifest at all (e.g. an officer deleted the entry) get disabled too,
    // same as an unchecked one - reversible, not deleted outright. Trashed
    // entries are left alone — being off the manifest doesn't change
    // anything about something already sitting in Trash.
    let stale: Vec<String> = by_name
        .iter()
        .filter(|(name, e)| e.kind != "custom" && !e.disabled && !e.deleted && !manifest_names.contains(name.as_str()))
        .map(|(name, _)| name.clone())
        .collect();
    for name in stale {
        if let Some(prev) = by_name.get(&name).cloned() {
            if move_addon_files(&addon_dir, &disabled_dir, &prev.files).is_ok() {
                let mut updated = prev;
                updated.disabled = true;
                by_name.insert(name.clone(), updated);
                result.removed.push(name);
            }
        }
    }

    let final_list: Vec<AddonStateEntry> = by_name.into_values().collect();
    let _ = save_addons_state(&addon_dir, &final_list);

    result
}

pub async fn sync_wtf_files(app: &AppHandle, game_path: &str, files: &[WtfEntry]) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], removed: vec![], failed: vec![] };
    let game_root = PathBuf::from(game_path);
    let total = files.len();

    for (idx, entry) in files.iter().enumerate() {
        let dest = match safe_join(&game_root, &entry.target_path) {
            Ok(p) => p,
            Err(e) => {
                result.failed.push((entry.name.clone(), e));
                continue;
            }
        };

        let up_to_date =
            dest.exists() && sha256_file(&dest).map(|h| h.eq_ignore_ascii_case(&entry.sha256)).unwrap_or(false);
        if up_to_date {
            result.skipped.push(entry.name.clone());
            continue;
        }

        emit_progress(app, &entry.name, "downloading", idx + 1, total, None);
        if let Err(e) = download_to_file(&entry.url, &dest).await {
            emit_progress(app, &entry.name, "error", idx + 1, total, Some(e.clone()));
            result.failed.push((entry.name.clone(), e));
            continue;
        }

        emit_progress(app, &entry.name, "verifying", idx + 1, total, None);
        match sha256_file(&dest) {
            Ok(hash) if hash.eq_ignore_ascii_case(&entry.sha256) => {}
            Ok(hash) => {
                let _ = std::fs::remove_file(&dest);
                let msg = format!("checksum mismatch: expected {}, got {hash}", entry.sha256);
                emit_progress(app, &entry.name, "error", idx + 1, total, Some(msg.clone()));
                result.failed.push((entry.name.clone(), msg));
                continue;
            }
            Err(e) => {
                result.failed.push((entry.name.clone(), e));
                continue;
            }
        }

        result.updated.push(entry.name.clone());
        emit_progress(app, &entry.name, "done", idx + 1, total, None);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_zip(path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();

        zip.start_file("AddonA/AddonA.toc", opts).unwrap();
        zip.write_all(b"## Title: Addon A\n## Version: 1.0\n").unwrap();

        zip.start_file("AddonA/core.lua", opts).unwrap();
        zip.write_all(b"-- core").unwrap();

        zip.start_file("AddonA/libs/lib.lua", opts).unwrap();
        zip.write_all(b"-- lib").unwrap();

        zip.finish().unwrap();
    }

    #[test]
    fn extract_zip_reports_only_top_level_entries_and_removal_deletes_exactly_them() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let zip_path = tmp.join("AddonA.zip");
        let addon_dir = tmp.join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();

        make_test_zip(&zip_path);

        let top_level = extract_zip(&zip_path, &addon_dir).unwrap();
        assert_eq!(top_level, vec!["AddonA".to_string()]);
        assert!(addon_dir.join("AddonA/AddonA.toc").exists());
        assert!(addon_dir.join("AddonA/libs/lib.lua").exists());

        // A sibling addon must survive the removal of AddonA.
        std::fs::create_dir_all(addon_dir.join("OtherAddon")).unwrap();
        std::fs::write(addon_dir.join("OtherAddon/OtherAddon.toc"), b"## Interface: 100000").unwrap();

        remove_addon_files(&addon_dir, &top_level);
        assert!(!addon_dir.join("AddonA").exists());
        assert!(addon_dir.join("OtherAddon").exists());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn is_addon_wanted_applies_category_defaults() {
        let mandatory = AddonEntry {
            name: "M".into(),
            version: "1".into(),
            url: String::new(),
            sha256: String::new(),
            category: "mandatory".into(),
            changelog: String::new(),
        };
        let recommended = AddonEntry { name: "R".into(), category: "recommended".into(), ..mandatory.clone() };
        let optional = AddonEntry { name: "O".into(), category: "optional".into(), ..mandatory.clone() };

        let mut overrides = std::collections::HashMap::new();

        // No overrides: mandatory/recommended on by default, optional off.
        assert!(is_addon_wanted(&mandatory, &overrides));
        assert!(is_addon_wanted(&recommended, &overrides));
        assert!(!is_addon_wanted(&optional, &overrides));

        // Explicit opt-out of recommended, opt-in of optional.
        overrides.insert("M".to_string(), false);
        assert!(is_addon_wanted(&mandatory, &overrides)); // mandatory ignores overrides
        overrides.clear();
        overrides.insert(recommended.name.clone(), false);
        overrides.insert(optional.name.clone(), true);
        assert!(!is_addon_wanted(&recommended, &overrides));
        assert!(is_addon_wanted(&optional, &overrides));
    }

    #[test]
    fn parse_toc_reads_title_version_notes_and_strips_color_codes() {
        let raw = "## Title: |cff1784d1RCLootCouncil|r\n## Version: 3.0.0\n## Notes: Interface for running a Loot Council\n## Interface: 30300\n";
        let (title, version, notes, deps) = parse_toc(raw);
        assert_eq!(title.as_deref(), Some("RCLootCouncil"));
        assert_eq!(version.as_deref(), Some("3.0.0"));
        assert_eq!(notes.as_deref(), Some("Interface for running a Loot Council"));
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_toc_reads_dependencies_from_either_key_comma_separated() {
        let (_, _, _, deps) = parse_toc("## Dependencies: Masque, LibStub\n");
        assert_eq!(deps, vec!["Masque".to_string(), "LibStub".to_string()]);

        // Older addons use RequiredDeps instead; WoW treats it identically.
        let (_, _, _, deps) = parse_toc("## RequiredDeps: LibStub\n");
        assert_eq!(deps, vec!["LibStub".to_string()]);

        // OptionalDeps must never be picked up here - those don't break anything if missing.
        let (_, _, _, deps) = parse_toc("## OptionalDeps: Skada\n");
        assert!(deps.is_empty());
    }

    #[test]
    fn scan_custom_addons_skips_managed_folders_and_reads_toc() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-custom-{}", std::process::id()));
        let addon_dir = tmp.join("AddOns");
        std::fs::create_dir_all(addon_dir.join("ManagedAddon")).unwrap();
        std::fs::create_dir_all(addon_dir.join("PenetratingAnub")).unwrap();
        std::fs::write(
            addon_dir.join("PenetratingAnub/PenetratingAnub.toc"),
            "## Title: Penetrating Anub\n## Version: 1.4\n## Notes: Anub'arak boss mod\n",
        )
        .unwrap();

        let mut claimed = HashSet::new();
        claimed.insert("ManagedAddon".to_string());

        let custom = scan_custom_addons(&addon_dir, &claimed);
        assert_eq!(custom.len(), 1, "managed folder must be excluded, only the unmanaged one listed");
        assert_eq!(custom[0].files, vec!["PenetratingAnub".to_string()]);
        assert_eq!(custom[0].name, "Penetrating Anub");
        assert_eq!(custom[0].version, "1.4");
        assert_eq!(custom[0].notes, "Anub'arak boss mod");
        assert_eq!(custom[0].kind, "custom");
        assert!(!custom[0].disabled);

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn test_manifest() -> Manifest {
        Manifest {
            addons: vec![AddonEntry {
                name: "AddonA".to_string(),
                version: "1.0".to_string(),
                url: String::new(),
                sha256: "deadbeef".to_string(),
                category: "optional".to_string(),
                changelog: String::new(),
            }],
            wtf_files: vec![],
            news: vec![],
        }
    }

    #[test]
    fn sync_addons_writes_single_root_state_file_with_live_toc_version() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-sync-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();

        // Simulate an addon already extracted with a marker recorded, but
        // whose .toc on disk has since drifted from what we last wrote
        // (e.g. hand-edited, or extracted by something else).
        std::fs::create_dir_all(addon_dir.join("AddonA")).unwrap();
        std::fs::write(addon_dir.join("AddonA/AddonA.toc"), b"## Title: Addon A\n## Version: 9.9.9\n").unwrap();

        let entries = vec![AddonStateEntry {
            name: "AddonA".to_string(),
            kind: "optional".to_string(),
            version: "1.0".to_string(), // stale, recorded at a previous install
            api_version: Some("1.0".to_string()),
            sha256: Some("deadbeef".to_string()),
            notes: String::new(),
            files: vec!["AddonA".to_string()],
            files_present: true,
            disabled: false,
            deleted: false,
            dependencies: vec![],
        }];
        save_addons_state(&addon_dir, &entries).unwrap();

        let manifest = test_manifest();
        let game_path = tmp.to_str().unwrap();
        let state = get_installed_state(game_path, &manifest);
        let addon_state = state.addons.get("AddonA").unwrap();
        assert_eq!(addon_state.version, "9.9.9", "installed version must be read live from the .toc, not the stale recorded value");
        assert_eq!(addon_state.api_version.as_deref(), Some("1.0"));
        assert!(addon_state.files_present);

        // The single root file itself should have been rewritten with the refreshed data.
        let on_disk = load_addons_state(&addon_dir);
        let refreshed = on_disk.iter().find(|e| e.name == "AddonA").unwrap();
        assert_eq!(refreshed.version, "9.9.9");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn disabling_moves_files_to_disabled_dir_and_re_enabling_restores_without_redownload() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-disable-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();
        std::fs::create_dir_all(addon_dir.join("AddonA")).unwrap();
        std::fs::write(addon_dir.join("AddonA/AddonA.toc"), b"## Title: Addon A\n## Version: 1.0\n").unwrap();

        let entries = vec![AddonStateEntry {
            name: "AddonA".to_string(),
            kind: "optional".to_string(),
            version: "1.0".to_string(),
            api_version: Some("1.0".to_string()),
            sha256: Some("deadbeef".to_string()),
            notes: String::new(),
            files: vec!["AddonA".to_string()],
            files_present: true,
            disabled: false,
            deleted: false,
            dependencies: vec![],
        }];
        save_addons_state(&addon_dir, &entries).unwrap();

        let game_path = tmp.to_str().unwrap();
        let disabled_dir = disabled_dir_for(game_path);

        set_addon_disabled(game_path, "AddonA", true).unwrap();
        assert!(!addon_dir.join("AddonA").exists(), "folder must leave AddOns");
        assert!(disabled_dir.join("AddonA").exists(), "folder must land in .DisabledAddons");
        let after_disable = load_addons_state(&addon_dir);
        assert!(after_disable.iter().find(|e| e.name == "AddonA").unwrap().disabled);

        set_addon_disabled(game_path, "AddonA", false).unwrap();
        assert!(addon_dir.join("AddonA").exists(), "folder must move back to AddOns");
        assert!(!disabled_dir.join("AddonA").exists());
        let after_enable = load_addons_state(&addon_dir);
        assert!(!after_enable.iter().find(|e| e.name == "AddonA").unwrap().disabled);

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn custom_entry(name: &str, folder: &str) -> AddonStateEntry {
        AddonStateEntry {
            name: name.to_string(),
            kind: "custom".to_string(),
            version: "1.1".to_string(),
            api_version: None,
            sha256: None,
            notes: String::new(),
            files: vec![folder.to_string()],
            files_present: true,
            disabled: false,
            deleted: false,
            dependencies: vec![],
        }
    }

    #[test]
    fn delete_addon_moves_to_trash_and_keeps_tracking_as_deleted() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-delete-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();
        std::fs::create_dir_all(addon_dir.join("PenetratingAnub")).unwrap();
        std::fs::write(addon_dir.join("PenetratingAnub/PenetratingAnub.toc"), b"## Title: Penetrating Anub\n## Version: 1.1\n").unwrap();

        let entries = vec![custom_entry("Penetrating Anub", "PenetratingAnub")];
        save_addons_state(&addon_dir, &entries).unwrap();

        let game_path = tmp.to_str().unwrap();
        let deleted_dir = deleted_dir_for(game_path);

        let result = delete_addon(game_path, "Penetrating Anub").unwrap();
        assert!(result.is_some());
        assert!(!addon_dir.join("PenetratingAnub").exists(), "folder must leave AddOns");
        assert!(deleted_dir.join("PenetratingAnub").exists(), "folder must land in .DeletedAddons, not be destroyed");

        let after_delete = load_addons_state(&addon_dir);
        let entry = after_delete.iter().find(|e| e.name == "Penetrating Anub").unwrap();
        assert!(entry.deleted, "tracking entry must survive, marked deleted");
        assert!(!entry.disabled);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn delete_addon_forever_purges_files_and_tracking() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-purge-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();
        std::fs::create_dir_all(addon_dir.join("PenetratingAnub")).unwrap();
        save_addons_state(&addon_dir, &[custom_entry("Penetrating Anub", "PenetratingAnub")]).unwrap();

        let game_path = tmp.to_str().unwrap();
        let deleted_dir = deleted_dir_for(game_path);

        delete_addon(game_path, "Penetrating Anub").unwrap();
        assert!(deleted_dir.join("PenetratingAnub").exists());

        delete_addon_forever(game_path, "Penetrating Anub").unwrap();
        assert!(!deleted_dir.join("PenetratingAnub").exists(), "must actually remove the trashed files");
        assert!(load_addons_state(&addon_dir).is_empty(), "must drop the tracking entry entirely");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn restore_addon_moves_back_to_addons_and_clears_deleted() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-restore-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();
        std::fs::create_dir_all(addon_dir.join("PenetratingAnub")).unwrap();
        std::fs::write(addon_dir.join("PenetratingAnub/PenetratingAnub.toc"), b"## Title: Penetrating Anub\n## Version: 1.1\n").unwrap();
        save_addons_state(&addon_dir, &[custom_entry("Penetrating Anub", "PenetratingAnub")]).unwrap();

        let game_path = tmp.to_str().unwrap();
        delete_addon(game_path, "Penetrating Anub").unwrap();

        let restored = restore_addon(game_path, "Penetrating Anub", false).unwrap();
        assert!(restored.is_some());
        assert!(addon_dir.join("PenetratingAnub").exists(), "folder must move back to AddOns");
        assert!(!deleted_dir_for(game_path).join("PenetratingAnub").exists());

        let after_restore = load_addons_state(&addon_dir);
        let entry = after_restore.iter().find(|e| e.name == "Penetrating Anub").unwrap();
        assert!(!entry.deleted);
        assert!(!entry.disabled);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn restore_addon_refuses_to_evict_an_occupant_without_force() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-restore-collide-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();
        std::fs::create_dir_all(addon_dir.join("SharedFolder")).unwrap();

        // "Old" is trashed, then a differently-named "New" addon gets
        // installed into the very same folder name while it's away.
        save_addons_state(&addon_dir, &[custom_entry("Old", "SharedFolder")]).unwrap();
        let game_path = tmp.to_str().unwrap();
        delete_addon(game_path, "Old").unwrap();

        std::fs::create_dir_all(addon_dir.join("SharedFolder")).unwrap();
        let mut entries = load_addons_state(&addon_dir);
        entries.push(custom_entry("New", "SharedFolder"));
        save_addons_state(&addon_dir, &entries).unwrap();

        let err = restore_addon(game_path, "Old", false).unwrap_err();
        assert!(err.contains("New"), "error must name the occupant: {err}");
        assert!(addon_dir.join("SharedFolder").exists(), "occupant must survive an unforced restore attempt");

        let restored = restore_addon(game_path, "Old", true).unwrap();
        assert!(restored.is_some(), "forced restore must succeed");
        let after = load_addons_state(&addon_dir);
        assert!(after.iter().find(|e| e.name == "New").is_none(), "occupant must be purged entirely");
        assert!(after.iter().find(|e| e.name == "Old").is_some_and(|e| !e.deleted));

        std::fs::remove_dir_all(&tmp).ok();
    }

    // sync_addons itself needs a real network download and a Tauri
    // AppHandle, so it can't be exercised directly in a unit test — this
    // instead drives the exact same peek-then-evict-then-extract sequence
    // sync_addons runs internally (see its use of list_zip_top_level there),
    // to confirm that ordering actually preserves a colliding custom
    // addon's files instead of the old silent-overwrite behavior.
    #[test]
    fn collision_detection_moves_existing_addon_to_trash_before_extract_would_overwrite_it() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-sync-collide-{}", std::process::id()));
        let addon_dir = tmp.join("Interface").join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();

        // A custom addon happens to occupy the exact folder name the
        // manifest addon's own zip will extract into ("AddonA").
        std::fs::create_dir_all(addon_dir.join("AddonA")).unwrap();
        std::fs::write(addon_dir.join("AddonA/custom-marker.txt"), b"do not overwrite me").unwrap();
        save_addons_state(&addon_dir, &[custom_entry("MyCustomThing", "AddonA")]).unwrap();

        let game_path = tmp.to_str().unwrap();
        let deleted_dir = deleted_dir_for(game_path);

        let zip_path = tmp.join("AddonA.zip");
        make_test_zip(&zip_path);

        let incoming = list_zip_top_level(&zip_path).unwrap();
        assert_eq!(incoming, vec!["AddonA".to_string()]);

        let mut entries = load_addons_state(&addon_dir);
        let victim = entries.iter_mut().find(|e| e.name == "MyCustomThing").unwrap();
        move_addon_files(&addon_dir, &deleted_dir, &victim.files).unwrap();
        victim.deleted = true;
        save_addons_state(&addon_dir, &entries).unwrap();

        assert!(deleted_dir.join("AddonA").exists(), "custom addon's original files must survive in Trash");
        assert!(deleted_dir.join("AddonA/custom-marker.txt").exists());
        assert!(!addon_dir.join("AddonA/custom-marker.txt").exists(), "folder must be clear for the real extract to follow");

        let extracted = extract_zip(&zip_path, &addon_dir).unwrap();
        assert_eq!(extracted, vec!["AddonA".to_string()]);
        assert!(addon_dir.join("AddonA/AddonA.toc").exists(), "manifest addon extracts cleanly into the vacated folder");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
