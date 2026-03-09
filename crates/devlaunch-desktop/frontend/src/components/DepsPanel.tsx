import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DependencyStatusDto } from '../types'
import { ShieldCheck } from 'lucide-react'

interface Props {
  project: string
  deps: DependencyStatusDto[]
  setDeps: (deps: DependencyStatusDto[]) => void
}

export default function DepsPanel({ project, deps, setDeps }: Props) {
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
        <h2>Dependencias</h2>
        <button className="btn btn-primary" onClick={check} disabled={loading}>
          {loading ? <><span className="loading-spinner" /> Verificando...</> : <><ShieldCheck size={12} /> Verificar</>}
        </button>
      </div>

      {deps.length > 0 && (
        <>
          <div style={{ marginBottom: 16, fontFamily: "'JetBrains Mono', monospace", fontSize: 12, color: 'var(--text-secondary)' }}>
            {okCount}/{deps.length} dependencias listas
          </div>
          <table className="deps-table">
            <thead>
              <tr>
                <th>Tipo</th>
                <th>Estado</th>
                <th>Version</th>
                <th>Detalles</th>
                <th>Fix</th>
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
          <p>Presiona "Verificar" para analizar las dependencias</p>
        </div>
      )}
    </div>
  )
}
