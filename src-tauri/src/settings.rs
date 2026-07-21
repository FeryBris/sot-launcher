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
        return Ok(Settings::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())
}
