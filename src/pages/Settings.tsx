import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { ipc } from '../lib/ipc'
import { useStore } from '../lib/store'
import { Toggle } from '../components/ui'
import type { Settings } from '../lib/types'

function PathField({ label, value, detected, override, onChange, directory, help }: {
  label: string
  value: string | null
  detected: string | null
  override: string | null
  onChange: (v: string | null) => void
  directory: boolean
  help: string
}) {
  const browse = async () => {
    const picked = await open({
      directory,
      multiple: false,
      defaultPath: value ?? undefined,
      filters: directory ? undefined : [{ name: 'arcade-highscores.json', extensions: ['json'] }],
    })
    if (typeof picked === 'string') onChange(picked)
  }
  return (
    <div className="field">
      <div className="field-label">{label}</div>
      <div className="path-row">
        <input className="input" value={override ?? ''} placeholder={detected ?? 'Not found'}
          onChange={e => onChange(e.target.value || null)} />
        <button className="btn btn-ghost btn-sm" onClick={browse}>Browse…</button>
        {override && <button className="btn btn-ghost btn-sm" onClick={() => onChange(null)}>Use detected</button>}
        {value && <button className="btn btn-ghost btn-sm" onClick={() => ipc.revealPath(value)}>Show</button>}
      </div>
      <div className="field-help">
        {override ? 'Using the path above.' : detected ? `Detected: ${detected}` : 'Not found automatically. Pick it with Browse.'}
      </div>
      <div className="field-help">{help}</div>
    </div>
  )
}

export default function SettingsPage() {
  const { snap, setSnap } = useStore()
  const [form, setForm] = useState<Settings | null>(null)
  const [autostart, setAutostart] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (snap && !form) { setForm(snap.settings); setAutostart(snap.autostart) }
  }, [snap, form])

  if (!snap || !form) return null
  const det = snap.paths.detected
  const dirty = JSON.stringify(form) !== JSON.stringify(snap.settings) || autostart !== snap.autostart
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => { setForm({ ...form, [k]: v }); setSaved(false) }

  const save = async () => {
    setError('')
    try {
      const next = await ipc.saveSettings({ ...form, server_url: form.server_url.trim() }, autostart)
      setSnap(next); setForm(next.settings); setAutostart(next.autostart); setSaved(true)
    } catch (e) { setError(String(e)) }
  }

  const redetect = async () => {
    try { setSnap(await ipc.redetect()) } catch (e) { setError(String(e)) }
  }

  const signOut = async () => { await ipc.signOut() }

  return (
    <>
      <div className="page-head">
        <div>
          <div className="eyebrow">UNBEATEN Launcher {snap.version}</div>
          <div className="page-title">Sett<span>ings</span></div>
        </div>
        <div className="row">
          {saved && !dirty && <span className="small" style={{ color: 'var(--green)' }}>Saved</span>}
          <button className="btn btn-primary" disabled={!dirty} onClick={save}>Save changes</button>
        </div>
      </div>
      {error && <div className="error-text" style={{ marginBottom: 12 }}>{error}</div>}

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">UNBEATABLE</span>
          <button className="btn btn-ghost btn-sm" onClick={redetect}>Detect again</button>
        </div>
        <div className="panel-body">
          <PathField label="Save file (arcade-highscores.json)" value={snap.paths.save_file}
            detected={det.save_file} override={form.save_file} onChange={v => set('save_file', v)} directory={false}
            help="The launcher reads your scores from it; it never writes to it." />
          {det.other_saves.length > 0 && !form.save_file && (
            <div className="field-help" style={{ marginTop: -10, marginBottom: 14 }}>
              Other save files found: {det.other_saves.join(', ')}. The most recently played one is used.
            </div>
          )}
          <PathField label="CustomSongs folder" value={snap.paths.custom_songs}
            detected={det.custom_songs} override={form.custom_songs} onChange={v => set('custom_songs', v)} directory
            help="Where ranked maps get installed. Folders you replace are moved to “UNBEATEN Launcher/backups” next to it." />
          <div className="field-help">
            Game folder: {snap.paths.game_dir ?? 'not found in your Steam libraries'}
            {det.steam_roots.length > 0 && ` · Steam: ${det.steam_roots.join(', ')}`}
          </div>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><span className="panel-title">Behaviour</span></div>
        <div className="panel-body">
          <Toggle on={form.auto_sync} onChange={v => set('auto_sync', v)} label="Sync scores automatically"
            help="Sends new scores to UNBEATEN a few seconds after the game saves them, and when the game closes." />
          <Toggle on={form.notifications} onChange={v => set('notifications', v)} label="Notifications"
            help="A desktop notification when new scores are synced." />
          <Toggle on={autostart} onChange={v => { setAutostart(v); setSaved(false) }} label="Start with my computer"
            help="Starts in the tray, so scores sync even when you launch the game from Steam." />
          <Toggle on={form.start_minimized} onChange={v => set('start_minimized', v)} label="Start minimised to the tray" disabled={!snap.tray} />
          <Toggle on={form.close_to_tray} onChange={v => set('close_to_tray', v)} label="Keep running in the tray when the window is closed"
            disabled={!snap.tray}
            help={snap.tray ? undefined : 'No system tray is available on this desktop, so closing the window quits the launcher.'} />
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><span className="panel-title">Account</span></div>
        <div className="panel-body">
          <div className="row" style={{ justifyContent: 'space-between' }}>
            <div>Signed in as <b>{snap.me?.username ?? '…'}</b> on {snap.server}</div>
            <button className="btn btn-red btn-sm" onClick={signOut}>Sign out</button>
          </div>
          <div className="field mt">
            <div className="field-label">Server</div>
            <div className="path-row">
              <input className="input" value={form.server_url} onChange={e => set('server_url', e.target.value)} />
            </div>
            <div className="field-help">Leave this as https://unbeaten.cc unless you run your own UNBEATEN server. Changing it signs you out of this one.</div>
          </div>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><span className="panel-title">About</span></div>
        <div className="panel-body small" style={{ lineHeight: 1.8, color: 'var(--cream3)' }}>
          <div>
            UNBEATEN Launcher {snap.version}.{' '}
            {snap.update ? <a onClick={() => ipc.openExternal(snap.update!.url)}>Version {snap.update.version} is available.</a> : 'Up to date.'}
          </div>
          <div>
            <a onClick={() => ipc.openExternal(snap.server)}>UNBEATEN</a> ·{' '}
            <a onClick={() => ipc.openExternal('https://github.com/ofujiiro/UNBEATEN_APP')}>Source and releases</a>
          </div>
          <div className="muted">A fan project, not affiliated with D-CELL GAMES or Playstack. UNBEATABLE™ is property of D-CELL GAMES.</div>
        </div>
      </div>
    </>
  )
}
