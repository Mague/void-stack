import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import type { DependencyStatusDto } from '../types'
import { ShieldCheck } from 'lucide-react'

interface Props {
  project: string
  deps: DependencyStatusDto[]
  setDeps: (deps: DependencyStatusDto[]) => void
}

export default function DepsPanel({ project, deps, setDeps }: Props) {
  const { t } = useTranslation()
  const [loading, setLoading] = useState(false)

  const check = async () => {
    setLoading(true)
    try {
      const result = await invoke<DependencyStatusDto[]>('check_dependencies', { project })
      setDeps(result)
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  const okCount = deps.filter(d => d.status === 'Ok').length

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('deps.title')}</h2>
        <button className="btn btn-primary" onClick={check} disabled={loading}>
          {loading ? <><span className="loading-spinner" /> {t('deps.verifying')}</> : <><ShieldCheck size={12} /> {t('deps.verify')}</>}
        </button>
      </div>

      {deps.length > 0 && (
        <>
          <div style={{ marginBottom: 16, fontFamily: "'JetBrains Mono', monospace", fontSize: 12, color: 'var(--text-secondary)' }}>
            {t('deps.summary', { ok: okCount, total: deps.length })}
          </div>
          <table className="deps-table">
            <thead>
              <tr>
                <th>{t('deps.colType')}</th>
                <th>{t('deps.colStatus')}</th>
                <th>{t('deps.colVersion')}</th>
                <th>{t('deps.colDetails')}</th>
                <th>{t('deps.colFix')}</th>
              </tr>
            </thead>
            <tbody>
              {deps.map(d => (
                <tr key={d.dep_type}>
                  <td className="dep-type">{d.dep_type}</td>
                  <td>
                    <span className={`dep-status dep-${d.status.toLowerCase()}`}>
                      {d.status}
                    </span>
                  </td>
                  <td style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 11 }}>{d.version || '-'}</td>
                  <td style={{ fontSize: 12 }}>{d.details.join(', ') || '-'}</td>
                  <td className="fix-hint">{d.fix_hint || '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}

      {deps.length === 0 && !loading && (
        <div className="analysis-empty">
          <ShieldCheck size={32} style={{ opacity: 0.2 }} />
          <p>{t('deps.emptyPrompt')}</p>
        </div>
      )}
    </div>
  )
}
