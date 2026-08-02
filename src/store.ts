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
  WtfAccountRow,
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
  /** Full guild roster from the SoTGuildSync addon — unlike guildSyncData,
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
  /** WTF/Account/* subfolders found locally. */
  wtfAccounts: string[]
  /** Union of local folders and cloud backups, each with sync status —
   *  populated by refreshWtfAccountRows() when the Settings Backup dialog opens. */
  wtfAccountRows: WtfAccountRow[]
  wtfAccountRowsLoading: boolean
  /** Which account (if any) currently has a backup/restore in flight. */
  wtfSyncingAccount: string | null
  /** Which direction wtfSyncingAccount's in-flight operation is - lets the
   *  UI show "Backing up…"/"Restoring…" instead of a generic "Working…". */
  wtfSyncingDirection: 'backup' | 'restore' | null
  wtfSyncError: string
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
    isOfficer: false,
    wtfSyncAutoPull: false,
    wtfSyncAutoPush: false,
    wtfAccountSync: {},
    verboseLogging: false,
    combatLogAutoSync: false,
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
  wtfAccounts: [],
  wtfAccountRows: [],
  wtfAccountRowsLoading: false,
  wtfSyncingAccount: null,
  wtfSyncingDirection: null,
  wtfSyncError: '',
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
let gameClosedUnlisten: (() => void) | null = null

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

  // Rust watches SoTCharacterSync.lua/SoTGuildSync.lua directly (see
  // src-tauri/src/watcher.rs) and emits this the moment either actually
  // changes — no need to poll on a timer for something that only ever
  // changes on real WoW-side events.
  guildDataChangedUnlisten = await api.onGuildDataChanged(() => {
    void refreshGuildSyncData().then(() => maybeAutoSyncGuildData())
    void refreshGuildRoster().then(() => maybeAutoSyncGuildRoster())
  })

  // The game itself can only ever be running on one device at a time, so
  // "it just closed" is as good a signal as any that this device's WTF
  // folders may have just changed and are worth pushing, if the player
  // opted in (see wtf_sync.rs::watch_for_game_close - only armed at all
  // when wtfSyncAutoPush is on).
  gameClosedUnlisten = await api.onGameClosed(() => {
    if (store.settings.wtfSyncAutoPush) {
      void autoSyncAllWtfAccounts('push')
    }
  })

  await refreshManifest()
  await refreshGuildSyncData()
  void maybeAutoSyncGuildData()
  await refreshGuildRoster()
  void maybeAutoSyncGuildRoster()
  void refreshMyLoot()
  void refreshWtfAccounts()

  // Mirrors the game-close trigger: launcher open is the other natural
  // moment to reconcile a device's accounts against the cloud, before the
  // player starts playing on one.
  if (store.settings.wtfSyncAutoPull && store.settings.discordToken) {
    void autoSyncAllWtfAccounts('pull')
  }
}

/** Auto-push/auto-pull apply to every WTF account already known on this
 *  device (not ones only known via a cloud backup from elsewhere - see the
 *  Settings Backup dialog for pulling those down for the first time) that
 *  hasn't been opted out via the dialog's per-account checkbox (an account
 *  with no wtfAccountSync entry at all - never synced yet - is still
 *  included, matching WtfAccountSyncState.autoSyncEnabled's own true
 *  default), one at a time so overlapping confirm dialogs (if several
 *  happen to conflict at once) don't stack on top of each other. */
