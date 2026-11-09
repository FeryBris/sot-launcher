//! Baked in at compile time from `.env` by `build.rs` — see
//! `.env.example`. Deliberately not part of `Settings`: these aren't
//! player-editable, and the API key must never reach the frontend.
//!
//! Each game has its own guild site (WotLK: SOT_API_BASE_URL, Forever:
//! SOT_API_BASE_URL_FOREVER), with its own launcher API key and its own
//! Discord sign-in (keychain.rs). Requests go to the active game's site;
//! `set_active_client` is called by settings::load_settings, which every
//! command runs first, so it always follows the game the launcher shows.

use crate::types::GameClient;
use std::sync::atomic::{AtomicBool, Ordering};

/// The main site's key - also what the self-updater sends (launcher
/// releases are published on the main site only).
pub const API_KEY: &str = env!("SOT_API_KEY");
const API_BASE_URL: &str = env!("SOT_API_BASE_URL");
/// Optional: without them Forever uses the main site.
const API_KEY_FOREVER: Option<&str> = option_env!("SOT_API_KEY_FOREVER");
const API_BASE_URL_FOREVER: Option<&str> = option_env!("SOT_API_BASE_URL_FOREVER");

static FOREVER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_active_client(client: GameClient) {
    FOREVER_ACTIVE.store(client == GameClient::Forever, Ordering::Relaxed);
}

pub fn active_client() -> GameClient {
    if FOREVER_ACTIVE.load(Ordering::Relaxed) {
        GameClient::Forever
    } else {
        GameClient::Wotlk
    }
}

/// The active game's site API base, e.g. `https://sot.sk/api/launcher`.
pub fn api_base_url() -> &'static str {
    let url = match active_client() {
        GameClient::Wotlk => API_BASE_URL,
        GameClient::Forever => API_BASE_URL_FOREVER.filter(|u| !u.is_empty()).unwrap_or(API_BASE_URL),
    };
    url.trim_end_matches('/')
}

/// The active game's site launcher API key.
pub fn api_key() -> &'static str {
    match active_client() {
        GameClient::Wotlk => API_KEY,
        GameClient::Forever => API_KEY_FOREVER.filter(|k| !k.is_empty()).unwrap_or(API_KEY),
    }
}
