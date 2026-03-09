import { useState, useEffect, useRef, useCallback } from 'react'
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

// ── Draw.io XML renderer with maxGraph ──────────────────────

function DrawioViewer({ xml }: { xml: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const graphRef = useRef<InstanceType<typeof import('@maxgraph/core').Graph> | null>(null)
  const [zoom, setZoom] = useState(1)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const renderGraph = useCallback(async () => {
    if (!containerRef.current || !xml) return
    setLoading(true)
    setError(null)

    try {
      const { Graph, ModelXmlSerializer, FitPlugin } = await import('@maxgraph/core')

      // Clean up previous graph instance
      if (graphRef.current) {
        graphRef.current.destroy()
        graphRef.current = null
      }
      containerRef.current.innerHTML = ''

      const graph = new Graph(containerRef.current, undefined, [FitPlugin])
      graphRef.current = graph

      // Configure for read-only viewing
      graph.setEnabled(false)
      graph.setCellsSelectable(false)
      graph.setCellsMovable(false)
      graph.setCellsResizable(false)
      graph.setCellsEditable(false)
      graph.setTooltips(true)

      // Apply dark theme styling
      const stylesheet = graph.getStylesheet()
      const defaultVertex = stylesheet.getDefaultVertexStyle()
      defaultVertex.fillColor = '#1a1a2e'
      defaultVertex.strokeColor = '#00f0ff'
      defaultVertex.fontColor = '#e0e0e0'
      defaultVertex.fontSize = 11
      defaultVertex.rounded = true

      const defaultEdge = stylesheet.getDefaultEdgeStyle()
      defaultEdge.strokeColor = '#00f0ff'
      defaultEdge.fontColor = '#a0a0a0'
      defaultEdge.fontSize = 10

      // Parse and import the Draw.io XML
      const model = graph.getDataModel()
      const serializer = new ModelXmlSerializer(model)

      // Draw.io uses <mxGraphModel> wrapper — extract the inner content if needed
      let xmlContent = xml.trim()
      if (xmlContent.includes('<mxfile')) {
        // Extract the <diagram> content which contains the mxGraphModel
        const diagramMatch = xmlContent.match(/<diagram[^>]*>([\s\S]*?)<\/diagram>/)
        if (diagramMatch) {
          // Draw.io stores base64+deflate encoded content, or raw XML
          const inner = diagramMatch[1].trim()
          if (inner.startsWith('<mxGraphModel')) {
            xmlContent = inner
          } else {
            // Compressed diagram — try to decode
            try {
              const decoded = atob(inner)
              const bytes = new Uint8Array(decoded.length)
              for (let i = 0; i < decoded.length; i++) bytes[i] = decoded.charCodeAt(i)
              const inflated = new TextDecoder().decode(
                new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'))).body
                  ? await new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'))).arrayBuffer()
                    .then(buf => new Uint8Array(buf))
                  : bytes
              )
              const decodedXml = decodeURIComponent(inflated)
              if (decodedXml.includes('<mxGraphModel')) {
                xmlContent = decodedXml
              }
            } catch {
              // If decompression fails, try the raw content
            }
          }
        }
      }

      serializer.import(xmlContent)

      // Fit the diagram to the container
      const fitPlugin = graph.getPlugin<InstanceType<typeof FitPlugin>>('fit')
      if (fitPlugin) {
        fitPlugin.maxFitScale = 2
        fitPlugin.fitCenter({ margin: 20 })
      }

      setLoading(false)
    } catch (e) {
      console.error('Draw.io render error:', e)
      setError(String(e))
      setLoading(false)
    }
  }, [xml])

  useEffect(() => {
    renderGraph()
    return () => {
      if (graphRef.current) {
        graphRef.current.destroy()
        graphRef.current = null
      }
    }
  }, [renderGraph])

  useEffect(() => {
    if (!graphRef.current) return
    const view = graphRef.current.getView()
    view.setScale(zoom)
    graphRef.current.center()
  }, [zoom])

  if (error) {
    return (
      <div className="drawio-fallback">
        <div className="drawio-fallback-header">
          <span style={{ color: 'var(--accent)', fontSize: 11 }}>Draw.io XML</span>
          <span style={{ fontSize: 10, opacity: 0.5 }}>Preview failed — use diagrams.net to view</span>
        </div>
        <pre className="mermaid-raw">{xml.slice(0, 5000)}{xml.length > 5000 ? '\n...' : ''}</pre>
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
      <div className="drawio-render" ref={containerRef} style={{ minHeight: 300, position: 'relative' }}>
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
