import { useState, useEffect, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Search, Network, GitCompare, X } from 'lucide-react'

interface Props {
  project: string
  /** All registered project names (for the cross-project selector). */
  projects: string[]
}

interface Hit {
  file_path: string
  chunk: string
  score: number
  line_start: number
  line_end: number
  origin?: string
}
interface GraphRagResult {
  semantic_seeds: Hit[]
  combined: Hit[]
  communities_hit: number[]
  token_estimate: number
  has_structural_index: boolean
  files_skipped_not_indexed: number
}
interface CrossLink { from_project: string; to_project: string; via: string; shared_symbols: string[] }
interface CrossResult {
  primary: GraphRagResult
  related: [string, Hit[]][]
  cross_links: CrossLink[]
  related_omitted: number
}

type Mode = 'semantic' | 'graphrag' | 'cross'
const MODES: { id: Mode; icon: React.ReactNode }[] = [
  { id: 'semantic', icon: <Search size={13} /> },
  { id: 'graphrag', icon: <Network size={13} /> },
  { id: 'cross', icon: <GitCompare size={13} /> },
]

export default function SearchPanel({ project, projects }: Props) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<Mode>('semantic')
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [elapsed, setElapsed] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [semantic, setSemantic] = useState<Hit[] | null>(null)
  const [graphrag, setGraphrag] = useState<GraphRagResult | null>(null)
  const [cross, setCross] = useState<CrossResult | null>(null)
  const [related, setRelated] = useState<Set<string>>(new Set())
  const runIdRef = useRef(0)

  const others = projects.filter(p => p !== project)

  const clearResults = useCallback(() => {
    setSemantic(null); setGraphrag(null); setCross(null); setError(null)
  }, [])

  // Discard results (and any in-flight run) when the mode or project changes.
  useEffect(() => {
    runIdRef.current++
    setLoading(false)
    clearResults()
  }, [mode, project, clearResults])

  // Reset the cross selection when the project changes.
  useEffect(() => { setRelated(new Set()) }, [project])

  const openHit = (file: string, line: number, proj = project) => {
    invoke('open_in_editor_cmd', { project: proj, file, line }).catch(e => setError(String(e)))
  }

  const cancel = () => { runIdRef.current++; setLoading(false) }

  const run = useCallback(async () => {
    const q = query.trim()
    if (!q) return
    const myId = ++runIdRef.current
    setLoading(true); setError(null); setElapsed(0)
    clearResults()
    const started = Date.now()
    const timer = setInterval(() => {
      if (runIdRef.current === myId) setElapsed(Math.floor((Date.now() - started) / 1000))
    }, 500)
    try {
      if (mode === 'semantic') {
        const json = await invoke<string>('semantic_search_cmd', { projectName: project, query: q, topK: 12 })
        if (runIdRef.current === myId) setSemantic(JSON.parse(json))
      } else if (mode === 'graphrag') {
        const json = await invoke<string>('graph_rag_search_cmd', { projectName: project, query: q, topK: 8, depth: 2 })
        if (runIdRef.current === myId) setGraphrag(JSON.parse(json))
      } else {
        const json = await invoke<string>('graph_rag_search_cross_cmd', {
          projectName: project, query: q, topK: 8, depth: 2,
          related: related.size ? Array.from(related) : null,
        })
        if (runIdRef.current === myId) setCross(JSON.parse(json))
      }
    } catch (e) {
      if (runIdRef.current === myId) setError(String(e))
    } finally {
      clearInterval(timer)
      if (runIdRef.current === myId) setLoading(false)
    }
  }, [mode, query, project, related, clearResults])

  const snippet = (chunk: string) => chunk.split('\n').map(l => l.trim()).filter(Boolean).join(' ').slice(0, 130)

  const hitRow = (h: Hit, i: number, proj = project) => (
    <button className="vs-row vs-hit" key={`${proj}-${i}-${h.line_start}`} onClick={() => openHit(h.file_path, h.line_start, proj)} title={t('search.openHint')}>
      <span className="sev info">{Math.round(h.score * 100)}%</span>
      <span className="what">{snippet(h.chunk)}</span>
      <span className="where">{h.file_path}:{h.line_start}</span>
    </button>
  )

  return (
    <div className="vs-intel-section vs-search-panel">
      <div className="vs-search-modes">
        {MODES.map(m => (
          <button key={m.id} className={`vs-pill ${mode === m.id ? 'active' : ''}`} onClick={() => setMode(m.id)}>
            {m.icon} {t(`search.mode_${m.id}`)}
          </button>
        ))}
      </div>

      {mode === 'cross' && others.length > 0 && (
        <div className="vs-cross-picker">
          <span className="vs-cross-label">{t('search.crossWith')}</span>
          {others.map(p => (
            <button
              key={p}
              className={`vs-chip ${related.has(p) ? 'on' : ''}`}
              onClick={() => setRelated(prev => {
                const next = new Set(prev)
                next.has(p) ? next.delete(p) : next.add(p)
                return next
              })}
            >
              {p}
            </button>
          ))}
          {related.size === 0 && <span className="vs-cross-hint">{t('search.crossAll')}</span>}
        </div>
      )}

      <div className="vs-search-bar">
        <Search size={14} />
        <input
          value={query}
          onChange={e => setQuery(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && run()}
          placeholder={t('search.placeholder')}
          autoFocus
        />
        {loading ? (
          <button className="vs-btn" onClick={cancel}><X size={13} /> {t('common.cancel')} · {elapsed}s</button>
        ) : (
          <button className="vs-btn" onClick={run} disabled={!query.trim()}>
            <Search size={13} /> {t('search.run')}
          </button>
        )}
      </div>

      {error && <p className="vs-search-err">{error}</p>}
      {loading && <p className="vs-search-meta">{t('search.searching', { secs: elapsed })}</p>}

      <div className="vs-search-results">
        {/* Semantic */}
        {semantic && (
          semantic.length ? semantic.map((h, i) => hitRow(h, i)) : <p className="vs-search-meta">{t('search.noResults')}</p>
        )}

        {/* GraphRAG */}
        {graphrag && (
          <>
            <h2>{t('search.graphragMeta', { tokens: graphrag.token_estimate, communities: graphrag.communities_hit.length })}</h2>
            {!graphrag.has_structural_index && <p className="vs-search-warn">{t('search.noStructural')}</p>}
            {graphrag.combined.length
              ? graphrag.combined.map((h, i) => hitRow(h, i))
              : <p className="vs-search-meta">{t('search.noResults')}</p>}
          </>
        )}

        {/* Cross-project */}
        {cross && (
          <>
            <h2>{project}</h2>
            {cross.primary.combined.slice(0, 8).map((h, i) => hitRow(h, i))}
            {cross.cross_links.length > 0 && (
              <>
                <h2 style={{ marginTop: 14 }}>{t('search.links')}</h2>
                {cross.cross_links.map((l, i) => (
                  <div className="vs-row" key={i}>
                    <span className="sev info">{l.via}</span>
                    <span className="what">{l.from_project} → {l.to_project}{l.shared_symbols.length ? ` · ${l.shared_symbols.slice(0, 4).join(', ')}` : ''}</span>
                  </div>
                ))}
              </>
            )}
            {cross.related.map(([name, hits]) => (
              <div key={name}>
                <h2 style={{ marginTop: 14 }}>{name}</h2>
                {hits.map((h, i) => hitRow(h, i, name))}
              </div>
            ))}
            {cross.related_omitted > 0 && (
              <p className="vs-search-meta">{t('search.omitted', { count: cross.related_omitted })}</p>
            )}
          </>
        )}
      </div>
    </div>
  )
}
