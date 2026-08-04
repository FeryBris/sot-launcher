//! Stores the Discord session token in the OS-native credential store
//! (Windows Credential Manager, macOS/iOS Keychain, or the Linux Secret
//! Service via D-Bus) instead of plaintext in settings.json — a local
//! process reading the config directory (malware, another user account,
//! a shared support bundle) no longer gets the token for free.
//!
//! Linux has no secret service on some minimal setups (headless boxes,
//! some window managers without gnome-keyring/kwallet running) — every
//! function here degrades to "acts as if signed out" rather than
//! panicking or failing the caller in that case.

const SERVICE: &str = "gg.sotlauncher";
const ACCOUNT: &str = "discord_token";

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).ok()
}

pub fn get_token() -> Option<String> {
    entry()?.get_password().ok()
}

pub fn set_token(token: &str) -> Result<(), String> {
    entry()
        .ok_or_else(|| "no credential store available on this system".to_string())?
        .set_password(token)
        .map_err(|e| e.to_string())
}

/// Idempotent — succeeds whether or not a token was actually stored.
pub fn delete_token() {
    if let Some(e) = entry() {
        let _ = e.delete_credential();
    }
}
