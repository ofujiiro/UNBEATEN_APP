import { useCallback, useEffect, useMemo, useState } from 'react'
import { ipc, on, type ProgressEvent } from '../lib/ipc'
import { useStore } from '../lib/store'
import { joinUrl, mapTitle } from '../lib/format'
import { MapBadge } from '../components/ui'
import type { MapView } from '../lib/types'

type Filter = 'all' | 'updates' | 'missing' | 'installed'

const ACTION: Record<string, string> = {
  not_installed: 'Install',
  outdated: 'Update',
  modified: 'Reinstall',
  installed: 'Reinstall',
}

export default function Maps() {
  const { snap } = useStore()
  const [maps, setMaps] = useState<MapView[] | null>(null)
  const [error, setError] = useState('')
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [busy, setBusy] = useState<number | 'all' | null>(null)
  const [progress, setProgress] = useState<ProgressEvent | null>(null)
  const [rowError, setRowError] = useState<Record<number, string>>({})

  const load = useCallback(async () => {
    try { setMaps(await ipc.maps()); setError('') } catch (e) { setError(String(e)) }
  }, [])

  useEffect(() => { load() }, [load])
  useEffect(() => on<ProgressEvent>('maps://progress', setProgress), [])

  const shown = useMemo(() => {
    const term = q.trim().toLowerCase()
    return (maps ?? []).filter(m => {
      const st = m.status.state
      if (filter === 'updates' && st !== 'outdated' && st !== 'modified') return false
      if (filter === 'missing' && st !== 'not_installed') return false
      if (filter === 'installed' && st !== 'installed') return false
      if (!term) return true
      return [m.title, m.artist, m.creator, m.folder_name, m.install_folder]
        .some(v => v?.toLowerCase().includes(term))
    })
  }, [maps, q, filter])

  const outdated = maps?.filter(m => m.status.state === 'outdated').length ?? 0

  const install = async (m: MapView) => {
    setBusy(m.id); setProgress(null); setRowError(r => ({ ...r, [m.id]: '' }))
    try { await ipc.installMap(m.id) } catch (e) { setRowError(r => ({ ...r, [m.id]: String(e) })) }
    setBusy(null); load()
  }

  const updateAll = async () => {
    setBusy('all'); setError('')
    try { await ipc.updateAll() } catch (e) { setError(String(e)) }
    setBusy(null); load()
  }

  if (!snap) return null
  const songs = snap.paths.custom_songs

  return (
    <>
      <div className="page-head">
        <div>
          <div className="eyebrow">Ranked on UNBEATEN</div>
          <div className="page-title">Ranked <span>Maps</span></div>
        </div>
        <div className="row">
          {songs && <button className="btn btn-ghost btn-sm" onClick={() => ipc.revealPath(songs)}>Open CustomSongs</button>}
          <button className="btn btn-primary" disabled={!outdated || busy !== null || snap.game_running} onClick={updateAll}>
            {busy === 'all' ? 'Updating…' : `Update all${outdated ? ` (${outdated})` : ''}`}
          </button>
        </div>
      </div>

      {snap.game_running && (
        <div className="panel panel-body small" style={{ marginBottom: 14, color: 'var(--yellow)' }}>
          Close UNBEATABLE to install or update maps: the game reads CustomSongs when it starts.
        </div>
      )}

      <div className="toolbar">
        <input className="search" placeholder="Search title, artist, mapper…" value={q} onChange={e => setQ(e.target.value)} />
        <div className="seg">
          {([['all', 'All'], ['updates', 'Needs update'], ['missing', 'Not installed'], ['installed', 'Installed']] as const).map(([k, label]) => (
            <button key={k} className={filter === k ? 'active' : ''} onClick={() => setFilter(k)}>{label}</button>
          ))}
        </div>
      </div>

      {error && <div className="error-text" style={{ marginBottom: 12 }}>{error}</div>}

      <div className="panel">
        {maps === null && !error && <div className="empty">Loading the ranked maps…</div>}
        {maps && shown.length === 0 && <div className="empty">No map matches.</div>}
        {shown.map(m => {
          const st = m.status.state
          const cover = joinUrl(snap.server, m.cover_url)
          const isBusy = busy === m.id
          const pct = isBusy && progress?.id === m.id && progress.total ? Math.round(progress.done / progress.total * 100) : null
          return (
            <div key={m.id} className="map-row">
              {cover ? <img className="cover" src={cover} alt="" loading="lazy" /> : <div className="cover" />}
              <div className="map-main">
                <div className="map-title">{mapTitle(m)}</div>
                <div className="map-meta">
                  {m.artist} · mapped by {m.creator} · v{m.version}
                  {m.status.renamed && m.status.local_folder && ` · in "${m.status.local_folder}"`}
                </div>
                <div className="chips">
                  {m.difficulties.map(d => (
                    <span key={d.id} className="chip">{d.name} <b>★{d.star_rating.toFixed(2)}</b></span>
                  ))}
                  <a className="chip" style={{ textDecoration: 'none' }} onClick={() => ipc.openExternal(`${snap.server}/beatmaps/${m.id}`)}>Leaderboard ↗</a>
                </div>
                {rowError[m.id] && <div className="error-text">{rowError[m.id]}</div>}
              </div>
              <div className="map-side">
                <MapBadge state={st} />
                {isBusy ? (
                  <div className="progress"><div style={{ width: `${pct ?? 15}%` }} /></div>
                ) : (
                  <button
                    className={`btn btn-sm ${st === 'installed' ? 'btn-ghost' : 'btn-primary'}`}
                    disabled={busy !== null || snap.game_running}
                    title={st === 'modified' ? 'Your copy differs from the ranked chart. It is moved to backups, not deleted.' : undefined}
                    onClick={() => install(m)}
                  >
                    {ACTION[st]}
                  </button>
                )}
              </div>
            </div>
          )
        })}
      </div>
    </>
  )
}
