export type AddonCategory = 'mandatory' | 'recommended' | 'optional'

export interface AddonEntry {
  name: string
  description: string
  version: string
  url: string
  sha256: string
  category: AddonCategory
  changelog: string
}

export interface WtfEntry {
  name: string
  version: string
  url: string
  sha256: string
  targetPath: string
}

export interface NewsItem {
  title: string
  body: string
  date: string
}

export interface Manifest {
  /** The game the list is for (missing from sites before per-game lists). */
  client?: string | null
  addons: AddonEntry[]
  wtfFiles: WtfEntry[]
  news: NewsItem[]
}

export interface Settings {
  gamePathWindows: string | null
  gamePathLinux: string | null
  launchCommand: string | null
  hideOnLaunch: boolean
  closeToTray: boolean
  autoLaunch: boolean
  addonEnabled: Record<string, boolean>
  /** Whether a Discord pairing token is currently held in the OS keychain —
   *  the actual token never reaches the frontend, only this presence flag. */
  discordSignedIn: boolean
  discordUsername: string | null
  discordAvatar: string | null
  /** Whether the paired Discord user holds officer guild rank, as of the last pairing exchange. */
  isOfficer: boolean
  /** Applies to every detected WTF/Account/<name> folder, not just one —
   *  each account syncs to its own cloud slot rather than picking one as
   *  "the" account (see the Settings Backup dialog). */
  wtfSyncAutoPull: boolean
  wtfSyncAutoPush: boolean
  /** Per-account sync bookkeeping, keyed by WTF/Account/<name> - only ever
   *  has an entry for an account that's been pushed/pulled at least once. */
  wtfAccountSync: Record<string, WtfAccountSyncState>
  /** When on, debug.json records every action the launcher takes, not just
   *  errors — see Settings > Sync Logs' "Log everything" checkbox. */
  verboseLogging: boolean
  /** The game the launcher is showing. The per-game fields above (game
   *  paths, launch command, addonEnabled, wtfAccountSync) always hold THIS
   *  game's values; the other game's sit in storedProfiles (Rust:
   *  Settings::switch_client). */
  activeClient: GameClient
  storedProfiles: Partial<Record<GameClient, GameProfile>>
  /** False until the player picks a game on the first-open picker. */
  clientChosen: boolean
}

/** Also the `client` value the website stores with every upload. */
export type GameClient = 'wotlk' | 'forever'

/** The per-game settings of the game that is NOT active. */
export interface GameProfile {
  gamePathWindows: string | null
  gamePathLinux: string | null
  launchCommand: string | null
  addonEnabled: Record<string, boolean>
  wtfAccountSync: Record<string, WtfAccountSyncState>
}

/** Per-account WTF sync bookkeeping — see Settings.wtfAccountSync. */
export interface WtfAccountSyncState {
  lastSyncedFingerprint: string | null
  lastKnownRemoteUpdatedAt: number | null
  lastSyncedAt: number | null
  /** Whether this account participates in wtfSyncAutoPush/AutoPull - an
   *  account with no entry at all (never synced) is still enabled, same as
   *  this field's own true default once an entry does exist. */
  autoSyncEnabled: boolean
}

/** The paired user's personal WTF cloud backup state — distinct from
 *  Manifest.wtfFiles, which are guild-wide admin-managed template files
 *  pushed one-directionally to everyone. */
export interface WtfSyncStatus {
  exists: boolean
  updatedAt: number | null
  size: number | null
}

export interface WtfSyncInfo {
  status: WtfSyncStatus
  /** Whether the local account folder has changed since the last successful push/pull. */
  localChanged: boolean
}

/** One cloud-known WTF account backup — unlike WtfSyncStatus (one specific
 *  account's status), this is the full list, so the sync table can offer a
 *  Pull for an account backed up from another device that hasn't logged
 *  into this one yet (its WTF/Account/<name> folder wouldn't exist locally). */
export interface WtfCloudAccountEntry {
  account: string
  updatedAt: number | null
  size: number | null
}

