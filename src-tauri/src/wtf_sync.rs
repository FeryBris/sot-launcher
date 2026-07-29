//! Personal cross-device sync for a single WTF/Account/<name> folder —
//! account and character settings (keybinds, macros, chat/UI layout) and
//! every addon's SavedVariables. Distinct from sync.rs's wtf_files, which
//! push guild-wide, admin-managed template files one-directionally to
//! everyone; this is a private, bidirectional backup of one player's own
//! folder, scoped to the paired Discord user via Sanctum (see the
//! wtf-sync/* routes on the website).
//!
//! Sync unit is the whole selected account folder, zipped as one blob —
//! matches the addon sync's own download+extract-zip shape rather than a
//! real file-by-file diff engine, which would be a lot more machinery for
//! a feature most people use one device at a time anyway. Conflict
//! handling is "warn before overwriting": wtf_push/wtf_pull compare a
//! cheap fingerprint (path+size+mtime, not full content hashes — fast
//! enough to run on every launcher open/game close) against what was
//! recorded at the last successful sync, and refuse (with a message
//! naming what would be lost) unless `force` is set — see commands.rs.

use crate::types::WtfSyncStatus;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The only extensions a real WTF/Account folder ever contains (confirmed
/// against a live install): .lua (SavedVariables), .wtf/.txt (account and
/// character config, keybinds, macros, chat/UI layout), and .old (the
/// client's own previous-version backup of those, distinct from an
/// addon's .lua.bak). Whitelisting these rather than naming specific
/// noise files to skip means anything else — cache.md5's checksum, a
/// SavedVariables .bak — is excluded by default rather than needing its
/// own carve-out, and any future cache-file type the client adds stays
/// excluded until explicitly added here too.
const SYNCED_EXTENSIONS: &[&str] = &["lua", "wtf", "txt", "old"];

fn is_synced_file(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SYNCED_EXTENSIONS.iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)))
}

/// Every `WTF/Account/*` subfolder — a single WoW install can have several
/// (shared PC, multiple accounts), so the player picks theirs explicitly
/// in Settings rather than this guessing. Merges in names that only exist
/// under the account folder's VirtualStore mirror (see virtualstore.rs) —
/// an account WoW has only ever written to while running virtualized
/// wouldn't show up here at all otherwise, since the real folder may
/// never have been created.
pub fn list_wtf_accounts(game_path: &str) -> Vec<String> {
    let dir = PathBuf::from(game_path).join("WTF").join("Account");
    let mut names: Vec<String> = crate::virtualstore::list_dirs_merged(&dir)
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// `account` should always be a plain folder name straight from
/// list_wtf_accounts (the Settings picker only ever offers those), but it
/// round-trips through settings.json — a hand-edited file (or a future
/// bug) could set it to something like "../../Interface" and PathBuf::join
/// won't sanitize that away, so it's rejected explicitly here rather than
/// trusted.
///
/// Always returns the real (non-virtualized) path, even when only the
/// account's VirtualStore mirror actually exists — every caller of this
/// (collect_files et al.) resolves the real/mirror split for individual
/// files/folders on its own, so this only needs to confirm the account
/// exists *somewhere*.
pub fn account_dir(game_path: &str, account: &str) -> Result<PathBuf, String> {
    if account.is_empty() || account.contains(['/', '\\']) || account == "." || account == ".." {
        return Err("invalid WTF account name".to_string());
    }
    let dir = PathBuf::from(game_path).join("WTF").join("Account").join(account);
    let exists_virtualized = crate::virtualstore::virtualstore_mirror(&dir).is_some_and(|m| m.is_dir());
    if !dir.is_dir() && !exists_virtualized {
        return Err(format!("WTF/Account/{account} not found — pick a valid account folder in Settings"));
    }
    Ok(dir)
}

/// Every included file under `dir`, as (path relative to dir with forward
/// slashes, absolute path), sorted for a stable fingerprint/zip order.
/// Merges in `dir`'s VirtualStore mirror (see virtualstore.rs) if one
/// exists — an account can end up with some files real and others
/// virtualized (e.g. after a Windows update changed how it's run), so
/// relying on only one location risks silently backing up/fingerprinting
/// an incomplete snapshot. Where the same relative path exists in both,
/// whichever copy was modified more recently wins.
fn collect_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut by_rel: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    // dir itself may not exist at all when the account is entirely
    // virtualized (account_dir() explicitly allows that) - read_dir'ing a
    // missing folder would otherwise error out before ever getting a
    // chance to check the mirror below.
    if dir.is_dir() {
        merge_files_from(dir, &mut by_rel)?;
    }
    if let Some(mirror) = crate::virtualstore::virtualstore_mirror(dir) {
        if mirror.is_dir() {
            merge_files_from(&mirror, &mut by_rel)?;
        }
    }

    let mut out: Vec<(String, PathBuf)> = by_rel.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn merge_files_from(root: &Path, by_rel: &mut std::collections::HashMap<String, PathBuf>) -> Result<(), String> {
    let mut found = Vec::new();
    collect_files_inner(root, root, &mut found)?;
    for (rel, path) in found {
        let candidate_is_newer = match by_rel.get(&rel) {
            Some(existing) => {
                let existing_mtime = std::fs::metadata(existing).and_then(|m| m.modified()).ok();
                let candidate_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                candidate_mtime > existing_mtime
            }
            None => true,
        };
        if candidate_is_newer {
            by_rel.insert(rel, path);
        }
    }
    Ok(())
}

fn collect_files_inner(root: &Path, current: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for entry in std::fs::read_dir(current).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        // DirEntry::file_type() reports the symlink itself, not its
        // target — unlike Path::is_dir()/fs::read, which would silently
        // follow it and pull in whatever it points to, possibly well
        // outside the account folder being zipped.
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_files_inner(root, &path, out)?;
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if !is_synced_file(name_str) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, path));
    }
    Ok(())
}

