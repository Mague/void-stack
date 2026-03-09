import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DiagramResult } from '../types'
import { GitBranch } from 'lucide-react'

interface Props {
  project: string
  diagram: DiagramResult | null
  setDiagram: (d: DiagramResult | null) => void
}

export default function DiagramPanel({ project, diagram, setDiagram }: Props) {
  const [loading, setLoading] = useState(false)
  const [viewMode, setViewMode] = useState<'render' | 'code'>('render')

  const generate = async () => {
    setLoading(true)
    try {
      const result = await invoke<DiagramResult>('generate_diagram', { project })
      setDiagram(result)
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  // Render mermaid diagrams when data changes
  useEffect(() => {
    if (!diagram || viewMode !== 'render') return

    const renderMermaid = async () => {
      try {
        const m = await import('mermaid')
        m.default.initialize({ startOnLoad: false, theme: 'dark' })
        document.querySelectorAll('.mermaid-render').forEach(async (el) => {
          const code = el.getAttribute('data-code') || ''
          const clean = code.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()
          if (clean) {
            try {
              const { svg } = await m.default.render('mermaid-' + Math.random().toString(36).slice(2), clean)
              el.innerHTML = svg
            } catch {
              el.innerHTML = `<pre class="mermaid-raw">${clean}</pre>`
            }
          }
        })
      } catch {
        // mermaid not available, show raw
      }
    }

    // Small delay to let DOM update
    const t = setTimeout(renderMermaid, 100)
    return () => clearTimeout(t)
  }, [diagram, viewMode])

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>Diagramas</h2>
        <div className="diagram-controls">
          {diagram && (
            <div className="format-toggle">
              <button className={viewMode === 'render' ? 'active' : ''} onClick={() => setViewMode('render')}>Render</button>
              <button className={viewMode === 'code' ? 'active' : ''} onClick={() => setViewMode('code')}>Código</button>
            </div>
          )}
          <button className="btn btn-primary" onClick={generate} disabled={loading}>
            {loading ? <><span className="loading-spinner" /> Generando...</> : <><GitBranch size={12} /> Generar</>}
          </button>
        </div>
      </div>

      {diagram && (
        <div className="diagrams-content">
          <h3>Arquitectura</h3>
          {viewMode === 'render' ? (
            <div className="mermaid-render" data-code={diagram.architecture}></div>
          ) : (
            <pre className="mermaid-raw">{diagram.architecture.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
          )}

          {diagram.api_routes && (
            <>
              <h3>Rutas API</h3>
              {viewMode === 'render' ? (
                <div className="mermaid-render" data-code={diagram.api_routes}></div>
              ) : (
                <pre className="mermaid-raw">{diagram.api_routes.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
              )}
            </>
          )}

          {diagram.db_models && (
            <>
              <h3>Modelos DB</h3>
              {viewMode === 'render' ? (
                <div className="mermaid-render" data-code={diagram.db_models}></div>
              ) : (
                <pre className="mermaid-raw">{diagram.db_models.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
              )}
            </>
          )}

          {diagram.warnings.length > 0 && (
            <div className="warnings">
              <h3>Advertencias</h3>
              <ul>{diagram.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>
            </div>
          )}
        </div>
      )}

      {!diagram && !loading && (
        <div className="analysis-empty">
          <GitBranch size={32} style={{ opacity: 0.2 }} />
          <p>Presiona "Generar" para crear diagramas del proyecto</p>
        </div>
      )}
    </div>
  )
}
