import { useEffect, useState } from 'react'
import { ipc, on, type AuthEvent } from '../lib/ipc'
import { useStore } from '../lib/store'
import type { LinkStart } from '../lib/types'

const MESSAGES: Record<string, string> = {
  denied: 'The sign-in was denied on the website.',
  expired: 'The code expired. Start again.',
  cancelled: '',
}

export default function SignIn() {
  const { snap, refresh, setSnap } = useStore()
  const [link, setLink] = useState<LinkStart | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [server, setServer] = useState(snap?.settings.server_url ?? '')
  const [showServer, setShowServer] = useState(false)

  useEffect(() => on<AuthEvent>('auth://state', e => {
    if (e.status !== 'approved') {
      setLink(null)
      setError(MESSAGES[e.status] ?? '')
    }
    refresh()
  }), [refresh])

  const start = async () => {
    setBusy(true); setError('')
    try {
      setLink(await ipc.authBegin())
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const cancel = async () => { await ipc.authCancel(); setLink(null) }

  const saveServer = async () => {
    if (!snap) return
    try {
      setSnap(await ipc.saveSettings({ ...snap.settings, server_url: server.trim() || 'https://unbeaten.cc' }, snap.autostart))
      setShowServer(false)
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div className="signin">
      <div className="signin-card">
        <div className="signin-logo">UN<br />BEATEN</div>
        <div className="signin-tag">Launcher</div>
        <div className="signin-box">
          {!link ? (
            <>
              <p className="small" style={{ color: 'var(--cream3)', lineHeight: 1.7 }}>
                Sign in with your UNBEATEN account. Your browser opens {snap?.server.replace(/^https?:\/\//, '')}, where
                you sign in with Discord and approve this launcher. It can then sync your scores and install
                ranked maps, and nothing else.
              </p>
              <button className="btn btn-primary btn-xl mt" style={{ width: '100%', fontSize: 24 }} disabled={busy} onClick={start}>
                {busy ? 'Starting…' : 'Sign in'}
              </button>
            </>
          ) : (
            <>
              <div className="eyebrow">Approve this code in your browser</div>
              <div className="code">{link.user_code}</div>
              <p className="small muted">
                Waiting for approval… If the browser didn't open, go to{' '}
                <a onClick={() => ipc.openExternal(link.verification_uri_complete)}>{link.verification_uri}</a>{' '}
                and enter the code.
              </p>
              <div className="row mt">
                <button className="btn btn-ghost btn-sm" onClick={() => ipc.openExternal(link.verification_uri_complete)}>Open browser again</button>
                <button className="btn btn-ghost btn-sm" onClick={cancel}>Cancel</button>
              </div>
            </>
          )}
          {error && <div className="error-text">{error}</div>}
        </div>
        <div className="small" style={{ marginTop: 14, color: 'var(--ink5)' }}>
          {showServer ? (
            <div className="row">
              <input className="input" value={server} onChange={e => setServer(e.target.value)} placeholder="https://unbeaten.cc" />
              <button className="btn btn-sm btn-cream" onClick={saveServer}>Save</button>
            </div>
          ) : (
            <span style={{ cursor: 'pointer', textDecoration: 'underline' }} onClick={() => setShowServer(true)}>
              Server: {snap?.server}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}
