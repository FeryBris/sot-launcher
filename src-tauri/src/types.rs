use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
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

/// The cloud copy's state for the paired user's personal WTF backup —
/// distinct from WtfEntry/manifest wtf_files, which are guild-wide,
/// admin-managed template files pushed one-directionally to everyone.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WtfSyncStatus {
    pub exists: bool,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub size: Option<i64>,
}

/// One cloud-known WTF account backup, from GET /wtf-sync/accounts — unlike
/// WtfSyncStatus (one specific account's status) this is the full list, so
/// the launcher can offer a Pull for an account backed up from another
/// device that hasn't logged into this one yet (its WTF/Account/<name>
/// folder wouldn't exist locally, so list_wtf_accounts() alone can't find it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WtfCloudAccountEntry {
    pub account: String,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub size: Option<i64>,
}

/// Per-account sync bookkeeping — see Settings::wtf_account_sync.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WtfAccountSyncState {
    /// Fingerprint (see wtf_sync::fingerprint_account) of the local account
    /// folder as of the last successful push or pull — compared against the
    /// current fingerprint to detect local changes made since, so a pull
    /// can warn before silently discarding them.
    #[serde(default)]
    pub last_synced_fingerprint: Option<String>,
    /// The cloud copy's updatedAt as of the last successful push or pull —
    /// compared against a fresh status check to detect another device
    /// having pushed since, so a push can warn before silently overwriting it.
    #[serde(default)]
    pub last_known_remote_updated_at: Option<i64>,
    /// Wall-clock ms of the last successful push or pull, for display only.
    #[serde(default)]
    pub last_synced_at: Option<i64>,
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
    /// Whether the paired Discord user holds officer guild rank, as of the
    /// last pairing exchange — gates the Raids tab/Sync Raid action.
    #[serde(default)]
    pub is_officer: bool,
    /// Applies to every detected WTF/Account/<name> folder, not just one —
    /// a single WoW install can have several (shared PC, multiple
    /// accounts), and each is synced to its own cloud slot (see
    /// wtf_account_sync below) rather than picking one as "the" account.
    #[serde(default)]
    pub wtf_sync_auto_pull: bool,
    #[serde(default)]
    pub wtf_sync_auto_push: bool,
    /// Per-account sync bookkeeping, keyed by WTF/Account/<name> — lets
    /// several accounts under the same WoW install each track their own
    /// last-synced state independently instead of sharing one slot.
    #[serde(default)]
    pub wtf_account_sync: std::collections::HashMap<String, WtfAccountSyncState>,
    /// When on, debug_log::verbose() calls actually persist to debug.json
    /// instead of being dropped — see that function's doc comment. Off by
    /// default since most players never need to see "every action", just
    /// errors (which always log regardless of this).
    #[serde(default)]
    pub verbose_logging: bool,
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
    /// Top-level folder(s) this addon owns, under Interface/AddOns,
    /// Interface/.DisabledAddons, or Interface/.DeletedAddons depending on
    /// disabled/deleted below.
    #[serde(default)]
    pub files: Vec<String>,
    /// False if any folder in `files` is actually missing from its expected location.
    #[serde(default)]
    pub files_present: bool,
    /// True if this addon's folders currently live in Interface/.DisabledAddons
    /// instead of Interface/AddOns. Always false once `deleted` is true —
    /// disabled and deleted are two different locations, never both at once.
    #[serde(default)]
    pub disabled: bool,
    /// True if this addon's folders currently live in Interface/.DeletedAddons
    /// (soft-deleted — recoverable via Restore, or purged via Delete Forever).
    /// A separate flag from `disabled` (rather than folding it into one
    /// enum) so existing .sotlauncher.json files from before this field
    /// existed keep working unchanged — everything just defaults to false.
    #[serde(default)]
    pub deleted: bool,
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

/// Deliberately not camelCase-renamed, matching GuildSyncGlyphs — the
/// addon's collectEquipment() writes these 2 keys as snake_case. Equipment
/// is tied to the active dual-spec loadout the same way talents and glyphs
/// are (same "can't read the inactive group" server limitation — see
/// GuildSyncTalents below): a character can gear up differently per spec
/// (e.g. a tank/DPS Warrior), so first_equipment/second_equipment follow
/// the identical convention as first_glyphs/second_glyphs — whichever spec
/// is active gets refreshed, the other slot carries forward its last-known
/// snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuildSyncEquipment {
    #[serde(default)]
    pub first_equipment: Vec<GuildSyncEquipmentItem>,
    #[serde(default)]
    pub second_equipment: Vec<GuildSyncEquipmentItem>,
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
    /// Which dual-spec group (1 or 2) was actually active in-game during
    /// this collection — first_points/second_points look identical
    /// whether freshly collected or carried forward from a previous
    /// snapshot, so the website needs this to know which one is current.
    /// Defaults to 1 for older SavedVariables entries written before this
    /// field existed.
    #[serde(default = "default_active_group")]
    pub active_group: i64,
}

fn default_active_group() -> i64 {
    1
}

/// A single socketed glyph — `id` is the glyph EFFECT's spell ID (what
/// GetGlyphSocketInfo reports), not an item ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSyncGlyph {
    pub id: i64,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub glyph_type: Option<String>,
}

