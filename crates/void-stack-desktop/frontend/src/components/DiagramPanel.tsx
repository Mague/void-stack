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
// Mermaid SVG output is from a trusted library, not user input.
// We sanitize to satisfy audit but allow all SVG features Mermaid uses.

function sanitizeSvg(svg: string): string {
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { html: true, svg: true, svgFilters: true },
    ADD_TAGS: ['foreignObject', 'style'],
    ADD_ATTR: [
      'marker-end', 'marker-start', 'dominant-baseline', 'text-anchor',
      'transform', 'viewBox', 'xmlns', 'xmlns:xlink', 'xlink:href',
      'clip-path', 'fill-opacity', 'stroke-opacity', 'stroke-dasharray',
      'stroke-width', 'font-family', 'font-size', 'font-weight',
      'text-decoration', 'alignment-baseline', 'letter-spacing',
      'class', 'id', 'rx', 'ry', 'cx', 'cy', 'r', 'x', 'y',
      'x1', 'y1', 'x2', 'y2', 'dx', 'dy', 'width', 'height',
      'd', 'points', 'fill', 'stroke', 'opacity', 'style',
      'refX', 'refY', 'markerWidth', 'markerHeight', 'orient',
      'preserveAspectRatio', 'patternUnits', 'gradientTransform',
    ],
    WHOLE_DOCUMENT: false,
    RETURN_DOM: false,
  })
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
          containerRef.current.innerHTML = sanitizeSvg(svg)
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

// ── Draw.io XML → inline SVG renderer ───────────────────────
// Parses mxGraphModel XML and renders cells as SVG directly,
// avoiding maxGraph DOM manipulation issues in Tauri webview.

interface MxCell {
  id: string
  value: string
  parent: string
  vertex: boolean
  edge: boolean
  source?: string
  target?: string
  x: number
  y: number
  width: number
  height: number
  style: Record<string, string>
}

function parseMxCells(xml: string): MxCell[] {
  const parser = new DOMParser()
  const doc = parser.parseFromString(xml, 'text/xml')
  const cells: MxCell[] = []

  const mxCells = doc.querySelectorAll('mxCell')
  for (const cell of mxCells) {
    const id = cell.getAttribute('id') || ''
    const value = cell.getAttribute('value') || ''
    const parent = cell.getAttribute('parent') || ''
    const vertex = cell.getAttribute('vertex') === '1'
    const edge = cell.getAttribute('edge') === '1'
    const source = cell.getAttribute('source') || undefined
    const target = cell.getAttribute('target') || undefined

    const geo = cell.querySelector('mxGeometry')
    const x = parseFloat(geo?.getAttribute('x') || '0')
    const y = parseFloat(geo?.getAttribute('y') || '0')
    const width = parseFloat(geo?.getAttribute('width') || '100')
    const height = parseFloat(geo?.getAttribute('height') || '40')

    // Parse style string "key=value;key2=value2"
    const styleStr = cell.getAttribute('style') || ''
    const style: Record<string, string> = {}
    for (const part of styleStr.split(';')) {
      const [k, v] = part.split('=')
      if (k && v !== undefined) style[k.trim()] = v.trim()
    }

    cells.push({ id, value, parent, vertex, edge, source, target, x, y, width, height, style })
  }

  return cells
}