/** One row in the Settings Backup dialog's table — the union of local folders
 *  (existsLocally) and cloud backups (status.exists), so an account synced
 *  from another device but never logged into here still shows up, pullable. */
export interface WtfAccountRow {
  account: string
  existsLocally: boolean
  status: WtfSyncStatus
  localChanged: boolean
}

export interface AddonStateEntry {
  name: string
  /** 'mandatory' | 'recommended' | 'optional' | 'custom' */
  type: AddonCategory | 'custom'
  /** version read live from the addon's own .toc file on disk */
  version: string
  /** version the guild manifest currently declares; absent for custom addons */
  apiVersion: string | null
  sha256: string | null
  notes: string
  files: string[]
  filesPresent: boolean
  /** In Interface/.DisabledAddons instead of Interface/AddOns. Always false once `deleted` is true. */
  disabled: boolean
  /** In Interface/.DeletedAddons (Trash) — recoverable via restore, or purged via delete forever. */
  deleted: boolean
  /** Folder names of other addons this one requires, read from its .toc. */
  dependencies: string[]
}

export interface InstalledState {
  addons: Record<string, AddonStateEntry>
  /** addon/wtf name -> sha256 currently on disk */
  wtfFiles: Record<string, string>
}

export interface SyncProgress {
  item: string
  stage: 'downloading' | 'verifying' | 'extracting' | 'done' | 'error'
  current: number
  total: number
  message: string | null
}

export interface SyncResult {
  updated: string[]
  skipped: string[]
  removed: string[]
  failed: [string, string][]
}

export interface CombinedSyncResult {
  addons: SyncResult
  wtfFiles: SyncResult
}

export interface LutrisGame {
  id: number
  slug: string
  name: string
  runner: string
  launchCommand: string
}

/** name/level/quality/itemType/itemSubType/equipLocType/equipLocName/icon
 *  come straight from the addon's own GetItemInfo() read of the game's
 *  item cache — absent when the client hasn't cached that item's tooltip
 *  data yet. */
export interface GuildSyncEquipmentItem {
  itemId: number
  enchantId: number
  gemIds: number[]
  name: string | null
  level: number | null
  quality: string | null
  itemType: string | null
  itemSubType: string | null
  equipLocType: string | null
  equipLocName: string | null
  icon: string | null
}

/** Deliberately not camelCase, matching GuildSyncGlyphs — equipment is tied
 *  to the active dual-spec loadout the same way talents and glyphs are
 *  (same "can't read the inactive group" server limitation): a character
 *  can gear up differently per spec, so first_equipment/second_equipment
 *  follow the identical convention as first_glyphs/second_glyphs —
 *  whichever spec is active gets refreshed, the other slot carries
 *  forward its last-known snapshot. */
export interface GuildSyncEquipment {
  first_equipment: GuildSyncEquipmentItem[]
  second_equipment: GuildSyncEquipmentItem[]
}

/** Deliberately not camelCase — the addon's own collectTalents() writes
 *  these 4 fields as snake_case already, unlike the rest of GuildSyncData
 *  (matches the Rust side's GuildSyncTalents, which isn't renamed either).
 *  first_points/second_points hold every one of the class's 3 trees' full
 *  per-talent point arrays (in tab order), not just the dominant tree —
 *  WotLK dual spec means first/second are two independent full loadouts,
 *  each of which can spend points across all 3 trees at once. */
export interface GuildSyncTalents {
  first_tree: string | null
  first_points: number[][]
  second_tree: string | null
  second_points: number[][]
  /** Which dual-spec group (1 or 2) was actually active in-game during
   *  this collection — first_points/second_points look identical whether
   *  freshly collected or carried forward, so the website needs this to
   *  know which one is current. */
  active_group: number
}

/** `id` is the glyph EFFECT's spell ID (what GetGlyphSocketInfo reports),
 *  not an item ID. */
export interface GuildSyncGlyph {
  id: number
  name: string | null
  type: 'major' | 'minor' | null
}

