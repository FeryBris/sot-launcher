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

export function pickGameFolder(): Promise<string | null> {
  return invoke('pick_game_folder')
}

export function fetchManifest(apiBaseUrl: string): Promise<Manifest> {
  return invoke('fetch_manifest', { apiBaseUrl })
}

export function getLockState(): Promise<LockFile> {
  return invoke('get_lock_state')
}

export function runSync(manifest: Manifest, selectedOptional: string[]): Promise<CombinedSyncResult> {
  return invoke('run_sync', { manifest, selectedOptional })
}

export function launchGame(): Promise<void> {
  return invoke('launch_game')
}

export function onSyncProgress(handler: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>('sync-progress', (event) => handler(event.payload))
}
