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

fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name() refuses entries that try to escape dest_dir (zip-slip protection)
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let outpath = dest_dir.join(name);
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
    Ok(())
}

pub async fn sync_addons(
    app: &AppHandle,
    game_path: &str,
    addons: &[AddonEntry],
    selected_optional: &[String],
    lock: &mut LockFile,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], failed: vec![] };
    let addon_dir = PathBuf::from(game_path).join("Interface").join("AddOns");
    let total = addons.len();

    for (idx, addon) in addons.iter().enumerate() {
        let wanted = addon.category == "mandatory"
            || addon.category == "recommended"
            || selected_optional.iter().any(|n| n == &addon.name);
        if !wanted {
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
        if let Err(e) = extract_zip(&tmp_zip, &addon_dir) {
            let _ = std::fs::remove_file(&tmp_zip);
            emit_progress(app, &addon.name, "error", idx + 1, total, Some(e.clone()));
            result.failed.push((addon.name.clone(), e));
            continue;
        }
        let _ = std::fs::remove_file(&tmp_zip);

        lock.addons.insert(
            addon.name.clone(),
            LockEntry { version: addon.version.clone(), sha256: addon.sha256.clone() },
        );
        result.updated.push(addon.name.clone());
        emit_progress(app, &addon.name, "done", idx + 1, total, None);
    }

    lock.optional_selected = selected_optional.to_vec();
    result
}

pub async fn sync_wtf_files(
    app: &AppHandle,
    game_path: &str,
    files: &[WtfEntry],
    lock: &mut LockFile,
) -> SyncResult {
    let mut result = SyncResult { updated: vec![], skipped: vec![], failed: vec![] };
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
            LockEntry { version: entry.version.clone(), sha256: entry.sha256.clone() },
        );
        result.updated.push(entry.name.clone());
        emit_progress(app, &entry.name, "done", idx + 1, total, None);
    }

    result
}
