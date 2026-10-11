import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ipc } from '../lib/ipc'
import { useStore } from '../lib/store'
import { ago, pp } from '../lib/format'
import { Avatar } from '../components/ui'
import type { MapView } from '../lib/types'

export default function Home() {
  const { snap, refresh } = useStore()
  const nav = useNavigate()
  const [maps, setMaps] = useState<MapView[] | null>(null)
  const [syncError, setSyncError] = useState('')

  useEffect(() => { ipc.maps().then(setMaps).catch(() => setMaps(null)) }, [snap?.activity.length])

  if (!snap) return null
  const me = snap.me
  const count = (state: string) => maps?.filter(m => m.status.state === state).length ?? 0

  const syncNow = async () => {
    setSyncError('')
    try { await ipc.syncNow() } catch (e) { setSyncError(String(e)) }
    refresh()
  }

  return (
    <>
      <div className="hero">
        <Avatar url={me?.avatar_url} />
        <div className="grow">
          <div className="eyebrow" style={{ color: 'var(--ink5)' }}>Signed in to UNBEATEN</div>
          <div className="hero-name">{me?.username ?? '…'}</div>
          <div className="hero-stats">
            <span>Rank <b>#{me?.rank ?? '-'}</b></span>
            <span><b>{me ? me.total_pp.toFixed(2) : '-'}</b> pp</span>
            {me && (
              <a style={{ alignSelf: 'center', color: 'var(--pink)' }}
                onClick={() => ipc.openExternal(`${snap.server}/users/${me.id}`)}>View profile</a>
            )}
          </div>
        </div>
        <button className="btn btn-primary btn-xl" disabled={snap.game_running} onClick={() => ipc.launchGame()}>
          {snap.game_running ? 'Playing…' : 'Play'}
        </button>
      </div>

      <div className="grid-3">
        <div className="panel stat">
          <div className="eyebrow">Game</div>
          <div className="stat-value">
            <span className={`dot ${snap.game_running ? 'dot-on' : 'dot-off'}`} />
            {snap.game_running ? 'Running' : 'Not running'}
          </div>
          <div className="stat-sub">
            {snap.paths.game_dir ? 'Found in your Steam library' : "UNBEATABLE wasn't found in Steam; Play may not work"}
          </div>
        </div>
        <div className="panel stat">
          <div className="eyebrow">Score sync</div>
          <div className="stat-value">
            <span className={`dot ${snap.syncing ? 'dot-busy' : snap.settings.auto_sync ? 'dot-on' : 'dot-off'}`} />
            {snap.syncing ? 'Syncing…' : snap.settings.auto_sync ? 'Automatic' : 'Manual'}
          </div>
          <div className="stat-sub row" style={{ justifyContent: 'space-between' }}>
            <span>Last sync {ago(snap.last_sync_at)}</span>
            <button className="btn btn-ghost btn-sm" disabled={snap.syncing || !snap.paths.save_file} onClick={syncNow}>Sync now</button>
          </div>
          {syncError && <div className="error-text">{syncError}</div>}
        </div>
        <div className="panel stat">
          <div className="eyebrow">Ranked maps</div>
          <div className="stat-value">{maps ? `${count('installed')} / ${maps.length}` : '…'}</div>
          <div className="stat-sub row" style={{ justifyContent: 'space-between' }}>
            <span>{count('outdated') > 0 ? `${count('outdated')} need an update` : 'installed'}</span>
            <button className="btn btn-ghost btn-sm" onClick={() => nav('/maps')}>Browse</button>
          </div>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><span className="panel-title">Activity</span></div>
        {snap.activity.length === 0 ? (
          <div className="empty">
            Nothing yet. Play a ranked custom chart in UNBEATABLE and your score shows up here a few seconds
            after the results screen.
          </div>
        ) : (
          <ul className="feed">
            {snap.activity.map((a, i) => (
              <li key={i}>
                <span className="feed-time">{ago(a.at)}</span>
                <span className={`feed-kind ${a.kind}`}>{a.kind}</span>
                <div style={{ minWidth: 0 }}>
                  <div>{a.message}</div>
                  {a.imported.length > 0 && (
                    <div className="feed-scores">
                      {a.imported.slice(0, 8).map((s, j) => (
                        <div key={j} className="truncate" style={{ maxWidth: 600 }}>
                          {s.song.replace(/^CUSTOM_/i, '')} · {pp(s.pp)}
                          {s.previous_pp != null && ` (was ${pp(s.previous_pp)})`}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  )
}
