import { reactive } from 'vue'
import * as api from './api'
import type { AddonEntry, InstalledState, Manifest, Settings, SyncProgress } from './types'

export interface ConfirmRequest {
  title: string
  message: string
  confirmLabel: string
  danger: boolean
}

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

/** Active confirm-dialog request, rendered by <ConfirmDialog> mounted once at the app root. */
export const confirmState = reactive<{ request: ConfirmRequest | null }>({ request: null })
let pendingConfirmResolve: ((value: boolean) => void) | null = null

/** Shows the themed confirm modal and resolves once the user picks an option. */
function requestConfirm(request: ConfirmRequest): Promise<boolean> {
  // Only one confirm dialog can be active at a time; resolve any stale one as cancelled.
  pendingConfirmResolve?.(false)
  return new Promise((resolve) => {
    pendingConfirmResolve = resolve
    confirmState.request = request
  })
}

export function answerConfirm(value: boolean) {
  confirmState.request = null
  pendingConfirmResolve?.(value)
  pendingConfirmResolve = null
}

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

function dependentWarning(name: string, verb: string): string {
  const dependents = getDependents(name)
  if (dependents.length === 0) return ''
  const plural = dependents.length === 1 ? 'requires' : 'require'
  const those = dependents.length === 1 ? 'that addon' : 'those addons'
  return `\n\n${dependents.join(', ')} ${plural} "${name}" to work. ${verb} it will likely break ${those} in-game.`
}

/** One themed confirm dialog for a disable action: always asks, and folds in
 *  a dependents warning when relevant instead of showing a second popup. */
export function confirmDisable(name: string): Promise<boolean> {
  const message = `Disable ${name}?${dependentWarning(name, 'Disabling')}\n\nIt will move to Interface/.DisabledAddons - you can re-enable it later.`
  return requestConfirm({ title: 'Disable addon', message, confirmLabel: 'Disable', danger: false })
}

/** One themed confirm dialog for a permanent delete action. */
export function confirmDelete(name: string): Promise<boolean> {
  const message = `Delete ${name} completely?${dependentWarning(name, 'Deleting')}\n\nThis can't be undone - you'd need to sync it again to get it back.`
  return requestConfirm({ title: 'Delete addon', message, confirmLabel: 'Delete', danger: true })
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
