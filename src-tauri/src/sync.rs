use crate::types::{
    AddonEntry, AddonMarker, CustomAddon, InstalledAddonState, InstalledState, Manifest, SyncProgress, SyncResult, WtfEntry,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter};

const EVENT: &str = "sync-progress";
const MARKER_FILENAME: &str = ".sotlauncher.json";

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

async fn download_to_file(url: &str, dest: &Path) -> Result<(), String> {
    let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
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

/// Deletes only the top-level entries recorded for an addon at install time.
fn remove_addon_files(addon_dir: &Path, files: &[String]) {
    for name in files {
        let path = addon_dir.join(name);
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Writes the marker inside the first directory the zip created (or, in the
/// rare case a zip has no top-level directory, as a hidden file at the addon
/// root) so it travels naturally with the addon's own files.
fn write_addon_marker(addon_dir: &Path, marker: &AddonMarker) -> Result<(), String> {
    let home = marker.files.iter().find(|f| addon_dir.join(f).is_dir());
    let marker_path = match home {
        Some(f) => addon_dir.join(f).join(MARKER_FILENAME),
        None => addon_dir.join(format!(".{}{}", sanitize_filename(&marker.name), MARKER_FILENAME)),
    };
    let json = serde_json::to_string_pretty(marker).map_err(|e| e.to_string())?;
    std::fs::write(marker_path, json).map_err(|e| e.to_string())
}

/// Scans immediate subfolders of `addon_dir` for `.sotlauncher.json` markers
/// and returns the installed state they describe, keyed by the addon name
/// each marker records (not the folder name, which need not match).
fn scan_installed_addons(addon_dir: &Path) -> HashMap<String, InstalledAddonState> {
    let mut result = HashMap::new();
    let Ok(entries) = std::fs::read_dir(addon_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(path.join(MARKER_FILENAME)) else {
            continue;
        };
        let Ok(marker) = serde_json::from_str::<AddonMarker>(&raw) else {
            continue;
        };
        let files_present = !marker.files.is_empty() && marker.files.iter().all(|f| addon_dir.join(f).exists());
        result.insert(
            marker.name.clone(),
            InstalledAddonState {
                version: marker.version,
                sha256: marker.sha256,
                files: marker.files,
                files_present,
            },
        );
    }
    result
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

/// Reads `## Title:`, `## Version:`, and `## Notes:` out of a .toc file's header comments.
fn parse_toc(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut title = None;
    let mut version = None;
    let mut notes = None;
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
        }
    }
    (title, version, notes)
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

/// Lists every AddOns folder that isn't claimed by any launcher-managed
/// addon marker, reading its .toc for a human-readable title/version.
fn scan_custom_addons(addon_dir: &Path, claimed_folders: &HashSet<String>) -> Vec<CustomAddon> {
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
        let (title, version, notes) = parse_toc(&raw);
        result.push(CustomAddon {
            title: title.unwrap_or_else(|| folder.clone()),
            version: version.unwrap_or_else(|| "unknown".to_string()),
            notes: notes.unwrap_or_default(),
            folder,
        });
    }
    result.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    result
}

/// Computes current on-disk state for everything in `manifest`, by scanning
/// addon markers and hashing WTF files directly. No cache file involved.
pub fn get_installed_state(game_path: &str, manifest: &Manifest) -> InstalledState {
    let addon_dir = PathBuf::from(game_path).join("Interface").join("AddOns");
    let addons = scan_installed_addons(&addon_dir);

    let claimed_folders: HashSet<String> = addons.values().flat_map(|s| s.files.iter().cloned()).collect();
    let custom_addons = scan_custom_addons(&addon_dir, &claimed_folders);

    let game_root = PathBuf::from(game_path);
    let mut wtf_files = HashMap::new();
    for entry in &manifest.wtf_files {
        if let Ok(path) = safe_join(&game_root, &entry.target_path) {
            if let Ok(hash) = sha256_file(&path) {
                wtf_files.insert(entry.name.clone(), hash);
            }
        }
    }

    InstalledState { addons, wtf_files, custom_addons }
}

pub async fn sync_addons(
    app: &AppHandle,
    game_path: &str,
    addons: &[AddonEntry],
    addon_enabled: &HashMap<String, bool>,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], removed: vec![], failed: vec![] };
    let addon_dir = PathBuf::from(game_path).join("Interface").join("AddOns");
    let _ = std::fs::create_dir_all(&addon_dir);
    let installed = scan_installed_addons(&addon_dir);
    let total = addons.len();
    let manifest_names: std::collections::HashSet<&str> = addons.iter().map(|a| a.name.as_str()).collect();

    for (idx, addon) in addons.iter().enumerate() {
        if !is_addon_wanted(addon, addon_enabled) {
            if let Some(prev) = installed.get(&addon.name) {
                remove_addon_files(&addon_dir, &prev.files);
                result.removed.push(addon.name.clone());
                emit_progress(app, &addon.name, "done", idx + 1, total, Some("removed".to_string()));
            }
            continue;
        }

        let up_to_date = installed
            .get(&addon.name)
            .map(|s| s.files_present && s.version == addon.version && s.sha256.eq_ignore_ascii_case(&addon.sha256))
            .unwrap_or(false);
        if up_to_date {
            result.skipped.push(addon.name.clone());
            continue;
        }

        // Clean up the previous version's files first, in case the new zip
        // dropped a folder the old one had (otherwise it'd be left orphaned).
        if let Some(prev) = installed.get(&addon.name) {
            remove_addon_files(&addon_dir, &prev.files);
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

        let marker = AddonMarker {
            name: addon.name.clone(),
            version: addon.version.clone(),
            sha256: addon.sha256.clone(),
            files: extracted_files,
        };
        if let Err(e) = write_addon_marker(&addon_dir, &marker) {
            emit_progress(app, &addon.name, "error", idx + 1, total, Some(e.clone()));
            result.failed.push((addon.name.clone(), e));
            continue;
        }

        result.updated.push(addon.name.clone());
        emit_progress(app, &addon.name, "done", idx + 1, total, None);
    }

    // Addons that were installed previously but no longer appear in the
    // manifest at all (e.g. an officer deleted the entry) get cleaned up too.
    for (name, state) in &installed {
        if !manifest_names.contains(name.as_str()) {
            remove_addon_files(&addon_dir, &state.files);
            result.removed.push(name.clone());
        }
    }

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

        let up_to_date = dest.exists()
            && sha256_file(&dest).map(|h| h.eq_ignore_ascii_case(&entry.sha256)).unwrap_or(false);
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
        zip.write_all(b"## Interface: 100000").unwrap();

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
    fn marker_write_and_scan_round_trip() {
        let tmp = std::env::temp_dir().join(format!("sotgl-test-marker-{}", std::process::id()));
        let addon_dir = tmp.join("AddOns");
        std::fs::create_dir_all(&addon_dir).unwrap();

        let zip_path = tmp.join("AddonA.zip");
        make_test_zip(&zip_path);
        let files = extract_zip(&zip_path, &addon_dir).unwrap();

        let marker = AddonMarker {
            name: "GuildHealBot".to_string(), // deliberately different from the folder name "AddonA"
            version: "1.2.3".to_string(),
            sha256: "deadbeef".to_string(),
            files: files.clone(),
        };
        write_addon_marker(&addon_dir, &marker).unwrap();
        assert!(addon_dir.join("AddonA").join(MARKER_FILENAME).exists());

        let scanned = scan_installed_addons(&addon_dir);
        let state = scanned.get("GuildHealBot").expect("marker should be found by its recorded name, not folder name");
        assert_eq!(state.version, "1.2.3");
        assert!(state.files_present);

        // Deleting the whole folder makes the addon disappear from scan
        // results entirely (equivalent to "not installed").
        std::fs::remove_dir_all(addon_dir.join("AddonA")).unwrap();
        let scanned_after_deletion = scan_installed_addons(&addon_dir);
        assert!(
            !scanned_after_deletion.contains_key("GuildHealBot"),
            "scanning shouldn't find a marker for a folder that no longer exists"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn files_present_catches_a_missing_secondary_folder() {
        // Some addons unpack into more than one top-level folder (e.g. a
        // core folder plus an options/plugins folder). The marker lives in
        // just one of them, so presence must be checked across all of them,
        // not just the one that happens to hold the marker file.
        let tmp = std::env::temp_dir().join(format!("sotgl-test-multifolder-{}", std::process::id()));
        let addon_dir = tmp.join("AddOns");
        std::fs::create_dir_all(addon_dir.join("BigAddon")).unwrap();
        std::fs::write(addon_dir.join("BigAddon/BigAddon.toc"), b"## Interface: 100000").unwrap();
        std::fs::create_dir_all(addon_dir.join("BigAddon_Options")).unwrap();
        std::fs::write(addon_dir.join("BigAddon_Options/Options.toc"), b"## Interface: 100000").unwrap();

        let marker = AddonMarker {
            name: "BigAddon".to_string(),
            version: "1.0".to_string(),
            sha256: "abc123".to_string(),
            files: vec!["BigAddon".to_string(), "BigAddon_Options".to_string()],
        };
        write_addon_marker(&addon_dir, &marker).unwrap();
        assert!(scan_installed_addons(&addon_dir).get("BigAddon").unwrap().files_present);

        // Delete only the secondary folder; the marker (in BigAddon/) survives.
        std::fs::remove_dir_all(addon_dir.join("BigAddon_Options")).unwrap();
        let scanned = scan_installed_addons(&addon_dir);
        let state = scanned.get("BigAddon").expect("marker folder itself is untouched, so it's still found");
        assert!(!state.files_present, "missing secondary folder must be caught even though the marker itself is intact");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parse_toc_reads_title_version_notes_and_strips_color_codes() {
        let raw = "## Title: |cff1784d1RCLootCouncil|r\n## Version: 3.0.0\n## Notes: Interface for running a Loot Council\n## Interface: 30300\n";
        let (title, version, notes) = parse_toc(raw);
        assert_eq!(title.as_deref(), Some("RCLootCouncil"));
        assert_eq!(version.as_deref(), Some("3.0.0"));
        assert_eq!(notes.as_deref(), Some("Interface for running a Loot Council"));
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
        assert_eq!(custom[0].folder, "PenetratingAnub");
        assert_eq!(custom[0].title, "Penetrating Anub");
        assert_eq!(custom[0].version, "1.4");
        assert_eq!(custom[0].notes, "Anub'arak boss mod");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
