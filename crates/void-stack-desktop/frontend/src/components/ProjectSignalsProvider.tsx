// Polls the selected project's fingerprint and publishes per-channel
// change counters — see ../projectSignals.ts.

import { useEffect, useRef, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { CHANNELS, POLL_MS, SignalsContext, ZERO, type Fingerprint, type ProjectSignals, type SignalChannel } from '../projectSignals'

export default function ProjectSignalsProvider({ project, children }: { project: string | null; children: ReactNode }) {
  const [signals, setSignals] = useState<ProjectSignals>(ZERO)
  const last = useRef<Fingerprint | null>(null)

  useEffect(() => {
    last.current = null
    setSignals(ZERO)
    if (!project) return

    let cancelled = false
    let inFlight = false
    const bump = (changed: SignalChannel[]) => {
      if (cancelled || changed.length === 0) return
      setSignals(prev => {
        const next = { ...prev }
        for (const c of changed) next[c] = prev[c] + 1
        return next
      })
    }

    const poll = async () => {
      // Skip while hidden (tray/minimized) or if the last poll is still running
      // (slow WSL paths) — never stack requests.
      if (inFlight || document.visibilityState !== 'visible') return
      inFlight = true
      try {
        const fp = await invoke<Fingerprint>('get_project_fingerprint_cmd', { project })
        if (cancelled) return
        const prev = last.current
        last.current = fp
        // First sample is the baseline: views already loaded fresh data.
        if (prev) bump(CHANNELS.filter(c => prev[c] !== fp[c]))
      } catch {
        // Project removed/renamed mid-poll: the project list refresh handles it.
      } finally {
        inFlight = false
      }
    }

    const onVisible = () => { if (document.visibilityState === 'visible') poll() }

    poll()
    const timer = setInterval(poll, POLL_MS)
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      cancelled = true
      clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [project])

  return <SignalsContext.Provider value={signals}>{children}</SignalsContext.Provider>
}
