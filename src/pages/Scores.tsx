import { useCallback, useEffect, useMemo, useState } from 'react'
import { ipc } from '../lib/ipc'
import { useStore } from '../lib/store'
import { ago, mods, percent, pp } from '../lib/format'
import { ScoreBadge, scoreStatusHelp } from '../components/ui'
import type { ScoreView } from '../lib/types'

type Filter = 'all' | 'synced' | 'attention' | 'unranked'

const GROUP: Record<string, Filter> = {
  imported: 'synced', no_change: 'synced',
  outdated_chart: 'attention', modified_chart: 'attention',
  unmatched: 'unranked',
}

export default function Scores() {
  const { snap, refresh } = useStore()
  const [rows, setRows] = useState<ScoreView[] | null>(null)
  const [error, setError] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [q, setQ] = useState('')

  const load = useCallback(async () => {
    try { setRows(await ipc.scores()); setError('') } catch (e) { setError(String(e)) }
  }, [])

  useEffect(() => { load() }, [load, snap?.last_sync_at])

  const shown = useMemo(() => {
    const term = q.trim().toLowerCase()
    return (rows ?? [])
      .filter(r => filter === 'all' || GROUP[r.status ?? ''] === filter)
      .filter(r => !term || r.folder.toLowerCase().includes(term))
      .sort((a, b) => (b.pp ?? -1) - (a.pp ?? -1) || a.folder.localeCompare(b.folder))
  }, [rows, filter, q])

  const counts = useMemo(() => {
    const c: Record<Filter, number> = { all: rows?.length ?? 0, synced: 0, attention: 0, unranked: 0 }
    for (const r of rows ?? []) { const g = GROUP[r.status ?? '']; if (g) c[g]++ }
    return c
  }, [rows])

  const syncNow = async () => {
    setError('')
    try { await ipc.syncNow() } catch (e) { setError(String(e)) }
    refresh(); load()
  }

  if (!snap) return null

  return (
    <>
      <div className="page-head">
        <div>
          <div className="eyebrow">From your save file · last sync {ago(snap.last_sync_at)}</div>
          <div className="page-title">Your <span>Scores</span></div>
        </div>
        <button className="btn btn-primary" disabled={snap.syncing || !snap.paths.save_file} onClick={syncNow}>
          {snap.syncing ? 'Syncing…' : 'Sync now'}
        </button>
      </div>

      <div className="toolbar">
        <input className="search" placeholder="Search songs…" value={q} onChange={e => setQ(e.target.value)} />
        <div className="seg">
          {([['all', 'All'], ['synced', 'Synced'], ['attention', 'Needs attention'], ['unranked', 'Not ranked']] as const).map(([k, label]) => (
            <button key={k} className={filter === k ? 'active' : ''} onClick={() => setFilter(k)}>{label} ({counts[k]})</button>
          ))}
        </div>
      </div>

      {error && <div className="error-text" style={{ marginBottom: 12 }}>{error}</div>}
      {filter === 'attention' && counts.attention > 0 && (
        <div className="panel panel-body small" style={{ marginBottom: 14, color: 'var(--cream3)' }}>
          These were played on a chart that differs from the ranked version. Update or reinstall the map on the
          Maps page, then play it again: new plays on the ranked chart sync normally.
        </div>
      )}

      <div className="panel" style={{ overflow: 'auto' }}>
        {rows === null && !error && <div className="empty">Reading your save…</div>}
        {rows && shown.length === 0 && <div className="empty">No scores here.</div>}
        {shown.length > 0 && (
          <table className="table">
            <thead>
              <tr>
                <th>Song</th><th>Difficulty</th><th className="num">Accuracy</th><th className="num">Combo</th>
                <th className="num">Miss</th><th>Mods</th><th>Status</th><th className="num">PP</th>
              </tr>
            </thead>
            <tbody>
              {shown.map(r => (
                <tr key={r.song} title={scoreStatusHelp(r.status)}>
                  <td className="truncate">{r.folder}</td>
                  <td>{r.tier}{r.mode && r.mode !== 'Classic' ? ` · ${r.mode}` : ''}</td>
                  <td className="num">{percent(r.accuracy)}</td>
                  <td className="num">{r.max_combo}</td>
                  <td className="num">{r.misses}</td>
                  <td className="small">{mods(r.modifier_mask).join(' ')}</td>
                  <td><ScoreBadge status={r.status} />{r.verified && <span className="small muted" title="Matched to the ranked chart by its content"> ✓</span>}</td>
                  <td className="num">{r.pp != null ? pp(r.pp) : '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  )
}
