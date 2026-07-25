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
    /// Stored separately per OS so the same settings.json (e.g. synced
    /// across machines) doesn't show one platform's path under the
    /// other's label — see `Settings::game_path()` for the one that
    /// actually matters at runtime.
    #[serde(default)]
    pub game_path_windows: Option<String>,
    #[serde(default)]
    pub game_path_linux: Option<String>,
    /// Overrides how the game is launched (e.g. a Lutris/Bottles/custom Wine
    /// invocation). "{game_path}" is substituted with the configured game folder.
    #[serde(default)]
    pub launch_command: Option<String>,
    /// If set, the main window is hidden to the system tray (rather than
    /// left open) once `launch_game` successfully starts the game.
    /// Defaults to on (see `default_true` and `Settings::default()`'s
    /// override in settings.rs for brand-new installs).
    #[serde(default = "default_true")]
    pub hide_on_launch: bool,
    /// If set, the window's own close (X) button hides it to the tray
    /// instead of quitting the app — see the CloseRequested handler in
    /// lib.rs. Same default-true treatment as hide_on_launch.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default)]
    pub auto_launch: bool,
    /// addon name -> whether the user wants it active. Only stores explicit
    /// overrides; anything absent falls back to its category default
    /// (mandatory: always on, recommended: on, optional: off). This is a
    /// user preference, not installation state, so it lives here rather
    /// than being derived from the game folder like everything else.
    #[serde(default)]
    pub addon_enabled: std::collections::HashMap<String, bool>,
    /// Personal Bearer token from Discord pairing, sent on requests that
    /// need per-player identity (distinct from the shared, compile-time
    /// baked-in API key — see config.rs).
    #[serde(default)]
    pub discord_token: Option<String>,
    #[serde(default)]
    pub discord_username: Option<String>,
    #[serde(default)]
    pub discord_avatar: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Settings {
    /// The game path for whichever OS this build is actually running on.
    /// The Settings UI can show/edit both platforms' paths (so switching
    /// tabs isn't confusingly showing the same value under both labels),
    /// but only one of them is ever real for a given running instance.
    pub fn game_path(&self) -> Option<String> {
        if cfg!(target_os = "windows") {
            self.game_path_windows.clone()
        } else {
            self.game_path_linux.clone()
        }
    }
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

/// One game from Lutris's own library (native or Flatpak install), with a
/// ready-to-save launch_command already built for it — the Settings UI
/// just needs to display `name` and save `launch_command` verbatim, no
/// native-vs-Flatpak or slug-vs-id logic on the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LutrisGame {
    pub id: u64,
    pub slug: String,
    pub name: String,
    pub runner: String,
    pub launch_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncEquipmentItem {
    pub item_id: i64,
    #[serde(default)]
    pub enchant_id: i64,
    #[serde(default)]
    pub gem_ids: Vec<i64>,
    /// The remaining fields come straight from the addon's own
    /// GetItemInfo() read of this server's item cache — absent (rather
    /// than a placeholder) when the client hasn't cached that item's
    /// tooltip data yet.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub item_type: Option<String>,
    #[serde(default)]
    pub item_sub_type: Option<String>,
    #[serde(default)]
    pub equip_loc_type: Option<String>,
    #[serde(default)]
    pub equip_loc_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Deliberately not camelCase-renamed, unlike its sibling structs — the
/// addon's own collectTalents() emits these 4 keys as snake_case already
/// (same reasoning as GuildSyncStats below). The camelCase rename that
/// used to be here was silently discarding every one of these fields on
/// deserialization (missing Option<T>/#[serde(default)] fields just come
/// back None/empty instead of erroring), so talents were empty both in
/// the launcher's own display and in what got synced to the website.
///
/// `first_tree`/`second_tree` name whichever tree has the most points in
/// that loadout, for display — but WotLK dual spec means each of
/// first/second is a full independent loadout that can spend points in
/// all 3 of the class's trees at once (e.g. a Demonology-leaning build
/// still putting points in Affliction), so `first_points`/`second_points`
/// carry every tree's full per-talent point array, in tab order — not
/// just the dominant tree's. `second_*` are empty/None when the
/// character hasn't unlocked dual spec yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSyncTalents {
    pub first_tree: Option<String>,
    #[serde(default)]
    pub first_points: Vec<Vec<i64>>,
    pub second_tree: Option<String>,
    #[serde(default)]
    pub second_points: Vec<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncProfessions {
    pub first: Option<String>,
    pub first_skill: Option<i64>,
    pub second: Option<String>,
    pub second_skill: Option<i64>,
    pub first_aid: Option<i64>,
    pub cooking: Option<i64>,
    pub fishing: Option<i64>,
}

/// Deliberately not camelCase-renamed — the addon writes these keys as
/// snake_case already (they map 1:1 onto `character_stats`' columns on
/// the web side), unlike the rest of this payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSyncStats {
    pub strength: Option<i64>,
    pub agility: Option<i64>,
    pub stamina: Option<i64>,
    pub intellect: Option<i64>,
    pub spirit: Option<i64>,
    pub armor: Option<i64>,
    pub health: Option<i64>,
    pub mana: Option<i64>,
    pub attack_power: Option<i64>,
    pub melee_crit: Option<f64>,
    pub melee_haste: Option<f64>,
    pub melee_hit: Option<f64>,
    pub expertise: Option<f64>,
    pub armor_penetration: Option<f64>,
    pub ranged_attack_power: Option<i64>,
    pub ranged_crit: Option<f64>,
    pub ranged_hit: Option<f64>,
    pub spell_power: Option<i64>,
    pub spell_crit: Option<f64>,
    pub spell_haste: Option<f64>,
    pub spell_hit: Option<f64>,
    pub spell_penetration: Option<f64>,
    pub mana_regen: Option<f64>,
    pub dodge: Option<f64>,
    pub parry: Option<f64>,
    pub block: Option<f64>,
    pub block_value: Option<i64>,
    pub defense_rating: Option<i64>,
    pub resilience: Option<f64>,
    pub resistance_arcane: Option<i64>,
    pub resistance_fire: Option<i64>,
    pub resistance_frost: Option<i64>,
    pub resistance_nature: Option<i64>,
    pub resistance_shadow: Option<i64>,
}

/// Parsed straight from the SoTGuildSync addon's SavedVariables JSON blob.
/// Used unchanged both to display in the launcher and as the sync-to-web
/// payload — see `guild_sync::read_guild_sync_data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncData {
    pub name: String,
    pub class: String,
    pub race: String,
    pub gender: Option<String>,
    pub guild_rank: Option<String>,
    pub guild_rank_index: Option<i64>,
    pub level: i64,
    pub gold: i64,
    #[serde(default)]
    pub equipment: Vec<GuildSyncEquipmentItem>,
    pub talents: GuildSyncTalents,
    pub honorable_kills: i64,
    pub achievement_points: i64,
    pub stats: GuildSyncStats,
    pub professions: GuildSyncProfessions,
    pub updated_at: i64,
}