pub fn has_any_synced_files(dir: &Path) -> Result<bool, String> {
    Ok(!collect_files(dir)?.is_empty())
}

/// Cheap change-detection fingerprint: hashes each included file's
/// relative path + size + mtime, not its actual contents. A real edit
/// always bumps mtime, and this needs to run fast on every launcher
/// open/game close without reading every SavedVariables file (some of
/// which — ElvUI, WeakAuras — can be several MB).
pub fn fingerprint_account(dir: &Path) -> Result<String, String> {
    let files = collect_files(dir)?;
    let mut hasher = Sha256::new();
    for (rel, path) in &files {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let mtime_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(meta.len().to_le_bytes());
        hasher.update(mtime_secs.to_le_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn zip_account(dir: &Path) -> Result<Vec<u8>, String> {
    use std::io::Write;

    let files = collect_files(dir)?;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (rel, path) in &files {
            zip.start_file(rel.as_str(), options).map_err(|e| e.to_string())?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf)
}

/// Extracts `bytes` into a temp folder first and only swaps it into the
/// target folder's place once every entry has been written successfully —
/// a partial extraction straight into the target on failure (a bad chunk,
/// a full disk) would leave the player's real settings half-overwritten
/// with no way back.
///
/// If this account is already virtualized (its VirtualStore mirror
/// exists, meaning WoW's own writes for it are landing there rather than
/// under `dir` — see virtualstore.rs), restores go there instead: once
/// virtualization is active for a path, Windows always serves reads from
/// the mirror first, so restoring only to the real `dir` would leave WoW
/// reading its old data as if nothing had changed.
pub fn extract_account_zip(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let target_dir = match crate::virtualstore::virtualstore_mirror(dir) {
        Some(mirror) if mirror.is_dir() => mirror,
        _ => dir.to_path_buf(),
    };

    let tmp_name = format!(
        ".wtfsync-tmp-{}",
        target_dir.file_name().and_then(|n| n.to_str()).unwrap_or("account")
    );
    let tmp_dir = target_dir.with_file_name(tmp_name);
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let extract_result = extract_into(bytes, &tmp_dir);
    if let Err(e) = extract_result {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp_dir, &target_dir).map_err(|e| e.to_string())
}

fn extract_into(bytes: &[u8], dest_dir: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name() refuses entries that try to escape dest_dir (zip-slip protection).
        let Some(name) = entry.enclosed_name() else { continue };
        let outpath = dest_dir.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Best-effort check for a running Wow.exe/WowClassic.exe — guards
/// wtf_push/wtf_pull against reading or writing SavedVariables files the
/// client might currently have open (worst case right around logout,
/// which does the biggest flush). Shells out rather than adding a
/// process-listing crate, matching this file's existing
/// Lutris/Flatpak-detection style elsewhere in commands.rs.
pub fn is_wow_running() -> bool {
    if cfg!(target_os = "windows") {
        crate::commands::WOW_EXE_NAMES.iter().any(|name| {
            let filter = format!("IMAGENAME eq {name}");
            std::process::Command::new("tasklist")
                .args(["/FI", filter.as_str(), "/NH"])
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).to_lowercase().contains(&name.to_lowercase()))
                .unwrap_or(false)
        })
    } else {
        // Wine preserves the emulated .exe's own filename in the process
        // list, so matching on it here works the same way find_wow_exe's
        // WOW_EXE_NAMES check does when launching.
        std::process::Command::new("pgrep")
            .args(["-fi", "wow(classic)?\\.exe"])
            .output()
            .map(|out| !out.stdout.is_empty())
            .unwrap_or(false)
    }
}

/// Spawns a background thread that waits for Wow.exe/WowClassic.exe to
/// start (in case launch is slow — otherwise a fresh check might see
/// "not running" and mistake that for the game already having closed),
/// then waits for it to exit again, and only then emits `game-closed` for
/// the frontend to act on (auto-push, if the player enabled it). Guarded
/// by AppState::game_close_watching so clicking Play again while the game
/// is already running doesn't stack up a second poller.
pub fn watch_for_game_close(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    use tauri::{Emitter, Manager};

    {
        let state = app.state::<crate::commands::AppState>();
        if state.game_close_watching.swap(true, Ordering::SeqCst) {
            return;
        }
    }

    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !is_wow_running() && start.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        while is_wow_running() {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }

        app.state::<crate::commands::AppState>().game_close_watching.store(false, Ordering::SeqCst);
        let _ = app.emit("game-closed", ());
    });
}

