export type AddonCategory = 'mandatory' | 'recommended' | 'optional'

export interface AddonEntry {
  name: string
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
  discordToken: string | null
  discordUsername: string | null
  discordAvatar: string | null
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
  disabled: boolean
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
 *  GuildExport addon — visible for the WHOLE guild from any single
 *  member's client, unlike GuildSyncData which only ever covers the
 *  character actually logged in and running SoTGuildSync. */
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
  /** The only source of this for a character that never runs SoTGuildSync itself. */
  achievementPoints: number | null
}

/** Parsed from the GuildExport addon's SavedVariables. */
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

/** Parsed from the SoTGuildSync addon's SavedVariables — used unchanged
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
  equipment: GuildSyncEquipmentItem[]
  talents: GuildSyncTalents
  glyphs: GuildSyncGlyphs
  honorableKills: number
  achievementPoints: number
  stats: GuildSyncStats
  professions: GuildSyncProfessions
  updatedAt: number
}
