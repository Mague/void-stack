import { useState, useEffect, useRef, useMemo, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { ArrowDownToLine, Search } from 'lucide-react'

interface FilteredLogsResult {
  lines: string[]
  lines_original: number
  lines_filtered: number
  savings_pct: number
}

interface LogImpactResult {
  file: string
  impacted_files: string[]
  impacted_count: number
  truncated: boolean
}

interface Props {
  project: string
  services: string[]
  activeService: string | null
  onSelectService: (name: string) => void
}

type Level = 'error' | 'warn' | 'info' | 'debug'
const LEVELS: Level[] = ['error', 'warn', 'info', 'debug']

interface ParsedLine {
  raw: string
  ts: string | null
  /// Token as written in the line (ERROR, warn, …); null when no level.
  levelToken: string | null
  level: Level
  /// Text after stripping the matched ts/level prefix tokens.
  msg: string
  hasPath: boolean
}

// Tolerant parsers: ISO datetimes or bare clock times; bracketed or bare
// level tokens. Lines without a level default to `info`.
const TS_RE = /\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b|\b\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b/
const LEVEL_RE = /(^|[\s[(])(FATAL|ERROR|ERRO|WARNING|WARN|INFO|DEBUG|DBG|TRACE)([\s\]):]|$)/i
// Mirrors the backend detection closely enough to decide whether to OFFER
// the impact action; the backend does the authoritative extraction.
const PATH_RE = /(?:[\w.-]+[/\\])+[\w.-]+\.\w{1,8}(?::\d+)?/

function toLevel(token: string): Level {
  const t = token.toLowerCase()
  if (t === 'fatal' || t === 'error' || t === 'erro') return 'error'
  if (t === 'warn' || t === 'warning') return 'warn'
  if (t === 'debug' || t === 'dbg' || t === 'trace') return 'debug'
  return 'info'
}

function parseLine(raw: string): ParsedLine {
  const tsMatch = raw.match(TS_RE)
  const levelMatch = raw.match(LEVEL_RE)
  let msg = raw
  if (tsMatch) msg = msg.replace(tsMatch[0], '')
  if (levelMatch) msg = msg.replace(levelMatch[0], levelMatch[1] + levelMatch[3])
  return {
    raw,
    ts: tsMatch ? tsMatch[0] : null,
    levelToken: levelMatch ? levelMatch[2] : null,
    level: levelMatch ? toLevel(levelMatch[2]) : 'info',
    msg: msg.trim(),
    hasPath: PATH_RE.test(raw),
  }
}

const LINE_H = 22 // px, matches .log-content line-height with 13px font
const VIRTUALIZE_AT = 5000
const OVERSCAN = 30

interface Prefs {
  levels: Level[]
  wrap: boolean
  raw: boolean
  filterNoise: boolean
}

const DEFAULT_PREFS: Prefs = { levels: [...LEVELS], wrap: true, raw: false, filterNoise: true }

function loadPrefs(project: string): Prefs {
  try {
    const stored = localStorage.getItem(`void-logs-prefs:${project}`)
    if (stored) return { ...DEFAULT_PREFS, ...JSON.parse(stored) }
  } catch { /* corrupted prefs fall back to defaults */ }
  return DEFAULT_PREFS
}

export default function LogViewer({ project, services, activeService, onSelectService }: Props) {
  const { t } = useTranslation()
  const [logs, setLogs] = useState<string[]>([])
  const [displayLogs, setDisplayLogs] = useState<string[]>([])
  const [savings, setSavings] = useState<number | null>(null)
  const [prefs, setPrefs] = useState<Prefs>(() => loadPrefs(project))
  const [search, setSearch] = useState('')
  const [follow, setFollow] = useState(true)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewH, setViewH] = useState(600)
  const [impactFor, setImpactFor] = useState<number | null>(null)
  const [impact, setImpact] = useState<LogImpactResult | string | null>(null)
  const logRef = useRef<HTMLDivElement>(null)
  const followRef = useRef(true)
  followRef.current = follow

  const selected = activeService || services[0] || null

  // Per-project preference persistence.
  useEffect(() => setPrefs(loadPrefs(project)), [project])
  const setPref = <K extends keyof Prefs>(key: K, value: Prefs[K]) => {
    setPrefs(prev => {
      const next = { ...prev, [key]: value }
      localStorage.setItem(`void-logs-prefs:${project}`, JSON.stringify(next))
      return next
    })
  }

  useEffect(() => {
    if (!selected) return
    setLogs([])
    setDisplayLogs([])
    setSavings(null)
    setImpactFor(null)
    const fetchLogs = async () => {
      try {
        const lines = await invoke<string[]>('get_logs', {
          project,
          service: selected,
          lines: 5000,
        })
        setLogs(lines)
      } catch (e) {
        console.error(e)
      }
    }
    fetchLogs()
    const interval = setInterval(fetchLogs, 1500)
    return () => clearInterval(interval)
  }, [project, selected])

  // Noise filter (backend) when enabled.
  useEffect(() => {
    if (!prefs.filterNoise || logs.length === 0) {
      setDisplayLogs(logs)
      setSavings(null)
      return
    }
    invoke<FilteredLogsResult>('filter_logs_cmd', { rawLines: logs, compact: true })
      .then(result => {
        setDisplayLogs(result.lines)
        setSavings(result.savings_pct)
      })
      .catch(() => {
        setDisplayLogs(logs)
        setSavings(null)
      })
  }, [logs, prefs.filterNoise])

  const parsed = useMemo(() => displayLogs.map(parseLine), [displayLogs])

  const visible = useMemo(() => {
    const q = search.toLowerCase()
    return parsed.filter(p =>
      (prefs.raw || prefs.levels.includes(p.level)) &&
      (q === '' || p.raw.toLowerCase().includes(q))
    )
  }, [parsed, prefs.levels, prefs.raw, search])

  // Follow mode: stick to the bottom until the user scrolls up.
  useEffect(() => {
    if (followRef.current && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [visible])

  const onScroll = useCallback(() => {
    const el = logRef.current
    if (!el) return
    setScrollTop(el.scrollTop)
    setViewH(el.clientHeight)
    const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 30
    if (!atBottom && followRef.current) setFollow(false)
    if (atBottom && !followRef.current) setFollow(true)
  }, [])

  const jumpToLive = () => {
    setFollow(true)
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight
  }

  const toggleLevel = (lvl: Level) => {
    const has = prefs.levels.includes(lvl)
    setPref('levels', has ? prefs.levels.filter(l => l !== lvl) : [...prefs.levels, lvl])
  }

  const runImpact = async (idx: number, line: ParsedLine) => {
    setImpactFor(idx)
    setImpact(null)
    try {
      const res = await invoke<LogImpactResult>('log_impact_cmd', { project, line: line.raw })
      setImpact(res)
    } catch (e) {
      setImpact(String(e))
    }
  }

  // Simple windowing for huge buffers (only without wrapping — wrapped
  // lines have variable heights, so we cap instead).
  const virtualize = !prefs.wrap && visible.length > VIRTUALIZE_AT
  const capped = prefs.wrap && visible.length > VIRTUALIZE_AT
  const shown = capped ? visible.slice(-VIRTUALIZE_AT) : visible
  let start = 0
  let end = shown.length
  if (virtualize) {
    start = Math.max(0, Math.floor(scrollTop / LINE_H) - OVERSCAN)
    end = Math.min(shown.length, start + Math.ceil(viewH / LINE_H) + 2 * OVERSCAN)
  }

  const renderLine = (p: ParsedLine, idx: number) => {
    if (prefs.raw) {
      return <div key={idx} className="log-line">{p.raw}</div>
    }
    return (
      <div key={idx} className="log-line log-line-structured">
        {p.ts && <span className="log-ts">{p.ts} </span>}
        {p.levelToken && (
          <span className={`log-level log-level-${p.level}`}>{p.levelToken.toUpperCase()} </span>
        )}
        <span className={p.level === 'error' && p.levelToken ? 'log-msg-error' : undefined}>
          {p.msg}
        </span>
        {p.level === 'error' && p.hasPath && (
          <button className="log-impact-btn" onClick={() => runImpact(idx, p)}>
            {t('logViewer.impact')}
          </button>
        )}
        {impactFor === idx && (
          <div className="log-impact-box">
            {impact === null ? (
              <span>…</span>
            ) : typeof impact === 'string' ? (
              <span>{impact}</span>
            ) : (
              <>
                <strong>{impact.file}</strong> → {t('logViewer.impactCount', { count: impact.impacted_count })}
                {impact.impacted_files.length > 0 && (
                  <div className="log-impact-files">
                    {impact.impacted_files.slice(0, 8).join(', ')}
                    {impact.impacted_files.length > 8 && ` … +${impact.impacted_files.length - 8}`}
                  </div>
                )}
              </>
            )}
            <button className="log-impact-close" onClick={() => setImpactFor(null)}>×</button>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="panel log-panel">
      <div className="panel-header">
        <h2>{t('logViewer.title')}</h2>
        <div className="log-controls">
          <select value={selected || ''} onChange={e => onSelectService(e.target.value)}>
            {services.map(s => (
              <option key={s} value={s}>{s}</option>
            ))}
          </select>
          {!prefs.raw && (
            <div className="log-level-toggles">
              {LEVELS.map(lvl => (
                <button
                  key={lvl}
                  className={`log-level-toggle log-level-${lvl} ${prefs.levels.includes(lvl) ? 'active' : ''}`}
                  onClick={() => toggleLevel(lvl)}
                >
                  {lvl}
                </button>
              ))}
            </div>
          )}
          <div className="log-search">
            <Search size={12} />
            <input
              type="text"
              placeholder={t('logViewer.search')}
              value={search}
              onChange={e => setSearch(e.target.value)}
            />
          </div>
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={prefs.filterNoise}
              onChange={e => setPref('filterNoise', e.target.checked)}
            />
            {t('logViewer.filterNoise')}
          </label>
          {prefs.filterNoise && savings !== null && savings > 5 && (
            <span className="filter-badge">{t('logViewer.savings', { pct: savings.toFixed(0) })}</span>
          )}
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={prefs.wrap}
              onChange={e => setPref('wrap', e.target.checked)}
            />
            {t('logViewer.wrap')}
          </label>
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={prefs.raw}
              onChange={e => setPref('raw', e.target.checked)}
            />
            {t('logViewer.raw')}
          </label>
        </div>
      </div>
      <div
        className={`log-content ${prefs.wrap ? '' : 'log-nowrap'}`}
        ref={logRef}
        onScroll={onScroll}
      >
        {shown.length === 0 ? (
          <p className="log-empty">{t('logViewer.noLogs')}</p>
        ) : virtualize ? (
          <>
            <div style={{ height: start * LINE_H }} />
            {shown.slice(start, end).map((p, i) => renderLine(p, start + i))}
            <div style={{ height: (shown.length - end) * LINE_H }} />
          </>
        ) : (
          <>
            {capped && (
              <p className="log-empty">{t('logViewer.capped', { count: VIRTUALIZE_AT })}</p>
            )}
            {shown.map(renderLine)}
          </>
        )}
      </div>
      {!follow && (
        <button className="log-jump-live" onClick={jumpToLive}>
          <ArrowDownToLine size={12} /> {t('logViewer.jumpToLive')}
        </button>
      )}
    </div>
  )
}
