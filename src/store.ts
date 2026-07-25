import { reactive } from 'vue'
import * as api from './api'
import { isWindows } from './platform'
import type {
  AddonEntry,
  GuildRosterData,
  GuildSyncData,
  InstalledState,
  Manifest,
  MyLootResponse,
  Settings,
  SyncProgress,
} from './types'

export interface ConfirmRequest {
  title: string
  message: string
  confirmLabel: string
  danger: boolean
}

interface Store {
  settings: Settings
  /** The guild site's base URL, baked in at build time (see src-tauri/.env)
   *  and fetched once via get_api_base_url — not part of Settings since
   *  it's not player-editable. */
  apiBaseUrl: string
  manifest: Manifest | null
  installed: InstalledState | null
  /** Explicit user overrides only; absence falls back to the category default. */
  addonEnabled: Record<string, boolean>
  loadingManifest: boolean
  manifestError: string
  syncing: boolean
  syncProgress: SyncProgress | null
  syncError: string
  discordError: string
  loaded: boolean
  guildSyncData: GuildSyncData[]
  guildSyncing: boolean
  guildSyncError: string
  guildLastSyncedAt: number | null
  /** Full guild roster from the GuildExport addon — unlike guildSyncData,
   *  not scoped to characters logged into on this WoW account. */
  guildRoster: GuildRosterData | null
  guildRosterSyncing: boolean
  guildRosterSyncError: string
  guildRosterLastSyncedAt: number | null
  /** The signed-in user's own characters' loot, grouped by raid — fetched
   *  once (not polled; raid loot only ever changes after a raid night). */
  myLoot: MyLootResponse | null
  myLootLoading: boolean
  myLootError: string
}

