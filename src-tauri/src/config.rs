//! Baked in at compile time from `.env` by `build.rs` — see
//! `.env.example`. Deliberately not part of `Settings`: these aren't
//! player-editable, and the API key must never reach the frontend.

pub const API_KEY: &str = env!("SOT_API_KEY");
pub const API_BASE_URL: &str = env!("SOT_API_BASE_URL");
