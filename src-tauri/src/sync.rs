use crate::types::{AddonEntry, LockEntry, LockFile, SyncProgress, SyncResult, WtfEntry};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter};

const EVENT: &str = "sync-progress";

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

fn is_addon_wanted(addon: &AddonEntry, addon_enabled: &std::collections::HashMap<String, bool>) -> bool {
    match addon.category.as_str() {
        "mandatory" => true,
        "recommended" => *addon_enabled.get(&addon.name).unwrap_or(&true),
        _ => *addon_enabled.get(&addon.name).unwrap_or(&false),
    }
}

pub async fn sync_addons(
    app: &AppHandle,
    game_path: &str,
    addons: &[AddonEntry],
    addon_enabled: &std::collections::HashMap<String, bool>,
    lock: &mut LockFile,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], removed: vec![], failed: vec![] };
    let addon_dir = PathBuf::from(game_path).join("Interface").join("AddOns");
    let total = addons.len();
    let manifest_names: std::collections::HashSet<&str> = addons.iter().map(|a| a.name.as_str()).collect();

    for (idx, addon) in addons.iter().enumerate() {
        if !is_addon_wanted(addon, addon_enabled) {
            if let Some(prev) = lock.addons.remove(&addon.name) {
                remove_addon_files(&addon_dir, &prev.files);
                result.removed.push(addon.name.clone());
                emit_progress(app, &addon.name, "done", idx + 1, total, Some("removed".to_string()));
            }
            continue;
        }

        let up_to_date = lock
            .addons
            .get(&addon.name)
            .map(|e| e.version == addon.version && e.sha256.eq_ignore_ascii_case(&addon.sha256))
            .unwrap_or(false);
        if up_to_date {
            result.skipped.push(addon.name.clone());
            continue;
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

        lock.addons.insert(
            addon.name.clone(),
            LockEntry { version: addon.version.clone(), sha256: addon.sha256.clone(), files: extracted_files },
        );
        result.updated.push(addon.name.clone());
        emit_progress(app, &addon.name, "done", idx + 1, total, None);
    }

    // Addons that were installed previously but no longer appear in the
    // manifest at all (e.g. an officer deleted the entry) get cleaned up too.
    let stale: Vec<String> = lock
        .addons
        .keys()
        .filter(|name| !manifest_names.contains(name.as_str()))
        .cloned()
        .collect();
    for name in stale {
        if let Some(prev) = lock.addons.remove(&name) {
            remove_addon_files(&addon_dir, &prev.files);
            result.removed.push(name);
        }
    }

    lock.addon_enabled = addon_enabled.clone();
    result
}

pub async fn sync_wtf_files(
    app: &AppHandle,
    game_path: &str,
    files: &[WtfEntry],
    lock: &mut LockFile,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], removed: vec![], failed: vec![] };
    let game_root = PathBuf::from(game_path);
    let total = files.len();

    for (idx, entry) in files.iter().enumerate() {
        let up_to_date = lock
            .wtf_files
            .get(&entry.name)
            .map(|e| e.version == entry.version && e.sha256.eq_ignore_ascii_case(&entry.sha256))
            .unwrap_or(false);
        if up_to_date {
            result.skipped.push(entry.name.clone());
            continue;
        }

        let dest = match safe_join(&game_root, &entry.target_path) {
            Ok(p) => p,
            Err(e) => {
                result.failed.push((entry.name.clone(), e));
                continue;
            }
        };

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

        lock.wtf_files.insert(
            entry.name.clone(),
            LockEntry { version: entry.version.clone(), sha256: entry.sha256.clone(), files: vec![] },
        );
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
}
