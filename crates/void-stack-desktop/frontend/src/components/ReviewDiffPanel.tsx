import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { GitPullRequest, RefreshCw } from 'lucide-react'

interface ReviewPayload {
  markdown: string
  files_changed: number
  symbols_touched: number
  findings_on_changed_lines: number
  suppressed: number
  uncovered: number
}

interface Props {
  project: string
  onBuildGraph: () => Promise<void>
}

export default function ReviewDiffPanel({ project, onBuildGraph }: Props) {
  const { t } = useTranslation()
  const [payload, setPayload] = useState<ReviewPayload | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [base, setBase] = useState('')

  const run = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const p = await invoke<ReviewPayload>('review_diff_cmd', {
        project,
        gitBase: base.trim() || null,
      })
      setPayload(p)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [project, base])

  useEffect(() => { run() /* auto-run on open */ }, [project]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="vs-intel-section" style={{ overflow: 'auto' }}>
      <div className="vs-intel-actions">
        <input
          className="vs-base-input"
          placeholder={t('intel.gitBasePlaceholder')}
          value={base}
          onChange={e => setBase(e.target.value)}
          style={{ background: 'var(--vs-surface)', border: '1px solid var(--vs-line)', borderRadius: 8, padding: '6px 10px', color: 'var(--vs-text)', fontSize: 12 }}
        />
        <button className="vs-btn" onClick={run} disabled={loading}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <GitPullRequest size={13} />}
          {loading ? t('common.loading') : t('intel.runReview')}
        </button>
      </div>

      {error && (
        <div className="vs-empty">
          <span>{t('intel.graphNeeded')}</span>
          <button className="vs-btn" onClick={async () => { await onBuildGraph(); run() }}>{t('intel.buildGraph')}</button>
        </div>
      )}

      {payload && !error && (
        <>
          <h2>{t('intel.reviewSummary', {
            files: payload.files_changed,
            findings: payload.findings_on_changed_lines,
            uncovered: payload.uncovered,
          })}</h2>
          <div className="vs-markdown">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{payload.markdown}</ReactMarkdown>
          </div>
        </>
      )}
    </div>
  )
}
