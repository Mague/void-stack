import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'

interface ProjectStats {
  project: string
  avg_savings_pct: number
  operations: number
  lines_saved: number
}

interface OperationStats {
  operation: string
  avg_savings_pct: number
  operations: number
  lines_saved: number
}

interface StatsReport {
  total_operations: number
  avg_savings_pct: number
  total_lines_saved: number
  by_project: ProjectStats[]
  by_operation: OperationStats[]
  period_days: number
}

interface Props {
  project: string | null
}

export default function StatsPanel({ project }: Props) {
  const { t } = useTranslation()
  const [report, setReport] = useState<StatsReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [filterProject, setFilterProject] = useState<string>('')

  const loadStats = async () => {
    setLoading(true)
    try {
      const json = await invoke<string>('get_token_stats_cmd', {
        project: filterProject || null,
        days: 30,
      })
      setReport(JSON.parse(json))
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadStats()
  }, [filterProject])

  const maxPct = Math.max(
    ...(report?.by_project.map(p => p.avg_savings_pct) || [1]),
    ...(report?.by_operation.map(o => o.avg_savings_pct) || [1]),
    1
  )

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('statsPanel.title')}</h2>
        <div className="log-controls">
          <input
            type="text"
            placeholder={t('statsPanel.filterPlaceholder')}
            value={filterProject}
            onChange={e => setFilterProject(e.target.value)}
            style={{ width: '150px', padding: '2px 6px', fontSize: '0.75rem' }}
          />
          <button className="btn btn-sm" onClick={loadStats} disabled={loading}>
            {loading ? t('common.loading') : t('statsPanel.refresh')}
          </button>
        </div>
      </div>

      {!report ? (
        <div className="analysis-empty">
          <p>{loading ? t('common.loading') : t('statsPanel.emptyPrompt')}</p>
        </div>
      ) : (
        <div className="docs-content" style={{ padding: '12px' }}>
          <div style={{ display: 'flex', gap: '24px', marginBottom: '16px', flexWrap: 'wrap' }}>
            <div>
              <div style={{ fontSize: '1.5rem', fontWeight: 'bold' }}>{report.total_operations}</div>
              <div style={{ fontSize: '0.7rem', opacity: 0.6 }}>{t('statsPanel.totalOps')}</div>
            </div>
            <div>
              <div style={{ fontSize: '1.5rem', fontWeight: 'bold' }}>{report.avg_savings_pct.toFixed(0)}%</div>
              <div style={{ fontSize: '0.7rem', opacity: 0.6 }}>{t('statsPanel.avgSavings')}</div>
            </div>
            <div>
              <div style={{ fontSize: '1.5rem', fontWeight: 'bold' }}>{report.total_lines_saved.toLocaleString()}</div>
              <div style={{ fontSize: '0.7rem', opacity: 0.6 }}>{t('statsPanel.linesSaved')}</div>
            </div>
          </div>

          {report.by_project.length > 0 && (
            <>
              <h3 style={{ fontSize: '0.8rem', marginBottom: '8px' }}>{t('statsPanel.byProject')}</h3>
              {report.by_project.map(p => (
                <div key={p.project} style={{ marginBottom: '6px' }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.75rem' }}>
                    <span>{p.project}</span>
                    <span>{p.avg_savings_pct.toFixed(0)}% ({p.operations} ops)</span>
                  </div>
                  <div style={{ background: 'var(--bg-darker, #111)', borderRadius: '3px', height: '6px', marginTop: '2px' }}>
                    <div style={{
                      width: `${(p.avg_savings_pct / maxPct) * 100}%`,
                      height: '100%',
                      background: 'var(--accent, #0af)',
                      borderRadius: '3px',
                    }} />
                  </div>
                </div>
              ))}
            </>
          )}

          {report.by_operation.length > 0 && (
            <>
              <h3 style={{ fontSize: '0.8rem', margin: '16px 0 8px' }}>{t('statsPanel.byOperation')}</h3>
              {report.by_operation.map(o => (
                <div key={o.operation} style={{ marginBottom: '6px' }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.75rem' }}>
                    <span>{o.operation}</span>
                    <span>{o.avg_savings_pct.toFixed(0)}% ({o.operations} ops)</span>
                  </div>
                  <div style={{ background: 'var(--bg-darker, #111)', borderRadius: '3px', height: '6px', marginTop: '2px' }}>
                    <div style={{
                      width: `${(o.avg_savings_pct / maxPct) * 100}%`,
                      height: '100%',
                      background: 'var(--accent-alt, #0fa)',
                      borderRadius: '3px',
                    }} />
                  </div>
                </div>
              ))}
            </>
          )}

          {report.total_operations === 0 && (
            <p style={{ opacity: 0.5, fontSize: '0.75rem' }}>{t('statsPanel.noData')}</p>
          )}
        </div>
      )}
    </div>
  )
}
