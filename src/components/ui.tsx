import type { ReactNode } from 'react'
import type { MapState } from '../lib/types'

export function Toggle({ on, onChange, label, help, disabled }: {
  on: boolean; onChange: (v: boolean) => void; label: string; help?: ReactNode; disabled?: boolean
}) {
  return (
    <div className={`toggle ${disabled ? 'disabled' : ''}`} onClick={() => !disabled && onChange(!on)}
      role="switch" aria-checked={on}>
      <div className={`switch ${on ? 'on' : ''}`} />
      <div>
        <div className="field-label">{label}</div>
        {help && <div className="field-help">{help}</div>}
      </div>
    </div>
  )
}

const MAP_BADGE: Record<MapState, [string, string]> = {
  installed: ['Installed', 'b-green'],
  outdated: ['Update', 'b-yellow'],
  modified: ['Differs', 'b-red'],
  not_installed: ['Not installed', 'b-muted'],
}

export function MapBadge({ state }: { state: MapState }) {
  const [label, cls] = MAP_BADGE[state]
  return <span className={`badge ${cls}`}>{label}</span>
}

// Sync outcomes (server statuses plus the launcher's own).
const SCORE_BADGE: Record<string, [string, string, string]> = {
  imported: ['Synced', 'b-green', 'Stored on UNBEATEN.'],
  no_change: ['Synced', 'b-green', 'UNBEATEN already has this score or a better one.'],
  unmatched: ['Not ranked', 'b-muted', "This chart isn't ranked on UNBEATEN (or your copy doesn't match it)."],
  outdated_chart: ['Old chart', 'b-yellow', 'Played on an older version of the ranked chart. Update the map, then play it again.'],
  modified_chart: ['Edited chart', 'b-red', "Your copy of this chart differs from the ranked one, so the score can't count. Reinstall the map."],
  failed_run: ['Failed run', 'b-muted', "Songs you didn't clear don't count."],
  baseline: ['Before update', 'b-muted', 'Set before this chart was installed or changed, so it may be from another version. Play it again to sync.'],
  official: ['Official', 'b-muted', 'Official songs are not ranked on UNBEATEN.'],
}

export function ScoreBadge({ status }: { status: string | null }) {
  if (!status) return <span className="badge b-cyan" title="Not sent yet">Pending</span>
  const [label, cls, title] = SCORE_BADGE[status] || [status, 'b-muted', '']
  return <span className={`badge ${cls}`} title={title}>{label}</span>
}

export function scoreStatusHelp(status: string | null): string {
  return status ? (SCORE_BADGE[status]?.[2] ?? '') : 'Waiting for the next sync.'
}

export function Avatar({ url, small }: { url: string | null | undefined; small?: boolean }) {
  return url
    ? <img className={`avatar ${small ? 'avatar-sm' : ''}`} src={url} alt="" />
    : <div className={`avatar ${small ? 'avatar-sm' : ''}`} />
}
