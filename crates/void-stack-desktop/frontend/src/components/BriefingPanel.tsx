import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Sunrise, RefreshCw } from 'lucide-react'

interface Props {
  project: string | null
}

export default function BriefingPanel({ project }: Props) {
  const { t } = useTranslation()
  const [briefing, setBriefing] = useState<string | null>(null)
  const [active, setActive] = useState<string[]>([])
  const [scope, setScope] = useState<'active' | 'current'>('active')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    invoke<string[]>('briefing_active_cmd')
      .then(setActive)
      .catch(() => setActive([]))
  }, [])

  const run = (which: 'active' | 'current') => {
    setScope(which)
    setLoading(true)
    setError(null)
    const only = which === 'current' && project ? [project] : null
    invoke<string>('daily_briefing_cmd', { only })
      .then(setBriefing)
      .catch(e => setError(String(e)))
      .finally(() => setLoading(false))
  }

  return (
    <div className="vs-briefing-panel">
      <div className="vs-board-toolbar">
        <button className="vs-btn" onClick={() => run('active')} disabled={loading}>
          <Sunrise size={13} />{' '}
          {t('briefing.runActive', { count: active.length })}
        </button>
        <button className="vs-btn" onClick={() => run('current')} disabled={loading || !project}>
          <RefreshCw size={13} /> {t('briefing.runCurrent', { project: project ?? '—' })}
        </button>
        {active.length > 0 && (
          <span className="vs-search-meta vs-briefing-active">
            {t('briefing.activeList')}: {active.join(', ')}
          </span>
        )}
      </div>
      {error && <p className="vs-search-err">{error}</p>}
      {loading && (
        <p className="vs-search-meta">
          {t('briefing.generating', {
            scope: scope === 'current' && project ? project : t('briefing.activeList'),
          })}
        </p>
      )}
      {!briefing && !loading && !error && (
        <p className="vs-search-meta">{t('briefing.empty')}</p>
      )}
      {briefing && !loading && (
        <div className="docs-content vs-briefing-content">
          <Markdown remarkPlugins={[remarkGfm]}>{briefing}</Markdown>
        </div>
      )}
    </div>
  )
}