/// Deliberately not camelCase-renamed, matching GuildSyncTalents — the
/// addon's collectGlyphs() writes these 2 keys as snake_case. Glyphs are
/// tied to the active dual-spec loadout the same way talents are (and
/// this server's client has the same "can't read the inactive group"
/// limitation, confirmed for talents — see GuildSyncTalents above), so
/// first_glyphs/second_glyphs follow the identical convention: whichever
/// spec is active gets refreshed, the other slot carries forward its
/// last-known snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuildSyncGlyphs {
    #[serde(default)]
    pub first_glyphs: Vec<GuildSyncGlyph>,
    #[serde(default)]
    pub second_glyphs: Vec<GuildSyncGlyph>,
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

/// One faction row from GetFactionInfo, reported unfiltered — isHeader/
/// hasRep don't reliably mark "real faction" on this server (confirmed
/// live), so the addon just sends everything it saw and lets the website
/// decide what's worth showing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncReputationEntry {
    pub name: String,
    pub standing: Option<i64>,
    pub value: Option<i64>,
    pub max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncRecentAchievement {
    pub id: i64,
    pub name: Option<String>,
    pub points: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
    pub year: Option<i64>,
}

/// `total`/`completed` come from a GetCategoryNumAchievements(catID, true)
/// scan — the `true` (includeAll) is required, without it this undercounts
/// (confirmed live: 911 vs. the 1058 the in-game Achievements frame itself
/// reports). Overall point total is tracked separately on
/// GuildSyncData::achievement_points, not duplicated here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncAchievements {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub completed: i64,
    #[serde(default)]
    pub recent: Vec<GuildSyncRecentAchievement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncCurrencyItem {
    pub name: String,
    pub count: Option<i64>,
    pub item_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncCurrency {
    pub honor: Option<i64>,
    pub arena: Option<i64>,
    #[serde(default)]
    pub items: Vec<GuildSyncCurrencyItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncArenaTeamMember {
    pub name: String,
    pub rank: Option<i64>,
    pub level: Option<i64>,
    pub class: Option<String>,
    pub online: bool,
    pub played: Option<i64>,
    pub wins: Option<i64>,
    pub season_played: Option<i64>,
    pub season_wins: Option<i64>,
    pub personal_rating: Option<i64>,
}

/// `size` (2/3/5) is what the website maps to "2v2"/"3v3"/"5v5" — the
/// addon has no way to read those strings directly, only team size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncArenaTeam {
    pub name: String,
    pub size: Option<i64>,
    pub rating: Option<i64>,
    pub week_played: Option<i64>,
    pub week_wins: Option<i64>,
    pub season_played: Option<i64>,
    pub season_wins: Option<i64>,
    pub player_played: Option<i64>,
    pub season_player_played: Option<i64>,
    pub rank: Option<i64>,
    pub player_rating: Option<i64>,
    #[serde(default)]
    pub members: Vec<GuildSyncArenaTeamMember>,
}

/// todayHK/todayHonor/yesterdayHK/yesterdayHonor/lifetimeHK/lifetimeRank —
/// confirmed field order live via GetInspectHonorData(). "lifetime" here
/// reads as current-season-only on this server (a level 80 with real past
/// honorable kills came back 0 across the board), so it's complementary
/// to `GuildSyncData::honorable_kills` (the true lifetime total from
/// GetPVPLifetimeStats()), not a replacement for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSyncHonorData {
    pub today_honorable_kills: Option<i64>,
    pub today_honor: Option<i64>,
    pub yesterday_honorable_kills: Option<i64>,
    pub yesterday_honor: Option<i64>,
    pub lifetime_honorable_kills: Option<i64>,
    pub lifetime_rank: Option<i64>,
}

/// One attempt at syncing data to the guild website — recorded for both
/// successes and failures so the Settings > Sync Logs view can show why an
/// auto-sync silently didn't go through (e.g. not signed in, server error),
/// not just that data hasn't updated. See sync_log.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncLogEntry {
    /// Unix epoch milliseconds when the attempt finished.
    pub at: i64,
    /// "characters" | "guild-roster" | "raid"
    pub kind: String,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// Set only for kind "raid" — the HeadCount raid start-time id (see
    /// raid_loot_sync::CapturedRaid). get_captured_raids scans successful
    /// "raid" entries by this to mark a raid as already synced.
    #[serde(default)]
    pub raid_id: Option<String>,
}

/// Parsed straight from the SoTCharacterSync addon's SavedVariables JSON blob.
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
    // Older SavedVariables entries (written before dual-spec equipment
    // existed) won't have a "equipment" key shaped like this at all —
    // default to empty rather than failing to deserialize the whole
    // character.
    #[serde(default)]
    pub equipment: GuildSyncEquipment,
    pub talents: GuildSyncTalents,
    // Older SavedVariables entries (written before this field existed)
    // won't have a "glyphs" key at all — default to empty instead of
    // failing to deserialize the whole character.
    #[serde(default)]
    pub glyphs: GuildSyncGlyphs,
    pub honorable_kills: i64,
    pub achievement_points: i64,
    pub stats: GuildSyncStats,
    pub professions: GuildSyncProfessions,
    // Reputation/achievements/currency/arenaTeams are all newer than the
    // rest of this payload — default to empty/zeroed rather than failing
    // to deserialize a character synced before the addon gathered them.
    #[serde(default)]
    pub reputation: Vec<GuildSyncReputationEntry>,
    #[serde(default)]
    pub achievements: GuildSyncAchievements,
    #[serde(default)]
    pub currency: GuildSyncCurrency,
    #[serde(default)]
    pub arena_teams: Vec<GuildSyncArenaTeam>,
    #[serde(default)]
    pub honor_data: Option<GuildSyncHonorData>,
    pub updated_at: i64,
}
