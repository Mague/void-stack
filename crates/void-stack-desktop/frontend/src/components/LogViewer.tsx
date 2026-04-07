import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'

interface FilteredLogsResult {
  lines: string[]
  lines_original: number
  lines_filtered: number
  savings_pct: number
}

interface Props {
  project: string
  services: string[]
  activeService: string | null
  onSelectService: (name: string) => void
}

export default function LogViewer({ project, services, activeService, onSelectService }: Props) {
  const { t } = useTranslation()
  const [logs, setLogs] = useState<string[]>([])
  const [displayLogs, setDisplayLogs] = useState<string[]>([])
  const [autoScroll, setAutoScroll] = useState(true)
  const [filterActive, setFilterActive] = useState(true)
  const [savings, setSavings] = useState<number | null>(null)
  const logRef = useRef<HTMLDivElement>(null)

  const selected = activeService || services[0] || null

  useEffect(() => {
    if (!selected) return
    setLogs([])
    setDisplayLogs([])
    setSavings(null)
    const fetchLogs = async () => {
      try {
        const lines = await invoke<string[]>('get_logs', {
          project,
          service: selected,
          lines: 200,
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

  // Apply filter when logs or filter state changes
  useEffect(() => {
    if (!filterActive || logs.length === 0) {
      setDisplayLogs(logs)
      setSavings(null)
      return
    }
    invoke<FilteredLogsResult>('filter_logs_cmd', {
      rawLines: logs,
      compact: true,
    }).then(result => {
      setDisplayLogs(result.lines)
      setSavings(result.savings_pct)
    }).catch(() => {
      setDisplayLogs(logs)
      setSavings(null)
    })
  }, [logs, filterActive])

  useEffect(() => {
    if (autoScroll && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [displayLogs, autoScroll])

  return (
    <div className="panel log-panel">
      <div className="panel-header">
        <h2>{t('logViewer.title')}</h2>
        <div className="log-controls">
          <select
            value={selected || ''}
            onChange={e => onSelectService(e.target.value)}
          >
            {services.map(s => (
              <option key={s} value={s}>{s}</option>
            ))}
          </select>
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={filterActive}
              onChange={e => setFilterActive(e.target.checked)}
            />
            {t('logViewer.filterNoise')}
          </label>
          {filterActive && savings !== null && savings > 5 && (
            <span className="filter-badge">{t('logViewer.savings', { pct: savings.toFixed(0) })}</span>
          )}
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={e => setAutoScroll(e.target.checked)}
            />
            {t('logViewer.autoScroll')}
          </label>
        </div>
      </div>
      <div className="log-content" ref={logRef}>
        {displayLogs.length === 0 ? (
          <p className="log-empty">{t('logViewer.noLogs')}</p>
        ) : (
          displayLogs.map((line, i) => (
            <div key={i} className="log-line">{line}</div>
          ))
        )}
      </div>
    </div>
  )
}
