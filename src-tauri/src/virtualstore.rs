//! Windows' UAC file-system virtualization redirects a legacy
//! (unmanifested) 32-bit app's writes under Program Files/Windows to a
//! hidden per-user shadow copy (%LOCALAPPDATA%\VirtualStore\...) instead
//! of the real location, whenever that app isn't running elevated - and
//! for that same process, reads for that path then transparently come
//! back from the shadow copy too. This launcher is a modern, manifested
//! exe, so it's never subject to this itself - it always sees the real
//! path. But WoW's own (old, unmanifested) 3.3.5a client very much still
//! gets virtualized when installed under Program Files without being run
//! as admin, which is a common enough default install location that the
//! launcher would otherwise regularly end up reading a stale/empty real
//! WTF/Account path while the actual fresh SavedVariables data sits
//! invisibly in the shadow copy instead.
//!
//! Every function here is a no-op passthrough on non-Windows targets and
//! for any path that isn't under a virtualization-eligible root -
//! callers never need their own `cfg(windows)` gating.

use std::path::{Path, PathBuf};

/// The would-be VirtualStore mirror of `path`, if `path` is under a
/// virtualization-eligible root (Program Files, Program Files (x86), or
/// the Windows folder) — `None` otherwise.
pub fn virtualstore_mirror(path: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "windows") {
        return None;
    }

    let path_str = path.to_str()?;
    let lower = path_str.to_lowercase();
    let is_virtualized_root = ["\\program files (x86)\\", "\\program files\\", "\\windows\\"]
        .iter()
        .any(|root| lower.contains(root));
    if !is_virtualized_root {
        return None;
    }

    // VirtualStore mirrors the path verbatim beneath %LOCALAPPDATA%\VirtualStore,
    // with the drive letter dropped (e.g. "C:\Program Files (x86)\Foo" ->
    // "%LOCALAPPDATA%\VirtualStore\Program Files (x86)\Foo").
    let bytes = path_str.as_bytes();
    if bytes.len() <= 2 || bytes[1] != b':' || !matches!(bytes[2], b'\\' | b'/') {
        return None;
    }
    let without_drive = &path_str[3..];

    let local_app_data = dirs::data_local_dir()?;
    Some(local_app_data.join("VirtualStore").join(without_drive))
}

/// Picks whichever of `path` or its VirtualStore mirror actually has the
/// data — the mirror if only it exists, or whichever of the two was
/// modified more recently if both do. Returns `path` unchanged if there's
/// no mirror to consider (non-Windows, or not under a virtualized root).
pub fn resolve_read_path(path: &Path) -> PathBuf {
    let Some(mirror) = virtualstore_mirror(path) else {
        return path.to_path_buf();
    };

    let real_exists = path.exists();
    let mirror_exists = mirror.exists();

    if !real_exists && mirror_exists {
        return mirror;
    }
    if real_exists && mirror_exists {
        let real_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mirror_mtime = std::fs::metadata(&mirror).and_then(|m| m.modified()).ok();
        if mirror_mtime > real_mtime {
            return mirror;
        }
    }
    path.to_path_buf()
}

/// Every subdirectory immediately under `base`, merging the real location
/// with its VirtualStore mirror (if any) and deduping by folder name — an
/// account's WTF data can end up entirely in one location or the other
/// depending on whether virtualization was active for it.
pub fn list_dirs_merged(base: &Path) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    let mut add_from = |dir: &Path| {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str().map(str::to_string) {
                if seen.insert(name) {
                    out.push(path);
                }
            }
        }
    };

    add_from(base);
    if let Some(mirror) = virtualstore_mirror(base) {
        add_from(&mirror);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_program_files_path_has_no_mirror() {
        assert!(virtualstore_mirror(Path::new(r"C:\Games\WoW\WTF\Account\FOO")).is_none());
    }

    #[test]
    fn relative_path_has_no_mirror() {
        assert!(virtualstore_mirror(Path::new("WTF/Account/FOO")).is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn program_files_x86_path_maps_under_virtualstore() {
        let mirror = virtualstore_mirror(Path::new(r"C:\Program Files (x86)\World of Warcraft\WTF"))
            .expect("should compute a mirror for a Program Files (x86) path");
        let mirror_str = mirror.to_string_lossy().to_lowercase();
        assert!(mirror_str.contains("virtualstore"));
        assert!(mirror_str.ends_with(r"program files (x86)\world of warcraft\wtf"));
    }
}
