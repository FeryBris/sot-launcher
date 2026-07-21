use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// "mandatory" | "recommended" | "optional"
    pub category: String,
    #[serde(default)]
    pub changelog: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WtfEntry {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// path relative to the WoW install root, e.g. "WTF/Account/ACCOUNTNAME/SavedVariables/HealBot.lua"
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub title: String,
    pub body: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default)]
    pub addons: Vec<AddonEntry>,
    #[serde(default)]
    pub wtf_files: Vec<WtfEntry>,
    #[serde(default)]
    pub news: Vec<NewsItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub game_path: Option<String>,
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,
}

fn default_api_base_url() -> String {
    "https://launcher-api.sotguild.example/api".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LockFile {
    /// item name -> installed version+hash, keyed separately for addons and wtf files
    #[serde(default)]
    pub addons: std::collections::HashMap<String, LockEntry>,
    #[serde(default)]
    pub wtf_files: std::collections::HashMap<String, LockEntry>,
    #[serde(default)]
    pub optional_selected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    pub version: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub item: String,
    pub stage: String, // "downloading" | "verifying" | "extracting" | "done" | "error"
    pub current: usize,
    pub total: usize,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<(String, String)>,
}