const WTF_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

pub async fn upload_wtf_zip(http: &reqwest::Client, token: &str, zip_bytes: Vec<u8>) -> Result<WtfSyncStatus, String> {
    let url = format!("{}/wtf-sync/upload", crate::config::API_BASE_URL.trim_end_matches('/'));
    let part = reqwest::multipart::Part::bytes(zip_bytes)
        .file_name("wtf.zip")
        .mime_str("application/zip")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = http
        .post(&url)
        .timeout(WTF_SYNC_TIMEOUT)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while uploading WTF data", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn download_wtf_zip(http: &reqwest::Client, token: &str) -> Result<Vec<u8>, String> {
    let url = format!("{}/wtf-sync/download", crate::config::API_BASE_URL.trim_end_matches('/'));
    let resp = http
        .get(&url)
        .timeout(WTF_SYNC_TIMEOUT)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("no synced WTF data yet — push from a device with your settings on it first".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while downloading WTF data", resp.status()));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

pub async fn fetch_wtf_status(http: &reqwest::Client, token: &str) -> Result<WtfSyncStatus, String> {
    let url = format!("{}/wtf-sync/status", crate::config::API_BASE_URL.trim_end_matches('/'));
    let resp = http
        .get(&url)
        .bearer_auth(token)
        .header("X-API-KEY", crate::config::API_KEY)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} while checking WTF sync status", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn account_dir_rejects_path_traversal() {
        assert!(account_dir("C:/WoW", "../../Interface").is_err());
        assert!(account_dir("C:/WoW", "..").is_err());
        assert!(account_dir("C:/WoW", "sub/dir").is_err());
        assert!(account_dir("C:/WoW", "sub\\dir").is_err());
        assert!(account_dir("C:/WoW", "").is_err());
    }

    #[test]
    fn fingerprint_ignores_extensions_outside_the_synced_set() {
        let tmp = std::env::temp_dir().join(format!("wtfsync-test-{}", uuid_like()));
        write(&tmp.join("config-cache.wtf"), "a");
        let fp1 = fingerprint_account(&tmp).unwrap();
        write(&tmp.join("cache.md5"), "checksum, not settings");
        write(&tmp.join("SavedVariables").join("Foo.lua.bak"), "backup, not real data");
        let fp2 = fingerprint_account(&tmp).unwrap();
        assert_eq!(fp1, fp2, "only .lua/.wtf/.txt/.old should ever affect the fingerprint");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn fingerprint_includes_every_synced_extension() {
        let tmp = std::env::temp_dir().join(format!("wtfsync-test-{}", uuid_like()));
        write(&tmp.join("config-cache.wtf"), "a");
        let fp1 = fingerprint_account(&tmp).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&tmp.join("SavedVariables").join("ElvUI.lua"), "ElvUIDB = {}");
        let fp2 = fingerprint_account(&tmp).unwrap();
        assert_ne!(fp1, fp2, ".lua should affect the fingerprint");

        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&tmp.join("layout-local.txt"), "b");
        let fp3 = fingerprint_account(&tmp).unwrap();
        assert_ne!(fp2, fp3, ".txt should affect the fingerprint");

        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&tmp.join("macros-cache.old"), "c");
        let fp4 = fingerprint_account(&tmp).unwrap();
        assert_ne!(fp3, fp4, ".old should affect the fingerprint");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn fingerprint_changes_when_a_real_file_changes() {
        let tmp = std::env::temp_dir().join(format!("wtfsync-test-{}", uuid_like()));
        write(&tmp.join("SavedVariables").join("ElvUI.lua"), "profile v1");
        let fp1 = fingerprint_account(&tmp).unwrap();
        // Force a distinct mtime — some filesystems have 1s+ mtime granularity.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&tmp.join("SavedVariables").join("ElvUI.lua"), "profile v2, longer content");
        let fp2 = fingerprint_account(&tmp).unwrap();
        assert_ne!(fp1, fp2);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn zip_then_extract_round_trips_included_files_only() {
        let src = std::env::temp_dir().join(format!("wtfsync-test-src-{}", uuid_like()));
        write(&src.join("config-cache.wtf"), "account settings");
        write(&src.join("SavedVariables").join("ElvUI.lua"), "ElvUIDB = {}");
        write(&src.join("cache.md5"), "should not round-trip");

        let zip = zip_account(&src).unwrap();

        let dest = std::env::temp_dir().join(format!("wtfsync-test-dest-{}", uuid_like()));
        extract_account_zip(&zip, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(dest.join("config-cache.wtf")).unwrap(), "account settings");
        assert_eq!(std::fs::read_to_string(dest.join("SavedVariables").join("ElvUI.lua")).unwrap(), "ElvUIDB = {}");
        assert!(!dest.join("cache.md5").exists());

        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn extract_replaces_stale_files_not_present_in_the_new_zip() {
        let src = std::env::temp_dir().join(format!("wtfsync-test-src2-{}", uuid_like()));
        write(&src.join("SavedVariables").join("New.lua"), "new");
        let zip = zip_account(&src).unwrap();

        let dest = std::env::temp_dir().join(format!("wtfsync-test-dest2-{}", uuid_like()));
        write(&dest.join("SavedVariables").join("Stale.lua"), "leftover from a removed addon");

        extract_account_zip(&zip, &dest).unwrap();

        assert!(dest.join("SavedVariables").join("New.lua").exists());
        assert!(
            !dest.join("SavedVariables").join("Stale.lua").exists(),
            "a full account-folder pull should mirror the cloud copy, not merge onto stale local files"
        );

        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dest).unwrap();
    }

    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!(
            "{}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            n
        )
    }
}
