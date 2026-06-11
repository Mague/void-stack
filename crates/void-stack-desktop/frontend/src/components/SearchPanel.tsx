import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Search, RefreshCw, Network, GitCompare } from 'lucide-react'

interface Props {
  project: string
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

export default function SearchPanel({ project }: Props) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<Mode>('semantic')
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [semantic, setSemantic] = useState<Hit[] | null>(null)
  const [graphrag, setGraphrag] = useState<GraphRagResult | null>(null)
  const [cross, setCross] = useState<CrossResult | null>(null)

  const openHit = (file: string, line: number, proj = project) => {
    invoke('open_in_editor_cmd', { project: proj, file, line }).catch(e => setError(String(e)))
  }

  const run = useCallback(async () => {
    const q = query.trim()
    if (!q) return
    setLoading(true); setError(null)
    setSemantic(null); setGraphrag(null); setCross(null)
    try {
      if (mode === 'semantic') {
        const json = await invoke<string>('semantic_search_cmd', { projectName: project, query: q, topK: 12 })
        setSemantic(JSON.parse(json))
      } else if (mode === 'graphrag') {
        const json = await invoke<string>('graph_rag_search_cmd', { projectName: project, query: q, topK: 8, depth: 2 })
        setGraphrag(JSON.parse(json))
      } else {
        const json = await invoke<string>('graph_rag_search_cross_cmd', { projectName: project, query: q, topK: 8, depth: 2 })
        setCross(JSON.parse(json))
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [mode, query, project])

  const snippet = (chunk: string) => chunk.split('\n').slice(0, 2).join(' ').slice(0, 120)

  const hitRow = (h: Hit, i: number, proj = project) => (
    <button className="vs-row vs-hit" key={`${proj}-${i}-${h.file_path}-${h.line_start}`} onClick={() => openHit(h.file_path, h.line_start, proj)}>
      <span className="sev info">{Math.round(h.score * 100)}%</span>
      <span className="what">{snippet(h.chunk)}</span>
      <span className="where">{h.file_path}:{h.line_start}</span>
    </button>
  )

  return (
    <div className="vs-intel-section" style={{ overflow: 'auto' }}>
      <div className="vs-search-modes">
        {MODES.map(m => (
          <button key={m.id} className={`vs-pill ${mode === m.id ? 'active' : ''}`} onClick={() => setMode(m.id)}>
            {m.icon} {t(`search.mode_${m.id}`)}
          </button>
        ))}
      </div>

      <div className="vs-search-bar">
        <Search size={14} />
        <input
          value={query}
          onChange={e => setQuery(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && run()}
          placeholder={t('search.placeholder')}
          autoFocus
        />
        <button className="vs-btn" onClick={run} disabled={loading || !query.trim()}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <Search size={13} />}
          {t('search.run')}
        </button>
      </div>

      {error && <p style={{ color: 'var(--vs-err)', fontSize: 12, marginTop: 8 }}>{error}</p>}

      {/* Semantic */}
      {semantic && (
        semantic.length ? semantic.map((h, i) => hitRow(h, i)) : <div className="vs-row"><span className="what">{t('search.noResults')}</span></div>
      )}

      {/* GraphRAG */}
      {graphrag && (
        <>
          <h2>{t('search.graphragMeta', { tokens: graphrag.token_estimate, communities: graphrag.communities_hit.length })}</h2>
          {!graphrag.has_structural_index && (
            <p style={{ fontSize: 11, color: 'var(--vs-warn)' }}>{t('search.noStructural')}</p>
          )}
          {graphrag.combined.length
            ? graphrag.combined.map((h, i) => hitRow(h, i))
            : <div className="vs-row"><span className="what">{t('search.noResults')}</span></div>}
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
            <p style={{ fontSize: 11, color: 'var(--vs-text-3)', marginTop: 8 }}>{t('search.omitted', { count: cross.related_omitted })}</p>
          )}
        </>
      )}
    </div>
  )
}