async function autoSyncAllWtfAccounts(direction: 'push' | 'pull') {
  await refreshWtfAccounts()
  for (const account of store.wtfAccounts) {
    if (store.settings.wtfAccountSync[account]?.autoSyncEnabled === false) continue
    if (direction === 'push') {
      await wtfPushWithConfirm(account)
    } else {
      await wtfPullWithConfirm(account)
    }
  }
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

/** Re-reads the SoTCharacterSync addon's SavedVariables snapshot, if any. */
export async function refreshGuildSyncData() {
  if (!currentGamePath()) {
    store.guildSyncData = []
    return
  }
  try {
    store.guildSyncData = await api.getGuildSyncData()
  } catch (e) {
    // A parse failure here almost always means the file watcher fired
    // while WoW was still mid-write to SoTCharacterSync.lua (e.g. the big
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

/** Re-reads the SoTGuildSync addon's SavedVariables snapshot, if any. */
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
 *  roster, set whenever SoTGuildSync re-saves) rather than a per-character
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

/** Every WTF/Account/* subfolder available under the current game folder. */
export async function refreshWtfAccounts() {
  if (!currentGamePath()) {
    store.wtfAccounts = []
    return
  }
  try {
    store.wtfAccounts = await api.getWtfAccounts()
  } catch {
    store.wtfAccounts = []
  }
}

/** Builds the Settings Backup dialog's table: the union of locally-found account
 *  folders and cloud-known backups (an account pushed from another device
 *  may not have a local folder here yet), each with fresh sync status.
 *  Called when the dialog opens (and its own Refresh button) rather than
 *  kept live in the background like guild data, since it means one status
 *  round-trip per account. */
export async function refreshWtfAccountRows() {
  if (!store.settings.discordToken) {
    store.wtfAccountRows = []
    return
  }
  store.wtfAccountRowsLoading = true
  store.wtfSyncError = ''
  try {
    const [localAccounts, cloudAccounts] = await Promise.all([
      currentGamePath() ? api.getWtfAccounts() : Promise.resolve<string[]>([]),
      api.getWtfCloudAccounts(),
    ])
    store.wtfAccounts = localAccounts
    const localSet = new Set(localAccounts)
    const names = Array.from(new Set([...localAccounts, ...cloudAccounts.map((c) => c.account)])).sort()
    store.wtfAccountRows = await Promise.all(
      names.map(async (account) => {
        const info = await api.getWtfSyncInfo(account)
        return {
          account,
          existsLocally: localSet.has(account),
          status: info.status,
          localChanged: info.localChanged,
        }
      }),
    )
  } catch (e) {
    store.wtfSyncError = String(e)
  } finally {
    store.wtfAccountRowsLoading = false
  }
}

export async function saveWtfSyncAutoPull(enabled: boolean) {
  store.settings = await api.setWtfSyncAutoPull(enabled)
}

export async function saveWtfSyncAutoPush(enabled: boolean) {
  store.settings = await api.setWtfSyncAutoPush(enabled)
}

export async function saveCombatLogAutoSync(enabled: boolean) {
  store.settings = await api.setCombatLogAutoSync(enabled)
}

/** Per-account opt-out from the Settings Backup dialog's checkbox - only
 *  affects wtfSyncAutoPush/AutoPull's automatic sync, not a manual Back
 *  Up/Restore click on the same account. */
export async function saveWtfAccountAutoSyncEnabled(account: string, enabled: boolean) {
  store.settings = await api.setWtfAccountAutoSyncEnabled(account, enabled)
}

/** Pushes one account's local WTF folder to the cloud. Rejects with a
 *  human-readable reason (not a generic Error) if `force` is false and the
 *  cloud copy changed since this device last synced it - see
 *  confirmWtfPush/wtfPushWithConfirm for the confirm-and-retry dance
 *  callers should use instead of calling this directly. */
export async function wtfPushNow(account: string, force = false) {
  store.wtfSyncingAccount = account
  store.wtfSyncingDirection = 'backup'
  store.wtfSyncError = ''
  try {
    store.settings = await api.wtfPush(account, force)
    await refreshWtfAccountRows()
  } catch (e) {
    store.wtfSyncError = String(e)
    throw e
  } finally {
    store.wtfSyncingAccount = null
    store.wtfSyncingDirection = null
  }
}

/** Pulls one account's cloud WTF backup down, replacing its local folder's
 *  contents (creating that folder if this device has never logged into
 *  that account before). Same force/confirm shape as wtfPushNow - see
 *  wtfPullWithConfirm. */
export async function wtfPullNow(account: string, force = false) {
  store.wtfSyncingAccount = account
  store.wtfSyncingDirection = 'restore'
  store.wtfSyncError = ''
  try {
    store.settings = await api.wtfPull(account, force)
    await refreshWtfAccountRows()
  } catch (e) {
    store.wtfSyncError = String(e)
    throw e
  } finally {
    store.wtfSyncingAccount = null
    store.wtfSyncingDirection = null
  }
}

/** One themed confirm dialog for a backup that would overwrite newer cloud changes. */
export function confirmWtfPush(account: string, reason: string): Promise<boolean> {
  return requestConfirm({
    title: `Back up settings — ${account}`,
    message: `${reason}\n\nContinue anyway?`,
    confirmLabel: 'Back Up Anyway',
    danger: true,
  })
}

/** One themed confirm dialog for a restore that would overwrite unsynced local changes. */
export function confirmWtfPull(account: string, reason: string): Promise<boolean> {
  return requestConfirm({
    title: `Restore settings — ${account}`,
    message: `${reason}\n\nContinue anyway?`,
    confirmLabel: 'Restore Anyway',
    danger: true,
  })
}

/** Every conflict wtf_push/wtf_pull refuse over (see commands.rs) mentions
 *  "will overwrite" - other failures (not signed in, game still running)
 *  aren't fixed by retrying with force, so those are just surfaced as a
 *  plain error instead of the confirm-and-retry dialog. */
function isWtfOverwriteConflict(reason: string): boolean {
  return reason.includes('will overwrite')
}

/** Push with the confirm-and-retry dance wired in - shared by the WTF Sync
 *  dialog's manual button and the auto-push-on-game-close trigger, so an
 *  automatic sync still surfaces the same warning a manual one would
 *  rather than silently choosing a side. */
export async function wtfPushWithConfirm(account: string) {
  try {
    await wtfPushNow(account)
  } catch (e) {
    if (isWtfOverwriteConflict(String(e)) && (await confirmWtfPush(account, String(e)))) {
      await wtfPushNow(account, true)
    }
  }
}

/** Same shape as wtfPushWithConfirm, for pull. */
export async function wtfPullWithConfirm(account: string) {
  try {
    await wtfPullNow(account)
  } catch (e) {
    if (isWtfOverwriteConflict(String(e)) && (await confirmWtfPull(account, String(e)))) {
      await wtfPullNow(account, true)
    }
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
  await refreshWtfAccounts()
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

export async function saveVerboseLogging(enabled: boolean) {
  store.settings = await api.setVerboseLogging(enabled)
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
    if (entry.name === name || entry.disabled || entry.deleted) continue
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

/** One themed confirm dialog for moving an addon to Trash. */
export function confirmDelete(name: string): Promise<boolean> {
  const message = `Delete ${name}?${dependentWarning(name, 'Deleting')}\n\nIt will move to Interface/.DeletedAddons - restore it from Trash later, or delete it forever from there.`
  return requestConfirm({ title: 'Delete addon', message, confirmLabel: 'Delete', danger: true })
}

/** One themed confirm dialog for permanently purging a trashed addon — the actual point of no return. */
export function confirmDeleteForever(name: string): Promise<boolean> {
  const message = `Permanently delete ${name}?\n\nThis can't be undone - you'd need to sync it again (or reinstall it by hand, for a custom addon) to get it back.`
  return requestConfirm({ title: 'Delete forever', message, confirmLabel: 'Delete Forever', danger: true })
}

/** One themed confirm dialog for restoring a trashed addon when doing so
 *  requires permanently deleting whatever currently occupies its folder.
 *  `reason` is the raw message from a blocked restoreAddonNow call (already
 *  names the occupant) - shown as-is rather than re-parsed on this end. */
export function confirmRestoreEviction(reason: string): Promise<boolean> {
  const message = `${reason} - this can't be undone.\n\nContinue anyway?`
  return requestConfirm({ title: 'Restore addon', message, confirmLabel: 'Restore anyway', danger: true })
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

/** Immediate move-to-Trash for one addon (managed or custom) - recoverable via restoreAddonNow. */
export async function deleteAddonNow(name: string) {
  if (!store.manifest) return
  try {
    store.installed = await api.deleteAddon(store.manifest, name)
  } catch (e) {
    store.syncError = String(e)
  }
}

/** Permanently purges a trashed addon - the actual point of no return. */
export async function deleteAddonForeverNow(name: string) {
  if (!store.manifest) return
  try {
    store.installed = await api.deleteAddonForever(store.manifest, name)
  } catch (e) {
    store.syncError = String(e)
  }
}

/** Moves a trashed addon back to Interface/AddOns. Rejects with the
 *  occupant's name (not a generic Error) if `force` is false and something
 *  else currently owns its folder, so the caller can show
 *  confirmRestoreEviction and retry with force: true. */
export async function restoreAddonNow(name: string, force = false) {
  if (!store.manifest) return
  try {
    store.installed = await api.restoreAddon(store.manifest, name, force)
  } catch (e) {
    store.syncError = String(e)
    throw e
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

/** Total items a sync would touch — downloads AND addons just waiting to be
 *  moved to .DisabledAddons after being unchecked. Drives whether Play
 *  auto-syncs first; not what the "Update (N)" button label should show,
 *  since "update" is misleading for an addon the user asked to turn off
 *  (see pendingUpdateCount). */
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

/** Same as pendingCount, but only items that actually need downloading —
 *  excludes unchecked addons pending a disable, which aren't an "update"
 *  even though a sync is still needed to move their files. */
export function pendingUpdateCount(): number {
  if (!store.manifest || !store.installed) return 0
  let count = 0
  for (const addon of store.manifest.addons) {
    if (!isAddonWanted(addon)) continue
    const installed = store.installed.addons[addon.name]
    if (!installed || installed.disabled || !installed.filesPresent || installed.sha256 !== addon.sha256) {
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
  gameClosedUnlisten?.()
}
