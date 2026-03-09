import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import type { DiagramResult } from '../types'
import { GitBranch, Save, FileDown } from 'lucide-react'
import CopyButton from './CopyButton'

interface Props {
  project: string
  diagram: DiagramResult | null
  setDiagram: (d: DiagramResult | null) => void
}

function ZoomableMermaid({ code }: { code: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [zoom, setZoom] = useState(1)
  const [rendered, setRendered] = useState(false)
  const clean = code.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()

  useEffect(() => {
    if (!containerRef.current || !clean) return
    setRendered(false)
    setZoom(1)
    const render = async () => {
      try {
        const m = await import('mermaid')
        m.default.initialize({ startOnLoad: false, theme: 'dark' })
        const id = 'mermaid-' + Math.random().toString(36).slice(2)
        const { svg } = await m.default.render(id, clean)
        if (containerRef.current) {
          containerRef.current.innerHTML = svg
          setRendered(true)
        }
      } catch {
        if (containerRef.current) {
          containerRef.current.innerHTML = `<pre class="mermaid-raw">${clean}</pre>`
        }
      }
    }
    render()
  }, [clean])

  useEffect(() => {
    if (!containerRef.current || !rendered) return
    const svg = containerRef.current.querySelector('svg')
    if (svg) {
      svg.style.transform = `scale(${zoom})`
      svg.style.transformOrigin = 'top left'
    }
  }, [zoom, rendered])

  return (
    <div>
      <div className="mermaid-zoom-controls">
        <button onClick={() => setZoom(z => Math.max(0.25, z - 0.25))}>-</button>
        <button onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
        <button onClick={() => setZoom(z => Math.min(3, z + 0.25))}>+</button>
      </div>
      <div className="mermaid-render" ref={containerRef} />
    </div>
  )
}

export default function DiagramPanel({ project, diagram, setDiagram }: Props) {
  const { t } = useTranslation()
  const [loading, setLoading] = useState(false)
  const [viewMode, setViewMode] = useState<'render' | 'code'>('render')
  const [format, setFormat] = useState<'drawio' | 'mermaid'>('drawio')
  const [saveMsg, setSaveMsg] = useState('')

  const generate = async () => {
    setLoading(true)
    setSaveMsg('')
    try {
      const result = await invoke<DiagramResult>('generate_diagram', { project, format })
      setDiagram(result)
      // Show auto-saved .drawio path
      if (result.saved_path) {
        setSaveMsg(result.saved_path)
      }
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  const handleFormatChange = (fmt: 'drawio' | 'mermaid') => {
    setFormat(fmt)
    setDiagram(null)
    setSaveMsg('')
  }

  const saveMermaid = async () => {
    if (!diagram) return
    setSaveMsg('')
    try {
      const content = [diagram.architecture, diagram.api_routes, diagram.db_models]
        .filter(Boolean)
        .join('\n\n')
      const path = await invoke<string>('save_diagram_file', { project, content, extension: 'md' })
      setSaveMsg(path)
    } catch (e) {
      setSaveMsg(`Error: ${e}`)
    }
  }

  const allMermaidCode = diagram
    ? [diagram.architecture, diagram.api_routes, diagram.db_models]
        .filter(Boolean)
        .map(c => c!.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim())
        .join('\n\n')
    : ''

  const hasDiagrams = diagram && (diagram.architecture || diagram.api_routes || diagram.db_models)

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('diagrams.title')}</h2>
        <div className="diagram-controls">
          <div className="format-toggle">
            <button className={format === 'drawio' ? 'active' : ''} onClick={() => handleFormatChange('drawio')}>Draw.io</button>
            <button className={format === 'mermaid' ? 'active' : ''} onClick={() => handleFormatChange('mermaid')}>Mermaid</button>
          </div>
          {hasDiagrams && (
            <>
              <div className="format-toggle">
                <button className={viewMode === 'render' ? 'active' : ''} onClick={() => setViewMode('render')}>{t('diagrams.render')}</button>
                <button className={viewMode === 'code' ? 'active' : ''} onClick={() => setViewMode('code')}>{t('diagrams.code')}</button>
              </div>
              <CopyButton text={allMermaidCode} />
              {format === 'mermaid' && (
                <button className="btn btn-sm" onClick={saveMermaid} title={t('diagrams.save')}>
                  <Save size={12} /> {t('diagrams.save')}
                </button>
              )}
            </>
          )}
          <button className="btn btn-primary" onClick={generate} disabled={loading}>
            {loading ? <><span className="loading-spinner" /> {t('diagrams.generating')}</> : <><GitBranch size={12} /> {t('diagrams.generate')}</>}
          </button>
        </div>
      </div>

      {saveMsg && (
        <div className={`save-msg ${saveMsg.startsWith('Error') ? 'error' : ''}`}>
          {saveMsg.startsWith('Error') ? saveMsg : (
            <>
              <FileDown size={14} />
              <span>{t('diagrams.saved')}: {saveMsg}</span>
              {format === 'drawio' && <span className="save-hint">{t('diagrams.drawioHint')}</span>}
            </>
          )}
        </div>
      )}

      {hasDiagrams && (
        <div className="diagrams-content">
          <h3>{t('diagrams.architecture')}</h3>
          {viewMode === 'render' ? (
            <ZoomableMermaid code={diagram!.architecture} />
          ) : (
            <pre className="mermaid-raw">{diagram!.architecture.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
          )}

          {diagram!.api_routes && (
            <>
              <h3>{t('diagrams.apiRoutes')}</h3>
              {viewMode === 'render' ? (
                <ZoomableMermaid code={diagram!.api_routes} />
              ) : (
                <pre className="mermaid-raw">{diagram!.api_routes.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
              )}
            </>
          )}

          {diagram!.db_models && (
            <>
              <h3>{t('diagrams.dbModels')}</h3>
              {viewMode === 'render' ? (
                <ZoomableMermaid code={diagram!.db_models} />
              ) : (
                <pre className="mermaid-raw">{diagram!.db_models.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
              )}
            </>
          )}

          {diagram!.warnings.length > 0 && (
            <div className="warnings">
              <h3>{t('diagrams.warnings')}</h3>
              <ul>{diagram!.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>
            </div>
          )}
        </div>
      )}

      {!hasDiagrams && !loading && (
        <div className="analysis-empty">
          <GitBranch size={32} style={{ opacity: 0.2 }} />
          <p>{t('diagrams.emptyPrompt')}</p>
        </div>
      )}
    </div>
  )
}
