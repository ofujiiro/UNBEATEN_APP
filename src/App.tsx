import { HashRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom'
import { StoreProvider, useStore } from './lib/store'
import { ipc } from './lib/ipc'
import { pp } from './lib/format'
import { Avatar } from './components/ui'
import SignIn from './pages/SignIn'
import Home from './pages/Home'
import Maps from './pages/Maps'
import Scores from './pages/Scores'
import SettingsPage from './pages/Settings'

const NAV = [
  ['/', 'Home'],
  ['/maps', 'Maps'],
  ['/scores', 'Scores'],
  ['/settings', 'Settings'],
] as const

function Banners() {
  const { snap } = useStore()
  const nav = useNavigate()
  if (!snap) return null
  const missing = !snap.paths.save_file || !snap.paths.custom_songs
  return (
    <>
      {snap.update_required && (
        <div className="banner banner-red">
          <span className="grow">{snap.update_required}</span>
          <button className="btn btn-sm btn-cream" onClick={() => ipc.openExternal(snap.update?.url || 'https://github.com/ofujiiro/UNBEATEN_APP/releases/latest')}>
            Download update
          </button>
        </div>
      )}
      {!snap.update_required && snap.update && (
        <div className="banner banner-pink">
          <span className="grow">Launcher {snap.update.version} is out (you have {snap.version}).</span>
          <button className="btn btn-sm btn-primary" onClick={() => ipc.openExternal(snap.update!.url)}>Get it</button>
        </div>
      )}
      {missing && (
        <div className="banner banner-yellow">
          <span className="grow">
            {!snap.paths.save_file
              ? "UNBEATABLE's save file wasn't found, so scores can't sync yet."
              : "UNBEATABLE's CustomSongs folder wasn't found."}
          </span>
          <button className="btn btn-sm btn-ghost" onClick={() => nav('/settings')}>Set it up</button>
        </div>
      )}
    </>
  )
}

function Shell() {
  const { snap } = useStore()
  const nav = useNavigate()
  const loc = useLocation()
  if (!snap) return <div className="empty">Loading…</div>
  if (!snap.signed_in) return <SignIn />
  const me = snap.me
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="logo">UN<br />BEATEN</div>
        <div className="logo-sub">Launcher</div>
        <nav className="nav">
          {NAV.map(([path, label]) => (
            <button key={path} className={loc.pathname === path ? 'active' : ''} onClick={() => nav(path)}>
              {label}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <div className="row" style={{ flexWrap: 'nowrap' }}>
            <Avatar url={me?.avatar_url} small />
            <div style={{ minWidth: 0 }}>
              <div className="truncate" style={{ fontWeight: 600, maxWidth: 140 }}>{me?.username ?? '…'}</div>
              <div className="small muted">{me ? `#${me.rank} · ${pp(me.total_pp)}` : ''}</div>
            </div>
          </div>
          <button className="btn btn-primary mt" style={{ width: '100%' }} disabled={snap.game_running}
            onClick={() => ipc.launchGame()}>
            {snap.game_running ? 'Playing…' : 'Play'}
          </button>
        </div>
      </aside>
      <main className="main">
        <Banners />
        <div className="content">
          <Routes>
            <Route path="/" element={<Home />} />
            <Route path="/maps" element={<Maps />} />
            <Route path="/scores" element={<Scores />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Routes>
        </div>
      </main>
    </div>
  )
}

export default function App() {
  return (
    <StoreProvider>
      <HashRouter>
        <Shell />
      </HashRouter>
    </StoreProvider>
  )
}
