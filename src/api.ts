import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  CombinedSyncResult,
  LockFile,
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

export function getLockState(): Promise<LockFile> {
  return invoke('get_lock_state')
}

export function runSync(manifest: Manifest, addonEnabled: Record<string, boolean>): Promise<CombinedSyncResult> {
  return invoke('run_sync', { manifest, addonEnabled })
}

export function launchGame(): Promise<void> {
  return invoke('launch_game')
}

export function onSyncProgress(handler: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>('sync-progress', (event) => handler(event.payload))
}
