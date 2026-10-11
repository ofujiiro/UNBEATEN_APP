// Mirrors of what the Rust side returns (unbeaten-core and src-tauri).

export interface Me {
  id: number
  username: string
  avatar_url: string | null
  country: string | null
  total_pp: number
  rank: number
}

export interface Settings {
  server_url: string
  auto_sync: boolean
  start_minimized: boolean
  close_to_tray: boolean
  notifications: boolean
  save_file: string | null
  custom_songs: string | null
}

export interface Detected {
  steam_roots: string[]
  game_dir: string | null
  data_dirs: string[]
  save_file: string | null
  other_saves: string[]
  custom_songs: string | null
  custom_songs_missing: boolean
}

export interface Paths {
  save_file: string | null
  custom_songs: string | null
  game_dir: string | null
  detected: Detected
}

export interface SyncTotals {
  total_pp_before: number
  total_pp: number
  rank_before: number
  rank: number
}

export interface Imported {
  song: string
  beatmap_id: number | null
  pp: number
  previous_pp: number | null
}

export interface Activity {
  at: number
  kind: 'sync' | 'install' | 'game' | 'error'
  message: string
  imported: Imported[]
  totals: SyncTotals | null
}

export interface UpdateInfo {
  version: string
  url: string
}

export interface Snapshot {
  server: string
  signed_in: boolean
  me: Me | null
  settings: Settings
  paths: Paths
  last_sync_at: number | null
  last_totals: SyncTotals | null
  version: string
  game_running: boolean
  syncing: boolean
  tray: boolean
  signing_in: boolean
  autostart: boolean
  update: UpdateInfo | null
  update_required: string | null
  activity: Activity[]
}

export interface ManifestDiff {
  id: number
  name: string
  level: number
  star_rating: number
  object_count: number
  max_pp: number
  filename: string
  chart_hash: string | null
}

export type MapState = 'installed' | 'outdated' | 'modified' | 'not_installed'

export interface MapView {
  id: number
  folder_name: string
  install_folder: string | null
  title: string | null
  artist: string | null
  creator: string | null
  version: number
  cover_url: string | null
  preview_url: string | null
  download_url: string
  unchartable_url: string | null
  difficulties: ManifestDiff[]
  previous_hashes: string[]
  status: { state: MapState; local_folder: string | null; renamed: boolean }
}

export interface ScoreView {
  song: string
  folder: string
  tier: string
  mode: string
  score: number
  accuracy: number
  max_combo: number
  misses: number
  modifier_mask: number
  cleared: boolean
  status: string | null
  pp: number | null
  verified: boolean
  beatmap_id: number | null
}

export interface SyncReport {
  sent: number
  imported: Imported[]
  statuses: Record<string, number>
  baselined: number
  failed_runs: number
  totals: SyncTotals | null
}

export interface LinkStart {
  device_code: string
  user_code: string
  verification_uri: string
  verification_uri_complete: string
  interval: number
  expires_in: number
}

export interface InstallOutcome {
  folder: string
  backup: string | null
}
