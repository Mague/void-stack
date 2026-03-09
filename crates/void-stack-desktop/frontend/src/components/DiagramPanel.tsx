import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import DOMPurify from 'dompurify'
import type { DiagramResult } from '../types'
import { GitBranch, Save, FileDown } from 'lucide-react'
import CopyButton from './CopyButton'

interface Props {
  project: string
  diagram: DiagramResult | null
  setDiagram: (d: DiagramResult | null) => void
}

// ── Mermaid renderer with DOMPurify ─────────────────────────

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
          containerRef.current.innerHTML = DOMPurify.sanitize(svg, {
            USE_PROFILES: { svg: true, svgFilters: true },
            ADD_TAGS: ['foreignObject'],
          })
          setRendered(true)
        }
      } catch {
        if (containerRef.current) {
          containerRef.current.textContent = clean
          containerRef.current.classList.add('mermaid-raw')
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

// ── Draw.io XML renderer (isolated container) ───────────────

function extractMxGraphModel(xml: string): string {
  let content = xml.trim()
  if (content.includes('<mxfile')) {
    const diagramMatch = content.match(/<diagram[^>]*>([\s\S]*?)<\/diagram>/)
    if (diagramMatch) {
      const inner = diagramMatch[1].trim()
      if (inner.startsWith('<mxGraphModel')) {
        content = inner
      }
    }
  }
  if (!content.includes('<mxGraphModel')) {
    throw new Error('No mxGraphModel found in XML')
  }
  return content
}

function DrawioViewer({ xml }: { xml: string }) {
  const outerRef = useRef<HTMLDivElement>(null)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const graphRef = useRef<any>(null)
  const graphContainerRef = useRef<HTMLDivElement | null>(null)
  const [zoom, setZoom] = useState(1)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!outerRef.current || !xml) return
    let cancelled = false

    // Destroy previous graph
    try {
      if (graphRef.current) {
        graphRef.current.destroy()
        graphRef.current = null
      }
    } catch { /* ignore cleanup errors */ }

    // Create a fresh isolated container for maxGraph (not managed by React)
    if (graphContainerRef.current && outerRef.current.contains(graphContainerRef.current)) {
      outerRef.current.removeChild(graphContainerRef.current)
    }
    const graphDiv = document.createElement('div')
    graphDiv.style.cssText = 'width:100%;min-height:400px;overflow:auto;background:#0a0a14;border-radius:8px;'
    outerRef.current.appendChild(graphDiv)
    graphContainerRef.current = graphDiv

    setLoading(true)
    setError(null)
    setZoom(1)

    const renderAsync = async () => {
      try {
        const mxGraphXml = extractMxGraphModel(xml)
        const { Graph, ModelXmlSerializer, FitPlugin, InternalEvent } = await import('@maxgraph/core')

        if (cancelled) return

        InternalEvent.disableContextMenu(graphDiv)
        const graph = new Graph(graphDiv, undefined, [FitPlugin])
        graphRef.current = graph

        graph.setEnabled(false)
        graph.setCellsSelectable(false)
        graph.setCellsMovable(false)
        graph.setCellsResizable(false)
        graph.setCellsEditable(false)
        graph.setTooltips(true)

        // Dark theme
        const ss = graph.getStylesheet()
        const dv = ss.getDefaultVertexStyle()
        dv.fillColor = '#1a1a2e'
        dv.strokeColor = '#00f0ff'
        dv.fontColor = '#e0e0e0'
        dv.fontSize = 11
        dv.rounded = true

        const de = ss.getDefaultEdgeStyle()
        de.strokeColor = '#00f0ff'
        de.fontColor = '#a0a0a0'
        de.fontSize = 10

        const model = graph.getDataModel()
        const serializer = new ModelXmlSerializer(model)
        serializer.import(mxGraphXml)

        const fitPlugin = graph.getPlugin<InstanceType<typeof FitPlugin>>('fit')
        if (fitPlugin) {
          fitPlugin.maxFitScale = 2
          fitPlugin.fitCenter({ margin: 20 })
        }

        if (!cancelled) setLoading(false)
      } catch (e) {
        console.error('Draw.io render error:', e)
        if (!cancelled) {
          setError(String(e))
          setLoading(false)
          // Clean up the broken graph container
          if (graphRef.current) {
            try { graphRef.current.destroy() } catch { /* */ }
            graphRef.current = null
          }
          if (graphDiv.parentElement) {
            graphDiv.innerHTML = ''
          }
        }
      }
    }

    renderAsync()

    return () => {
      cancelled = true
      try {
        if (graphRef.current) {
          graphRef.current.destroy()
          graphRef.current = null
        }
      } catch { /* ignore */ }
    }
  }, [xml])

  useEffect(() => {
    if (!graphRef.current) return
    try {
      const view = graphRef.current.getView()
      view.setScale(zoom)
    } catch { /* ignore zoom errors */ }
  }, [zoom])

  if (error) {
    return (
      <div className="drawio-fallback">
        <div className="drawio-fallback-header">
          <span style={{ color: 'var(--accent)', fontSize: 11 }}>Draw.io XML</span>
          <span style={{ fontSize: 10, opacity: 0.5 }}>Use diagrams.net to view — file auto-saved</span>
        </div>
        <pre className="mermaid-raw drawio-xml-code">{xml.slice(0, 5000)}{xml.length > 5000 ? '\n...' : ''}</pre>
      </div>
    )
  }

  return (
    <div>
      <div className="mermaid-zoom-controls">
        <button onClick={() => setZoom(z => Math.max(0.25, z - 0.25))}>-</button>
        <button onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
        <button onClick={() => setZoom(z => Math.min(3, z + 0.25))}>+</button>
      </div>
      <div className="drawio-render" ref={outerRef}>
        {loading && <div className="drawio-loading"><span className="loading-spinner" /> Rendering diagram...</div>}
      </div>
    </div>
  )
}

// ── Main Panel ──────────────────────────────────────────────

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

  const allCode = diagram
    ? [diagram.architecture, diagram.api_routes, diagram.db_models]
        .filter(Boolean)
        .map(c => c!.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim())
        .join('\n\n')
    : ''

  const hasDiagrams = diagram && (diagram.architecture || diagram.api_routes || diagram.db_models)
  const isMermaid = format === 'mermaid'

  const renderSection = (title: string, content: string | undefined) => {
    if (!content) return null
    return (
      <>
        <h3>{title}</h3>
        {viewMode === 'render' ? (
          isMermaid ? (
            <ZoomableMermaid code={content} />
          ) : (
            <DrawioViewer xml={content} />
          )
        ) : (
          <pre className="mermaid-raw">{content.replace(/```mermaid\n?/g, '').replace(/```\n?/g, '').trim()}</pre>
        )}
      </>
    )
  }

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
              <CopyButton text={allCode} />
              {isMermaid && (
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
              {!isMermaid && <span className="save-hint">{t('diagrams.drawioHint')}</span>}
            </>
          )}
        </div>
      )}

      {hasDiagrams && (
        <div className="diagrams-content">
          {renderSection(t('diagrams.architecture'), diagram!.architecture)}
          {renderSection(t('diagrams.apiRoutes'), diagram!.api_routes ?? undefined)}
          {renderSection(t('diagrams.dbModels'), diagram!.db_models ?? undefined)}

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
