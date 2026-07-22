import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-shell'
import type {
  CombinedSyncResult,
  InstalledState,
  LutrisGame,
  Manifest,
  Settings,
  SyncProgress,
} from './types'

export function getSettings(): Promise<Settings> {
  return invoke('get_settings')
}

export function setGamePath(path: string): Promise<Settings> {
  return invoke('set_game_path', { path })
}

export function setApiBaseUrl(url: string): Promise<Settings> {
  return invoke('set_api_base_url', { url })
}

export function setApiKey(key: string): Promise<Settings> {
  return invoke('set_api_key', { key })
}

export function setLaunchCommand(command: string): Promise<Settings> {
  return invoke('set_launch_command', { command })
}

export function setCloseOnLaunch(enabled: boolean): Promise<Settings> {
  return invoke('set_close_on_launch', { enabled })
}

export function setAutoLaunch(enabled: boolean): Promise<Settings> {
  return invoke('set_auto_launch', { enabled })
}

export function pickGameFolder(): Promise<string | null> {
  return invoke('pick_game_folder')
}

export function fetchManifest(apiBaseUrl: string): Promise<Manifest> {
  return invoke('fetch_manifest', { apiBaseUrl })
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

export function onSyncProgress(handler: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>('sync-progress', (event) => handler(event.payload))
}

/** Opens the guild site's Discord sign-in page in the system browser. */
export function startDiscordLogin(apiBaseUrl: string): Promise<void> {
  const site = new URL(apiBaseUrl)
  site.pathname = '/launcher/connect'
  site.search = ''
  return open(site.toString())
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