/** Deliberately not camelCase, matching GuildSyncTalents — glyphs are
 *  tied to the active dual-spec loadout the same way talents are (same
 *  "can't read the inactive group" server limitation), so first_glyphs/
 *  second_glyphs follow the identical convention: whichever spec is
 *  active gets refreshed, the other slot carries forward its last-known
 *  snapshot. */
export interface GuildSyncGlyphs {
  first_glyphs: GuildSyncGlyph[]
  second_glyphs: GuildSyncGlyph[]
}

export interface GuildSyncProfessions {
  first: string | null
  firstSkill: number | null
  second: string | null
  secondSkill: number | null
  firstAid: number | null
  cooking: number | null
  fishing: number | null
}

/** Field names match `character_stats`' columns 1:1 (snake_case, not
 *  renamed) — same reason as the Rust side, see guild_sync.rs. */
export interface GuildSyncStats {
  strength: number | null
  agility: number | null
  stamina: number | null
  intellect: number | null
  spirit: number | null
  armor: number | null
  health: number | null
  mana: number | null
  attack_power: number | null
  melee_crit: number | null
  melee_haste: number | null
  melee_hit: number | null
  expertise: number | null
  armor_penetration: number | null
  ranged_attack_power: number | null
  ranged_crit: number | null
  ranged_hit: number | null
  spell_power: number | null
  spell_crit: number | null
  spell_haste: number | null
  spell_hit: number | null
  spell_penetration: number | null
  mana_regen: number | null
  dodge: number | null
  parry: number | null
  block: number | null
  block_value: number | null
  defense_rating: number | null
  resilience: number | null
  resistance_arcane: number | null
  resistance_fire: number | null
  resistance_frost: number | null
  resistance_nature: number | null
  resistance_shadow: number | null
}

/** One guild member as reported by GetGuildRosterInfo() via the
 *  SoTGuildSync addon — visible for the WHOLE guild from any single
 *  member's client, unlike GuildSyncData which only ever covers the
 *  character actually logged in and running SoTCharacterSync. */
export interface GuildRosterMember {
  name: string
  class: string | null
  level: number | null
  guildRank: string | null
  guildRankIndex: number | null
  zone: string | null
  publicNote: string | null
  officerNote: string | null
  isOnline: boolean | null
  /** The only source of this for a character that never runs SoTCharacterSync itself. */
  achievementPoints: number | null
}

/** Parsed from the SoTGuildSync addon's SavedVariables. */
export interface GuildRosterData {
  guildName: string | null
  realm: string | null
  updatedAt: number
  members: GuildRosterMember[]
}

/** Website's DateFormatterHelper::get() shape — only the fields actually
 *  used here are listed, the rest pass through untyped. */
export interface FormattedDate {
  stringDate?: string
  time?: string
  formatted?: string
  /** Zero-padded "DD", "MM", "YYYY" — used for the compact loot-date box. */
  day?: string
  month?: string
  year?: string
  [key: string]: unknown
}

/** Website's ItemResource shape, as returned by /api/launcher/my-loot. */
export interface MyLootItem {
  id: number
  wowheadId: number
  name: string
  level: number | null
  /** Media relation is snake_case on the website's side (unlike the rest
   *  of this payload) — `full_path` is the actual icon image URL. No
   *  third-party fallback when it's missing: a local blank placeholder
   *  is used instead (see MyLoot.vue's itemIconSrc). */
  media: { full_path?: string } | null
  quality: string | null
  class: string | null
  subclass: string | null
  type: string | null
}

export interface MyLootEntry {
  id: number
  item: MyLootItem
  givenAt: FormattedDate | null
  /** Not collapsed — every loot of this item still gets its own entry —
   *  but if this item was looted more than once, the total count is
   *  attached here on its last (oldest) occurrence in the list, null on
   *  every other occurrence (including single, un-repeated loots). */
  count: number | null
  response: { id: number | null; label: string | null; name: string | null }
}

/** Loot grouped by instance (not by individual raid night — one raid can
 *  span several instances, and the same instance gets farmed across many
 *  raid nights). */
