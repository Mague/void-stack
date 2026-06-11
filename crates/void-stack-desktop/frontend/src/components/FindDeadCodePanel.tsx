import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Eraser, RefreshCw } from 'lucide-react'

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

  const run = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setReport(await invoke<DeadCodeReport>('find_dead_code_cmd', { project }))
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [project])

  useEffect(() => { run() }, [project]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="vs-intel-section" style={{ overflow: 'auto' }}>
      <div className="vs-intel-actions">
        <button className="vs-btn" onClick={run} disabled={loading}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <Eraser size={13} />}
          {loading ? t('common.loading') : t('intel.runDeadCode')}
        </button>
      </div>

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