export const store = reactive<Store>({
  settings: {
    gamePathWindows: null,
    gamePathLinux: null,
    launchCommand: null,
    hideOnLaunch: true,
    closeToTray: true,
    autoLaunch: false,
    addonEnabled: {},
    discordToken: null,
    discordUsername: null,
    discordAvatar: null,
  },
  apiBaseUrl: '',
  manifest: null,
  installed: null,
  addonEnabled: {},
  loadingManifest: false,
  manifestError: '',
  syncing: false,
  syncProgress: null,
  syncError: '',
  discordError: '',
  loaded: false,
  guildSyncData: [],
  guildSyncing: false,
  guildSyncError: '',
  guildLastSyncedAt: null,
  guildRoster: null,
  guildRosterSyncing: false,
  guildRosterSyncError: '',
  guildRosterLastSyncedAt: null,
  myLoot: null,
  myLootLoading: false,
  myLootError: '',
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
let discordPairedUnlisten: (() => void) | null = null
let discordPairErrorUnlisten: (() => void) | null = null
let guildDataChangedUnlisten: (() => void) | null = null

const AUTO_SYNC_DEBOUNCE_MS = 60_000
let lastAutoSyncSnapshot = ''
let lastAutoSyncAt = 0

/** name:updatedAt pairs, sorted so member order never causes a spurious
 *  "changed" result — only real per-character data changes should. */
function guildSyncSnapshot(data: typeof store.guildSyncData): string {
  return data
    .map((c) => `${c.name}:${c.updatedAt}`)
    .sort()
    .join('|')
}

/** Auto-syncs to the website when the local data actually changed since
 *  the last successful sync, debounced so a burst of gold/gear changes
 *  (e.g. vendoring during a raid) doesn't fire a request every poll tick.
 *  A failed attempt doesn't update the snapshot, so it's retried on the
 *  next poll once the debounce window passes. */
async function maybeAutoSyncGuildData() {
  if (!store.settings.discordToken) return
  if (store.guildSyncing) return

  const snapshot = guildSyncSnapshot(store.guildSyncData)
  if (snapshot === lastAutoSyncSnapshot) return
  if (Date.now() - lastAutoSyncAt < AUTO_SYNC_DEBOUNCE_MS) return

  lastAutoSyncAt = Date.now()
  await syncGuildDataToWeb()
  if (!store.guildSyncError) {
    lastAutoSyncSnapshot = snapshot
    store.guildLastSyncedAt = Date.now()
  }
}

/** The game path for whichever OS this is actually running on — same
 *  Windows/Linux split as the Rust side (Settings.game_path()), just
 *  determined client-side since the webview already knows its own OS. */
export function currentGamePath(): string | null {
  return isWindows() ? store.settings.gamePathWindows : store.settings.gamePathLinux
}

export async function init() {
  if (store.loaded) return
  store.settings = await api.getSettings()
  store.apiBaseUrl = await api.getApiBaseUrl()
  store.addonEnabled = { ...store.settings.addonEnabled }
  store.loaded = true

  progressUnlisten = await api.onSyncProgress((p) => {
    store.syncProgress = p
  })

  // The OS hands sot-launcher://pair?code=... to the Rust side, not directly
  // to this webview, so the exchanged result arrives here as an event
  // instead of a normal invoke() response.
  discordPairedUnlisten = await api.onDiscordPaired((s) => {
    store.settings = s
    store.discordError = ''
    void maybeAutoSyncGuildData()
    void maybeAutoSyncGuildRoster()
    void refreshMyLoot()
  })
  discordPairErrorUnlisten = await api.onDiscordPairError((message) => {
    store.discordError = message
  })

  // Rust watches SoTGuildSync.lua/GuildExport.lua directly (see
  // src-tauri/src/watcher.rs) and emits this the moment either actually
  // changes — no need to poll on a timer for something that only ever
  // changes on real WoW-side events.
  guildDataChangedUnlisten = await api.onGuildDataChanged(() => {
    void refreshGuildSyncData().then(() => maybeAutoSyncGuildData())
    void refreshGuildRoster().then(() => maybeAutoSyncGuildRoster())
  })

  await refreshManifest()
  await refreshGuildSyncData()
  void maybeAutoSyncGuildData()
  await refreshGuildRoster()
  void maybeAutoSyncGuildRoster()
  void refreshMyLoot()
}

/** Fetches the signed-in user's own loot, grouped by raid — not polled,
 *  since raid loot only ever changes after a raid night; the "My Loot"
 *  tab can call this again to manually refresh. */
export async function refreshMyLoot() {
  if (!store.settings.discordToken) {
    store.myLoot = null
    return
  }
  store.myLootLoading = true
  store.myLootError = ''
  try {
    store.myLoot = await api.getMyLoot()
  } catch (e) {
    store.myLootError = String(e)
  } finally {
    store.myLootLoading = false
  }
}

/** Re-reads the SoTGuildSync addon's SavedVariables snapshot, if any. */
export async function refreshGuildSyncData() {
  if (!currentGamePath()) {
    store.guildSyncData = []
    return
  }
  try {
    store.guildSyncData = await api.getGuildSyncData()
  } catch (e) {
    // A parse failure here almost always means the file watcher fired
    // while WoW was still mid-write to SoTGuildSync.lua (e.g. the big
    // flush on logout) and caught a truncated file — not a real "no data"
    // state. Keep whatever was last successfully read rather than
    // flashing the UI back to empty; the next file-changed event (or app
    // restart) will pick up the completed write.
    console.warn('Failed to read guild sync data, keeping last known data', e)
  }
}

async function syncGuildDataToWeb() {
  store.guildSyncing = true
  store.guildSyncError = ''
  try {
    await api.syncGuildDataToWeb()
  } catch (e) {
    store.guildSyncError = String(e)
  } finally {
    store.guildSyncing = false
  }
}

/** Re-reads the GuildExport addon's SavedVariables snapshot, if any. */
export async function refreshGuildRoster() {
  if (!currentGamePath()) {
    store.guildRoster = null
    return
  }
  try {
    store.guildRoster = await api.getGuildRoster()
  } catch {
    store.guildRoster = null
  }
}

let lastAutoRosterSyncAt = 0
let lastAutoRosterSyncedAt = 0

/** Same debounced-auto-sync shape as maybeAutoSyncGuildData, but keyed off
 *  the roster snapshot's own `updatedAt` (a single timestamp for the whole
 *  roster, set whenever GuildExport re-saves) rather than a per-character
 *  join — there's no per-member identity to track here, just "has the
 *  addon written a newer snapshot than what we last sent". */
async function maybeAutoSyncGuildRoster() {
  if (!store.settings.discordToken) return
  if (store.guildRosterSyncing) return
  if (!store.guildRoster || store.guildRoster.members.length === 0) return

  if (store.guildRoster.updatedAt === lastAutoRosterSyncedAt) return
  if (Date.now() - lastAutoRosterSyncAt < AUTO_SYNC_DEBOUNCE_MS) return

  lastAutoRosterSyncAt = Date.now()
  const syncedUpdatedAt = store.guildRoster.updatedAt
  await syncGuildRosterToWeb()
  if (!store.guildRosterSyncError) {
    lastAutoRosterSyncedAt = syncedUpdatedAt
    store.guildRosterLastSyncedAt = Date.now()
  }
}

async function syncGuildRosterToWeb() {
  store.guildRosterSyncing = true
  store.guildRosterSyncError = ''
  try {
    await api.syncGuildRosterToWeb()
  } catch (e) {
    store.guildRosterSyncError = String(e)
  } finally {
    store.guildRosterSyncing = false
  }
}

/** Opens the guild site's Discord sign-in in the system browser; the result
 *  comes back asynchronously via the discord-paired event, not this call. */
export async function signInWithDiscord() {
  store.discordError = ''
  try {
    await api.startDiscordLogin(store.apiBaseUrl)
  } catch (e) {
    store.discordError = String(e)
  }
}

export async function signOutDiscord() {
  try {
    store.settings = await api.clearDiscordLogin()
  } catch (e) {
    store.discordError = String(e)
  }
}

export async function refreshManifest() {
  store.loadingManifest = true
  store.manifestError = ''
  try {
    store.manifest = await api.fetchManifest()
    await refreshInstalledState()
  } catch (e) {
    store.manifestError = String(e)
  } finally {
    store.loadingManifest = false
  }
}

/** Re-reads on-disk state (addon markers + WTF file hashes) against the current manifest. */
export async function refreshInstalledState() {
  if (!store.manifest || !currentGamePath()) {
    store.installed = null
    return
  }
  try {
    store.installed = await api.getInstalledState(store.manifest)
  } catch {
    store.installed = null
  }
}

export async function saveGamePath(path: string, platform: 'windows' | 'linux') {
  store.settings = platform === 'windows' ? await api.setGamePathWindows(path) : await api.setGamePathLinux(path)
  await refreshInstalledState()
  await refreshGuildSyncData()
}

export async function saveLaunchCommand(command: string) {
  store.settings = await api.setLaunchCommand(command)
}

export async function saveHideOnLaunch(enabled: boolean) {
  store.settings = await api.setHideOnLaunch(enabled)
}

export async function saveCloseToTray(enabled: boolean) {
  store.settings = await api.setCloseToTray(enabled)
}

export async function saveAutoLaunch(enabled: boolean) {
  store.settings = await api.setAutoLaunch(enabled)
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
    const result = await api.runSync(store.manifest, store.addonEnabled)
    // run_sync only rejects on a whole-command failure (e.g. can't create
    // a directory) — a single addon/WTF file failing (bad checksum, a
    // dead download URL) is reported per-item in these `failed` arrays
    // instead, so it has to be checked explicitly or it's silently lost.
    const failed = [...result.addons.failed, ...result.wtfFiles.failed]
    if (failed.length > 0) {
      store.syncError = failed.map(([name, reason]) => `${name}: ${reason}`).join('; ')
    }
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
  if (!currentGamePath() || store.syncing) return
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
  discordPairedUnlisten?.()
  discordPairErrorUnlisten?.()
  guildDataChangedUnlisten?.()
}
