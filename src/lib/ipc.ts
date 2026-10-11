// Typed wrappers around the app's Tauri commands (src-tauri/src/commands.rs).

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  InstallOutcome, LinkStart, MapView, ScoreView, Settings, Snapshot, SyncReport,
} from './types'

export const ipc = {
  snapshot: () => invoke<Snapshot>('get_snapshot'),
  authBegin: () => invoke<LinkStart>('auth_begin'),
  authCancel: () => invoke<void>('auth_cancel'),
  signOut: () => invoke<void>('sign_out'),
  redetect: () => invoke<Snapshot>('redetect'),
  saveSettings: (settings: Settings, autostart: boolean) =>
    invoke<Snapshot>('save_settings', { settings, autostart }),
  maps: () => invoke<MapView[]>('maps_list'),
  installMap: (id: number) => invoke<InstallOutcome>('map_install', { id }),
  updateAll: () => invoke<number>('maps_update_all'),
  scores: () => invoke<ScoreView[]>('scores_list'),
  syncNow: () => invoke<SyncReport>('sync_now'),
  launchGame: () => invoke<void>('launch_game'),
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),
}

export type AuthEvent = { status: 'approved' | 'denied' | 'expired' | 'cancelled' }
export type ProgressEvent = { id: number; done: number; total: number | null }

export function on<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null
  let cancelled = false
  listen<T>(event, e => handler(e.payload)).then(fn => {
    if (cancelled) fn()
    else unlisten = fn
  })
  return () => {
    cancelled = true
    unlisten?.()
  }
}
