use crate::types::Settings;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve config dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        crate::config::set_active_client(crate::types::GameClient::Wotlk);
        // Settings::default() alone gives these bools' plain false default
        // — override for brand-new installs; existing settings.json files
        // keep whatever the player already chose.
        return Ok(Settings {
            hide_on_launch: true,
            close_to_tray: true,
            discord_signed_in: crate::keychain::get_token().is_some(),
            ..Settings::default()
        });
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut settings: Settings = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    // Site, API key and Discord token follow the active game - see config.rs.
    crate::config::set_active_client(settings.active_client);

    // One-time migration: older builds stored the Discord token in plain
    // text right in this file (a `discord_token` string field, since
    // dropped from the Settings struct — see its doc comment in
    // types.rs). Lift it into the OS keychain so it stops round-tripping
    // through settings.json on every save. Skipped if the keychain
    // already holds a token, so this can never clobber a fresher one.
    // Those builds only knew WotLK, so the token is WotLK's.
    if settings.active_client == crate::types::GameClient::Wotlk && crate::keychain::get_token().is_none() {
        if let Ok(legacy) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(token) = legacy
                .get("discord_token")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty())
            {
                if let Err(e) = crate::keychain::set_token(token) {
                    log::warn!("failed to migrate legacy discord_token into the keychain: {e}");
                }
            }
        }
    }

    settings.discord_signed_in = crate::keychain::get_token().is_some();
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    crate::config::set_active_client(settings.active_client);
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}
