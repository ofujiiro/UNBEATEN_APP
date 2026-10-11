// The app snapshot, refreshed whenever the Rust side says something changed.

import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react'
import { ipc, on } from './ipc'
import type { Snapshot } from './types'

interface Store {
  snap: Snapshot | null
  error: string
  refresh: () => Promise<void>
  setSnap: (s: Snapshot) => void
}

const Ctx = createContext<Store | null>(null)

export function StoreProvider({ children }: { children: ReactNode }) {
  const [snap, setSnap] = useState<Snapshot | null>(null)
  const [error, setError] = useState('')

  const refresh = useCallback(async () => {
    try {
      setSnap(await ipc.snapshot())
      setError('')
    } catch (e) {
      setError(String(e))
    }
  }, [])

  useEffect(() => {
    refresh()
    const offs = [
      on('state://changed', refresh),
      on('activity://new', refresh),
      on('game://state', refresh),
      on('auth://state', refresh),
    ]
    // Relative times ("5 min ago") and anything missed.
    const timer = setInterval(refresh, 30_000)
    return () => { offs.forEach(off => off()); clearInterval(timer) }
  }, [refresh])

  return <Ctx.Provider value={{ snap, error, refresh, setSnap }}>{children}</Ctx.Provider>
}

export function useStore(): Store {
  const s = useContext(Ctx)
  if (!s) throw new Error('useStore outside StoreProvider')
  return s
}
