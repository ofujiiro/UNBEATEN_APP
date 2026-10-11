export function pp(value: number | null | undefined): string {
  return value == null ? '-' : `${value.toFixed(2)}pp`
}

export function percent(acc: number): string {
  return `${(acc * 100).toFixed(2)}%`
}

export function ago(unixSeconds: number | null | undefined): string {
  if (!unixSeconds) return 'never'
  const s = Math.max(0, Math.round(Date.now() / 1000 - unixSeconds))
  if (s < 45) return 'just now'
  if (s < 3600) return `${Math.round(s / 60)} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  return new Date(unixSeconds * 1000).toLocaleDateString()
}

// Modifier bits as UNBEATEN shows them: 4 DT, 8 HT, 16 CT.
export function mods(mask: number): string[] {
  const out: string[] = []
  if (mask & 4) out.push('DT')
  if (mask & 8) out.push('HT')
  if (mask & 16) out.push('CT')
  return out
}

export function mapTitle(m: { title: string | null; folder_name: string; install_folder?: string | null }): string {
  return m.title || m.install_folder || m.folder_name
}

export function joinUrl(server: string, path: string | null | undefined): string | null {
  if (!path) return null
  return /^https?:\/\//.test(path) ? path : `${server.replace(/\/$/, '')}${path}`
}
