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
    /// Overrides how the game is launched (e.g. a Lutris/Bottles/custom Wine
    /// invocation). "{game_path}" is substituted with the configured game folder.
    #[serde(default)]
    pub launch_command: Option<String>,
    #[serde(default)]
    pub close_on_launch: bool,
    #[serde(default)]
    pub auto_launch: bool,
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,
    /// Sent as the X-API-KEY header on manifest/update requests.
    #[serde(default)]
    pub api_key: Option<String>,
    /// addon name -> whether the user wants it active. Only stores explicit
    /// overrides; anything absent falls back to its category default
    /// (mandatory: always on, recommended: on, optional: off). This is a
    /// user preference, not installation state, so it lives here rather
    /// than being derived from the game folder like everything else.
    #[serde(default)]
    pub addon_enabled: std::collections::HashMap<String, bool>,
    /// Personal Bearer token from Discord pairing, sent on requests that
    /// need per-player identity (distinct from the shared api_key above).
    #[serde(default)]
    pub discord_token: Option<String>,
    #[serde(default)]
    pub discord_username: Option<String>,
    #[serde(default)]
    pub discord_avatar: Option<String>,
}

fn default_api_base_url() -> String {
    "https://sot.sk/api/launcher".to_string()
}

/// One entry in `Interface/AddOns/.sotlauncher.json` — the single file that
/// records everything the launcher knows about every addon folder, managed
/// or not. Rewritten in full on every sync and every state refresh.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddonStateEntry {
    pub name: String,
    /// "mandatory" | "recommended" | "optional" | "custom"
    #[serde(rename = "type")]
    pub kind: String,
    /// Version read fresh from the addon's own .toc file on disk — what's
    /// actually installed right now, independent of anything we recorded.
    pub version: String,
    /// Version the guild manifest currently declares. Absent for custom addons.
    #[serde(default)]
    pub api_version: Option<String>,
    /// sha256 the manifest declared when this was installed. Absent for
    /// custom addons. This, not `version`, is what decides whether a sync
    /// re-downloads something — addon authors don't always bump their .toc
    /// Version field when they ship a fix, so trusting the hash is safer.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub notes: String,
    /// Top-level folder(s) this addon owns, under Interface/AddOns when
    /// enabled or Interface/.DisabledAddons when disabled.
    #[serde(default)]
    pub files: Vec<String>,
    /// False if any folder in `files` is actually missing from its expected location.
    #[serde(default)]
    pub files_present: bool,
    /// True if this addon's folders currently live in Interface/.DisabledAddons
    /// instead of Interface/AddOns.
    #[serde(default)]
    pub disabled: bool,
    /// Folder names of other addons this one requires to function, read from
    /// its .toc `## Dependencies:`/`## RequiredDeps:` line. Used to warn
    /// before disabling/deleting something another installed addon needs.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddonsStateFile {
    #[serde(default)]
    pub addons: Vec<AddonStateEntry>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledState {
    pub addons: std::collections::HashMap<String, AddonStateEntry>,
    /// wtf file name -> sha256 currently on disk (absent if the file doesn't exist)
    pub wtf_files: std::collections::HashMap<String, String>,
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
    pub removed: Vec<String>,
    pub failed: Vec<(String, String)>,
}