function DrawioViewer({ xml }: { xml: string }) {
  const [zoom, setZoom] = useState(1)
  const [svgContent, setSvgContent] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    setZoom(1)
    setError(null)
    try {
      // Extract mxGraphModel from <mxfile> wrapper if needed
      let content = xml.trim()
      if (content.includes('<mxfile')) {
        const m = content.match(/<diagram[^>]*>([\s\S]*?)<\/diagram>/)
        if (m) {
          const inner = m[1].trim()
          if (inner.startsWith('<mxGraphModel')) content = inner
        }
      }
      if (!content.includes('<mxGraphModel')) {
        setError('No mxGraphModel found')
        return
      }

      const cells = parseMxCells(content)
      const vertices = cells.filter(c => c.vertex && c.parent !== '0')
      const edges = cells.filter(c => c.edge)

      if (vertices.length === 0) {
        setError('No cells to render')
        return
      }

      // Calculate bounds
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
      for (const v of vertices) {
        // Cells with parent !== '1' are children — add parent offset
        const parentCell = cells.find(c => c.id === v.parent && c.vertex)
        const ox = parentCell ? parentCell.x : 0
        const oy = parentCell ? parentCell.y : 0
        const ax = v.x + ox
        const ay = v.y + oy
        minX = Math.min(minX, ax)
        minY = Math.min(minY, ay)
        maxX = Math.max(maxX, ax + v.width)
        maxY = Math.max(maxY, ay + v.height)
      }

      const pad = 40
      const vw = maxX - minX + pad * 2
      const vh = maxY - minY + pad * 2

      const lines: string[] = []
      lines.push(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${vw} ${vh}" width="${vw}" height="${vh}">`)
      lines.push('<defs>')
      lines.push('<marker id="arrow" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto"><polygon points="0 0, 10 3.5, 0 7" fill="#00f0ff"/></marker>')
      lines.push('</defs>')
      lines.push(`<rect width="${vw}" height="${vh}" fill="#0a0a14" rx="8"/>`)

      // Render containers first (cells whose id is parent of other cells)
      const containerIds = new Set(vertices.filter(v => v.parent !== '1').map(v => v.parent))
      const containers = vertices.filter(v => containerIds.has(v.id))
      const nonContainers = vertices.filter(v => !containerIds.has(v.id))

      // Draw containers
      for (const v of containers) {
        const rx = v.x - minX + pad
        const ry = v.y - minY + pad
        const fill = v.style.fillColor || '#1a1a2e'
        const stroke = v.style.strokeColor || '#00f0ff'
        lines.push(`<rect x="${rx}" y="${ry}" width="${v.width}" height="${v.height}" rx="8" fill="${fill}" fill-opacity="0.15" stroke="${stroke}" stroke-opacity="0.3" stroke-width="1.5" stroke-dasharray="6 3"/>`)
        // Container label at top
        const label = v.value.replace(/<[^>]*>/g, '').split('\n')[0].trim()
        if (label) {
          lines.push(`<text x="${rx + v.width / 2}" y="${ry + 20}" text-anchor="middle" fill="#e0e0e0" font-family="'JetBrains Mono',monospace" font-size="13" font-weight="700" opacity="0.7">${escapeXml(label)}</text>`)
        }
      }

      // Draw vertices
      for (const v of nonContainers) {
        const parentCell = cells.find(c => c.id === v.parent && c.vertex)
        const ox = parentCell ? parentCell.x : 0
        const oy = parentCell ? parentCell.y : 0
        const rx = v.x + ox - minX + pad
        const ry = v.y + oy - minY + pad
        const fill = v.style.fillColor || '#1a1a2e'
        const stroke = v.style.strokeColor || '#00f0ff'
        const isRounded = v.style.rounded === '1' || !v.style.rounded

        lines.push(`<rect x="${rx}" y="${ry}" width="${v.width}" height="${v.height}" rx="${isRounded ? 8 : 2}" fill="${fill}" stroke="${stroke}" stroke-width="1.5"/>`)

        // Label (strip HTML, split lines)
        const label = v.value.replace(/<br\s*\/?>/gi, '\n').replace(/<[^>]*>/g, '')
        const labelLines = label.split('\n').map(l => l.trim()).filter(Boolean)
        const lineHeight = 14
        const startY = ry + v.height / 2 - ((labelLines.length - 1) * lineHeight) / 2

        for (let i = 0; i < labelLines.length; i++) {
          const fontWeight = i === 0 ? '600' : '400'
          const fontSize = i === 0 ? 11 : 10
          const fillColor = i === 0 ? '#e0e0e0' : '#a0a0c0'
          lines.push(`<text x="${rx + v.width / 2}" y="${startY + i * lineHeight}" text-anchor="middle" dominant-baseline="central" fill="${fillColor}" font-family="'JetBrains Mono',monospace" font-size="${fontSize}" font-weight="${fontWeight}">${escapeXml(labelLines[i])}</text>`)
        }
      }

      // Draw edges
      for (const e of edges) {
        const src = cells.find(c => c.id === e.source)
        const tgt = cells.find(c => c.id === e.target)
        if (!src || !tgt) continue

        const srcParent = cells.find(c => c.id === src.parent && c.vertex)
        const tgtParent = cells.find(c => c.id === tgt.parent && c.vertex)
        const sox = srcParent ? srcParent.x : 0
        const soy = srcParent ? srcParent.y : 0
        const tox = tgtParent ? tgtParent.x : 0
        const toy = tgtParent ? tgtParent.y : 0

        const x1 = src.x + sox + src.width / 2 - minX + pad
        const y1 = src.y + soy + src.height - minY + pad
        const x2 = tgt.x + tox + tgt.width / 2 - minX + pad
        const y2 = tgt.y + toy - minY + pad
        const stroke = e.style.strokeColor || '#00f0ff'

        lines.push(`<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${stroke}" stroke-width="1.5" stroke-opacity="0.6" marker-end="url(#arrow)"/>`)
      }

      lines.push('</svg>')
      setSvgContent(lines.join('\n'))
    } catch (e) {
      setError(String(e))
    }
  }, [xml])

  useEffect(() => {
    if (!containerRef.current) return
    const svg = containerRef.current.querySelector('svg')
    if (svg) {
      svg.style.transform = `scale(${zoom})`
      svg.style.transformOrigin = 'top left'
    }
  }, [zoom, svgContent])

  if (error || !svgContent) {
    return (
      <div className="drawio-fallback">
        <div className="drawio-fallback-header">
          <span style={{ color: 'var(--accent)', fontSize: 11 }}>Draw.io XML</span>
          <span style={{ fontSize: 10, opacity: 0.5 }}>File auto-saved — open with diagrams.net for full view</span>
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
      <div
        className="mermaid-render"
        ref={containerRef}
        dangerouslySetInnerHTML={{ __html: sanitizeSvg(svgContent) }}
      />
    </div>
  )
}

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
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
