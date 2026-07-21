import { reactive } from 'vue'
import * as api from './api'
import type { LockFile, Manifest, Settings, SyncProgress } from './types'

interface Store {
  settings: Settings
  manifest: Manifest | null
  lock: LockFile | null
  selectedOptional: Set<string>
  loadingManifest: boolean
  manifestError: string
  syncing: boolean
  syncProgress: SyncProgress | null
  syncError: string
  loaded: boolean
}

export const store = reactive<Store>({
  settings: { gamePath: null, apiBaseUrl: '' },
  manifest: null,
  lock: null,
  selectedOptional: new Set(),
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
  store.selectedOptional = new Set(store.lock.optionalSelected)
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

export async function saveApiBaseUrl(url: string) {
  store.settings = await api.setApiBaseUrl(url)
  await refreshManifest()
}

export function toggleOptional(name: string) {
  if (store.selectedOptional.has(name)) {
    store.selectedOptional.delete(name)
  } else {
    store.selectedOptional.add(name)
  }
}

export async function syncNow() {
  if (!store.manifest) return
  store.syncing = true
  store.syncError = ''
  store.syncProgress = null
  try {
    await api.runSync(store.manifest, Array.from(store.selectedOptional))
    store.lock = await api.getLockState()
  } catch (e) {
    store.syncError = String(e)
  } finally {
    store.syncing = false
    store.syncProgress = null
  }
}

export function disposeStore() {
  progressUnlisten?.()
}
