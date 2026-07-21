import { reactive } from 'vue'
import * as api from './api'
import type { AddonEntry, InstalledState, Manifest, Settings, SyncProgress } from './types'

interface Store {
  settings: Settings
  manifest: Manifest | null
  installed: InstalledState | null
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
  settings: { gamePath: null, launchCommand: null, closeOnLaunch: false, autoLaunch: false, apiBaseUrl: '', apiKey: null, addonEnabled: {} },
  manifest: null,
  installed: null,
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
  store.addonEnabled = { ...store.settings.addonEnabled }
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
    await refreshInstalledState()
  } catch (e) {
    store.manifestError = String(e)
  } finally {
    store.loadingManifest = false
  }
}

/** Re-reads on-disk state (addon markers + WTF file hashes) against the current manifest. */
export async function refreshInstalledState() {
  if (!store.manifest || !store.settings.gamePath) {
    store.installed = null
    return
  }
  try {
    store.installed = await api.getInstalledState(store.manifest)
  } catch {
    store.installed = null
  }
}

export async function saveGamePath(path: string) {
  store.settings = await api.setGamePath(path)
  await refreshInstalledState()
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

/** Names of other currently-enabled tracked addons that require `name` to function. */
export function getDependents(name: string): string[] {
  const target = store.installed?.addons[name]
  if (!target) return []
  const targetFolders = new Set(target.files.map((f) => f.toLowerCase()))
  const dependents: string[] = []
  for (const entry of Object.values(store.installed?.addons ?? {})) {
    if (entry.name === name || entry.disabled) continue
    if (entry.dependencies.some((dep) => targetFolders.has(dep.toLowerCase()))) {
      dependents.push(entry.name)
    }
  }
  return dependents
}

/** Warns if disabling/deleting `name` would break other installed addons that
 *  depend on it. Returns true if there's nothing to warn about or the user
 *  confirmed anyway; false if they backed out. */
export function confirmDependentImpact(name: string, action: 'disable' | 'delete'): boolean {
  const dependents = getDependents(name)
  if (dependents.length === 0) return true
  const verb = action === 'delete' ? 'Deleting' : 'Disabling'
  const plural = dependents.length === 1 ? 'requires' : 'require'
  return confirm(
    `${dependents.join(', ')} ${plural} "${name}" to work.\n\n${verb} it will likely break ${dependents.length === 1 ? 'that addon' : 'those addons'} in-game.\n\nContinue anyway?`,
  )
}

/** Immediate disable/enable for one addon (managed or custom) - moves its folder
 *  to/from Interface/.DisabledAddons right away, no Sync click needed. */
export async function setAddonDisabledNow(name: string, disabled: boolean) {
  if (!store.manifest) return
  try {
    store.installed = await api.setAddonDisabled(store.manifest, name, disabled)
  } catch (e) {
    store.syncError = String(e)
  }
}

/** Immediate, permanent delete for one addon (managed or custom). */
export async function deleteAddonNow(name: string) {
  if (!store.manifest) return
  try {
    store.installed = await api.deleteAddon(store.manifest, name)
  } catch (e) {
    store.syncError = String(e)
  }
}

export async function syncNow() {
  if (!store.manifest) return
  store.syncing = true
  store.syncError = ''
  store.syncProgress = null
  try {
    await api.runSync(store.manifest, store.addonEnabled)
    await refreshInstalledState()
  } catch (e) {
    store.syncError = String(e)
  } finally {
    store.syncing = false
    store.syncProgress = null
  }
}

export function pendingCount(): number {
  if (!store.manifest || !store.installed) return 0
  let count = 0
  for (const addon of store.manifest.addons) {
    const installed = store.installed.addons[addon.name]
    if (isAddonWanted(addon)) {
      if (!installed || installed.disabled || !installed.filesPresent || installed.sha256 !== addon.sha256) {
        count++
      }
    } else if (installed && !installed.disabled) {
      count++
    }
  }
  for (const wtf of store.manifest.wtfFiles) {
    const hash = store.installed.wtfFiles[wtf.name]
    if (!hash || hash !== wtf.sha256) {
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
