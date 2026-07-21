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
}

export interface LockEntry {
  version: string
  sha256: string
  files: string[]
}

export interface LockFile {
  addons: Record<string, LockEntry>
  wtfFiles: Record<string, LockEntry>
  addonEnabled: Record<string, boolean>
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
