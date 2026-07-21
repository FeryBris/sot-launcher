import { reactive } from 'vue'
import * as api from './api'
import type { AddonEntry, LockFile, Manifest, Settings, SyncProgress } from './types'

interface Store {
  settings: Settings
  manifest: Manifest | null
  lock: LockFile | null
  /** Explicit user overrides only; absence falls back to the category default. */
  addonEnabled: Record<string, boolean>
  loadingManifest: boolean
  manifestError: string
  syncing: boolean
  syncProgress: SyncProgress | null
  syncError: string
  loaded: boolean
}

export const store = reactive<Store>({
  settings: { gamePath: null, launchCommand: null, closeOnLaunch: false, autoLaunch: false, apiBaseUrl: '', apiKey: null },
  manifest: null,
  lock: null,
  addonEnabled: {},
  loadingManifest: false,
  manifestError: '',
  syncing: false,
  syncProgress: null,
  syncError: '',
  loaded: false,
})

let progressUnlisten: (() => void) | null = null

export async function init() {
  if (store.loaded) return
  store.settings = await api.getSettings()
  store.lock = await api.getLockState()
  store.addonEnabled = { ...store.lock.addonEnabled }
  store.loaded = true

  progressUnlisten = await api.onSyncProgress((p) => {
    store.syncProgress = p
  })

  if (store.settings.apiBaseUrl) {
    await refreshManifest()
  }
}

export async function refreshManifest() {
  store.loadingManifest = true
  store.manifestError = ''
  try {
    store.manifest = await api.fetchManifest(store.settings.apiBaseUrl)
  } catch (e) {
    store.manifestError = String(e)
  } finally {
    store.loadingManifest = false
  }
}

export async function saveGamePath(path: string) {
  store.settings = await api.setGamePath(path)
}

export async function saveLaunchCommand(command: string) {
  store.settings = await api.setLaunchCommand(command)
}

export async function saveCloseOnLaunch(enabled: boolean) {
  store.settings = await api.setCloseOnLaunch(enabled)
}

export async function saveAutoLaunch(enabled: boolean) {
  store.settings = await api.setAutoLaunch(enabled)
}

export async function saveApiBaseUrl(url: string) {
  store.settings = await api.setApiBaseUrl(url)
  await refreshManifest()
}

export async function saveApiKey(key: string) {
  store.settings = await api.setApiKey(key)
  await refreshManifest()
}

/** Whether an addon should be active, applying category defaults when the user hasn't overridden it. */
export function isAddonWanted(addon: AddonEntry): boolean {
  if (addon.category === 'mandatory') return true
  const override = store.addonEnabled[addon.name]
  if (override !== undefined) return override
  return addon.category === 'recommended'
}

export function setAddonEnabled(name: string, enabled: boolean) {
  store.addonEnabled[name] = enabled
}

export async function syncNow() {
  if (!store.manifest) return
  store.syncing = true
  store.syncError = ''
  store.syncProgress = null
  try {
    await api.runSync(store.manifest, store.addonEnabled)
    store.lock = await api.getLockState()
  } catch (e) {
    store.syncError = String(e)
  } finally {
    store.syncing = false
    store.syncProgress = null
  }
}

export function pendingCount(): number {
  if (!store.manifest || !store.lock) return 0
  let count = 0
  for (const addon of store.manifest.addons) {
    const installed = store.lock.addons[addon.name]
    if (isAddonWanted(addon)) {
      if (!installed || installed.version !== addon.version || installed.sha256 !== addon.sha256) {
        count++
      }
    } else if (installed) {
      count++
    }
  }
  for (const wtf of store.manifest.wtfFiles) {
    const installed = store.lock.wtfFiles[wtf.name]
    if (!installed || installed.version !== wtf.version || installed.sha256 !== wtf.sha256) {
      count++
    }
  }
  return count
}

export async function playNow() {
  if (!store.settings.gamePath || store.syncing) return
  if (pendingCount() > 0) {
    await syncNow()
    if (store.syncError) return
  }
  try {
    await api.launchGame()
  } catch (e) {
    store.syncError = String(e)
  }
}

export function disposeStore() {
  progressUnlisten?.()
}