export interface MyLootInstance {
  id: number | null
  name: string
  /** Same abbreviation convention as the website's InstancePortrait.vue
   *  (e.g. "IC", "ToGC", "Naxx") — used to look up a banner image and as
   *  the sidebar's compact label. */
  abbreviation: string | null
  /** e.g. "Tier 9" — shown alongside the full name above the item list. */
  tier: string | null
  lastLootedAt: FormattedDate | null
  loots: MyLootEntry[]
}

export interface MyLootCharacter {
  name: string
  instances: MyLootInstance[]
}

export interface MyLootResponse {
  characters: MyLootCharacter[]
}

/** One recorded attempt at syncing data to the guild website — see
 *  Settings > Sync Logs. `error` is set only when `success` is false. */
export interface SyncLogEntry {
  /** Unix epoch milliseconds. */
  at: number
  kind: 'characters' | 'guild-roster'
  success: boolean
  error: string | null
}

/** One entry from the persistent error/warning log (debug.json) — active
 *  in every build, unlike the dev-only console log, so this is what
 *  actually has something in it after a shipped release fails. */
export interface DebugLogEntry {
  /** Local time, human-readable (e.g. "2026-07-31 14:32:05"). */
  at: string
  level: 'error' | 'warn' | 'info'
  /** Which command/operation this came from, e.g. "wtf_push". */
  context: string
  message: string
}

/** One raid the SoTRaidLootSync addon has captured (recent ones only —
 *  see that addon's own age window). raidJson/lootJson are opaque JSON
 *  strings (HeadCount's and RCLootCouncil's own export formats) — the
 *  launcher never parses their contents beyond what's needed to show a
 *  human-readable summary, the website's importers own that. lootJson is
 *  null for a raid with no RCLootCouncil data, which is a normal case. */
export interface SyncableRaid {
  raidId: string
  capturedAt: number
  raidJson: string
  lootJson: string | null
  alreadySynced: boolean
  isIgnored: boolean
}

/** One completed raw-log upload chunk (see combat_log_chunker.rs) — one
 *  whole play session's worth of raw WoWCombatLog.txt lines (closed only
 *  by real wall-clock idle time, no size limit - an earlier version
 *  targeted a fixed ~30 minutes instead), not one per boss pull the way
 *  this used to work. No player/boss/content-type summary is computed
 *  client-side anymore — PullSegmenter/CombatBreakdownService recompute
 *  all of that server-side from the raw text once this chunk is synced. */
export interface CombatLogSession {
  sessionId: string
  startTimeMs: number
  endTimeMs: number
  lineCount: number
}

/** get_captured_combat_sessions' return shape — the flattened
 *  CombatLogSession fields plus sync/ignore status, same
 *  already-synced/is-ignored convention as SyncableRaid above. */
export interface SyncableCombatLogSession extends CombatLogSession {
  alreadySynced: boolean
  isIgnored: boolean
}

/** One character from the active game's SoTCharacterSync, as the UI uses
 *  it. WotLK entries are full GuildSyncData; Forever entries are the addon's
 *  own JSON (Rust: commands.rs read_characters) - only these fields are
 *  common to both. */
export interface SyncedCharacter {
  name: string
  class: string
  race: string
  gender: string | null
  guildRank: string | null
  guildRankIndex: number | null
  level: number
  updatedAt: number
  /** Which WTF/Account/<name> folder this character came from. */
  account: string
}

/** Parsed from the SoTCharacterSync addon's SavedVariables — used unchanged
 *  for both local display and the sync-to-web payload. */
export interface GuildSyncData {
  name: string
  class: string
  race: string
  gender: string | null
  guildRank: string | null
  guildRankIndex: number | null
  level: number
  gold: number
  equipment: GuildSyncEquipment
  talents: GuildSyncTalents
  glyphs: GuildSyncGlyphs
  honorableKills: number
  achievementPoints: number
  stats: GuildSyncStats
  professions: GuildSyncProfessions
  updatedAt: number
  /** Which local WTF/Account/<name> folder this character was read from —
   *  a launcher-local concern (never sent to the website), used so Home can
   *  show which WoW account each character belongs to when a player has
   *  more than one under this install. */
  account: string
}
