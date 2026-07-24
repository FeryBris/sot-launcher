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

export interface GuildSyncTalents {
  firstTree: string | null
  firstPoints: number[]
  secondTree: string | null
  secondPoints: number[]
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
  honorableKills: number
  achievementPoints: number
  stats: GuildSyncStats
  professions: GuildSyncProfessions
  updatedAt: number
}
