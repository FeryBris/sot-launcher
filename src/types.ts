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
  gamePath: string | null
  launchCommand: string | null
  closeOnLaunch: boolean
  autoLaunch: boolean
  apiBaseUrl: string
  apiKey: string | null
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
