import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'

interface Props {
  project: string
  services: string[]
  activeService: string | null
  onSelectService: (name: string) => void
}

export default function LogViewer({ project, services, activeService, onSelectService }: Props) {
  const { t } = useTranslation()
  const [logs, setLogs] = useState<string[]>([])
  const [autoScroll, setAutoScroll] = useState(true)
  const logRef = useRef<HTMLDivElement>(null)

  const selected = activeService || services[0] || null

  useEffect(() => {
    if (!selected) return
    setLogs([]) // Clear logs when switching service
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

  useEffect(() => {
    if (autoScroll && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [logs, autoScroll])

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
              checked={autoScroll}
              onChange={e => setAutoScroll(e.target.checked)}
            />
            {t('logViewer.autoScroll')}
          </label>
        </div>
      </div>
      <div className="log-content" ref={logRef}>
        {logs.length === 0 ? (
          <p className="log-empty">{t('logViewer.noLogs')}</p>
        ) : (
          logs.map((line, i) => (
            <div key={i} className="log-line">{line}</div>
          ))
        )}
      </div>
    </div>
  )
}
