import { useState, useEffect, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Eraser, RefreshCw, X } from 'lucide-react'

interface DeadCodeCandidate {
  name: string
  file: string
  line: number
  kind: string
  language: string
  confidence: string
}
interface DeadCodeReport {
  candidates: DeadCodeCandidate[]
  total_found: number
  nodes_scanned: number
  uncertain_possibly_referenced: number
  caveats: string[]
}

interface Props {
  project: string
  onBuildGraph: () => Promise<void>
}

export default function FindDeadCodePanel({ project, onBuildGraph }: Props) {
  const { t } = useTranslation()
  const [report, setReport] = useState<DeadCodeReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [elapsed, setElapsed] = useState(0)
  // Monotonic id so a "cancel" (or project switch) discards a stale result.
  const runIdRef = useRef(0)

  const run = useCallback(async () => {
    const myId = ++runIdRef.current
    setLoading(true)
    setError(null)
    setElapsed(0)
    const started = Date.now()
    const timer = setInterval(() => {
      if (runIdRef.current === myId) setElapsed(Math.floor((Date.now() - started) / 1000))
    }, 500)
    try {
      const r = await invoke<DeadCodeReport>('find_dead_code_cmd', { project })
      if (runIdRef.current === myId) setReport(r)
    } catch (e) {
      if (runIdRef.current === myId) setError(String(e))
    } finally {
      clearInterval(timer)
      if (runIdRef.current === myId) setLoading(false)
    }
  }, [project])

  // Abandon the current run: the backend finishes on its own, but the UI
  // stops waiting and the (now stale) result is ignored.
  const cancel = () => {
    runIdRef.current++
    setLoading(false)
  }

  useEffect(() => {
    run()
    return () => { runIdRef.current++ } // discard in-flight result on unmount/switch
  }, [project]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="vs-intel-section" style={{ overflow: 'auto' }}>
      <div className="vs-intel-actions">
        <button className="vs-btn" onClick={run} disabled={loading}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <Eraser size={13} />}
          {loading ? t('intel.scanning', { secs: elapsed }) : t('intel.runDeadCode')}
        </button>
        {loading && (
          <button className="vs-btn" onClick={cancel}>
            <X size={13} /> {t('common.cancel')}
          </button>
        )}
      </div>

      {loading && (
        <p style={{ fontSize: 11, color: 'var(--vs-text-3)', marginTop: 4 }}>{t('intel.deadCodeHint')}</p>
      )}

      {error && (
        <div className="vs-empty">
          <span>{t('intel.graphNeeded')}</span>
          <button className="vs-btn" onClick={async () => { await onBuildGraph(); run() }}>{t('intel.buildGraph')}</button>
        </div>
      )}

      {report && !error && (
        <>
          <h2>{t('intel.deadSummary', { found: report.total_found, scanned: report.nodes_scanned, uncertain: report.uncertain_possibly_referenced })}</h2>
          {report.candidates.map((c, i) => (
            <div className="vs-row" key={i}>
              <span className={`sev ${c.confidence === 'high' ? 'high' : 'medium'}`}>{c.confidence}</span>
              <span className="what">{c.name} <span style={{ color: 'var(--vs-text-3)' }}>· {c.kind}</span></span>
              <span className="where">{c.file}:{c.line}</span>
            </div>
          ))}
          {report.candidates.length === 0 && <div className="vs-row"><span className="what">{t('intel.noDeadCode')}</span></div>}
          {report.caveats.length > 0 && (
            <p style={{ marginTop: 12, fontSize: 11, color: 'var(--vs-text-3)' }}>{report.caveats.join(' · ')}</p>
          )}
        </>
      )}
    </div>
  )
}
