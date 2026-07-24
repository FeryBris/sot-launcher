import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-shell'
import type {
  CombinedSyncResult,
  GuildRosterData,
  GuildSyncData,
  InstalledState,
  LutrisGame,
  Manifest,
  MyLootResponse,
  Settings,
  SyncProgress,
} from './types'

export function getSettings(): Promise<Settings> {
  return invoke('get_settings')
}

export function setGamePathWindows(path: string): Promise<Settings> {
  return invoke('set_game_path_windows', { path })
}

export function setGamePathLinux(path: string): Promise<Settings> {
  return invoke('set_game_path_linux', { path })
}

/** The guild site's base URL — safe to expose (not a secret), baked in at
 *  build time. Used only to build browser-open links. */
export function getApiBaseUrl(): Promise<string> {
  return invoke('get_api_base_url')
}

export function setLaunchCommand(command: string): Promise<Settings> {
  return invoke('set_launch_command', { command })
}

export function setHideOnLaunch(enabled: boolean): Promise<Settings> {
  return invoke('set_hide_on_launch', { enabled })
}

export function setCloseToTray(enabled: boolean): Promise<Settings> {
  return invoke('set_close_to_tray', { enabled })
}

export function setAutoLaunch(enabled: boolean): Promise<Settings> {
  return invoke('set_auto_launch', { enabled })
}

export function pickGameFolder(): Promise<string | null> {
  return invoke('pick_game_folder')
}

export function fetchManifest(): Promise<Manifest> {
  return invoke('fetch_manifest')
}

export function getInstalledState(manifest: Manifest): Promise<InstalledState> {
  return invoke('get_installed_state', { manifest })
}

export function setAddonDisabled(manifest: Manifest, name: string, disabled: boolean): Promise<InstalledState> {
  return invoke('set_addon_disabled', { manifest, name, disabled })
}

export function deleteAddon(manifest: Manifest, name: string): Promise<InstalledState> {
  return invoke('delete_addon', { manifest, name })
}

export function runSync(manifest: Manifest, addonEnabled: Record<string, boolean>): Promise<CombinedSyncResult> {
  return invoke('run_sync', { manifest, addonEnabled })
}

export function launchGame(): Promise<void> {
  return invoke('launch_game')
}

export function listLutrisGames(): Promise<LutrisGame[]> {
  return invoke('list_lutris_games')
}

export function getGuildSyncData(): Promise<GuildSyncData[]> {
  return invoke('get_guild_sync_data')
}

export function syncGuildDataToWeb(): Promise<void> {
  return invoke('sync_guild_data_to_web')
}

export function getGuildRoster(): Promise<GuildRosterData | null> {
  return invoke('get_guild_roster')
}

export function syncGuildRosterToWeb(): Promise<void> {
  return invoke('sync_guild_roster_to_web')
}

export function getMyLoot(): Promise<MyLootResponse> {
  return invoke('get_my_loot')
}

export function onSyncProgress(handler: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>('sync-progress', (event) => handler(event.payload))
}

/** Fires whenever SoTGuildSync.lua or GuildExport.lua actually changes on
 *  disk (see src-tauri/src/watcher.rs) — replaces polling on a timer. */
export function onGuildDataChanged(handler: () => void): Promise<UnlistenFn> {
  return listen('guild-data-changed', () => handler())
}

/** Opens the guild site's Discord sign-in page in the system browser. */
export function startDiscordLogin(apiBaseUrl: string): Promise<void> {
  const site = new URL(apiBaseUrl)
  site.pathname = '/launcher/connect'
  site.search = ''
  return open(site.toString())
}

/** Opens a character's public page on the guild site in the system browser. */
export function openCharacterPage(apiBaseUrl: string, name: string): Promise<void> {
  const site = new URL(apiBaseUrl)
  site.pathname = `/character/${encodeURIComponent(name)}`
  site.search = ''
  return open(site.toString())
}

/** Opens an item's Wowhead page (WotLK client DB) in the system browser. */
export function openWowheadItem(wowheadId: number): Promise<void> {
  return open(`https://www.wowhead.com/wotlk/item=${wowheadId}`)
}

export function pairWithCode(code: string): Promise<Settings> {
  return invoke('pair_with_code', { code })
}

export function clearDiscordLogin(): Promise<Settings> {
  return invoke('clear_discord_login')
}

export function onDiscordPaired(handler: (s: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>('discord-paired', (event) => handler(event.payload))
}

export function onDiscordPairError(handler: (message: string) => void): Promise<UnlistenFn> {
  return listen<string>('discord-pair-error', (event) => handler(event.payload))
}

/** Relabels the tray menu's first item ("Play" <-> "Update"). */
export function setTrayPrimaryLabel(label: string): Promise<void> {
  return invoke('set_tray_primary_label', { label })
}

/** Fired when the tray menu's first item is clicked — the frontend decides
 *  whether that means installUpdate() or playNow() based on its own
 *  update-available state. */
export function onTrayPrimaryClicked(handler: () => void): Promise<UnlistenFn> {
  return listen('tray-primary-clicked', () => handler())
}
