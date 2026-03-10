import { useState, useEffect } from 'react'
import type { ServiceStateDto } from '../types'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Play, Square, FileText, Globe, Clock, Hash, Monitor, Terminal, Container, Trash2 } from 'lucide-react'
import TechIcon from './TechIcon'

interface Props {
  name: string
  command: string
  target: string
  tech: string
  state?: ServiceStateDto
  loading?: boolean
  projectName: string
  dockerPorts?: string[]
  dockerVolumes?: string[]
  onStart: () => void
  onStop: () => void
  onViewLogs: () => void
  onRemove?: () => void
}

function formatUptime(startedAt: string | null): string {
  if (!startedAt) return '-'
  const start = new Date(startedAt).getTime()
  const now = Date.now()
  const seconds = Math.floor((now - start) / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

function targetIcon(target: string) {
  const t = target.toLowerCase()
  if (t === 'wsl' || t === 'linux') return <Terminal size={10} />
  if (t === 'docker') return <Container size={10} />
  return <Monitor size={10} />
}

export default function ServiceCard({ name, command, target, tech, state, loading, projectName, dockerPorts, dockerVolumes, onStart, onStop, onViewLogs, onRemove }: Props) {
  const { t } = useTranslation()
  const status = state?.status || 'STOPPED'
  const isRunning = status === 'RUNNING'
  const isTransitional = status === 'STARTING' || status === 'STOPPING'
  const [lastLog, setLastLog] = useState<string | null>(null)
  const [confirmRemove, setConfirmRemove] = useState(false)

  // Fetch last log line for running services
  useEffect(() => {
    if (!isRunning) {
      setLastLog(null)
      return
    }
    const fetchLastLog = async () => {
      try {
        const lines = await invoke<string[]>('get_logs', {
          project: projectName,
          service: name,
          lines: 1,
        })
        if (lines.length > 0) {
          setLastLog(lines[lines.length - 1])
        }
      } catch {
        // ignore
      }
    }
    fetchLastLog()
    const interval = setInterval(fetchLastLog, 3000)
    return () => clearInterval(interval)
  }, [isRunning, projectName, name])

  return (
    <div className={`service-card status-${status.toLowerCase()}`}>
      <div className="card-header">
        <div className="card-title">
          <span className={`status-dot ${status.toLowerCase()} ${isTransitional ? 'pulse' : ''}`} />
          <TechIcon tech={tech} size={18} />
          <h3>{name}</h3>
        </div>
        <span className="target-badge-env">
          {targetIcon(target)}
          <span>{target}</span>
        </span>
      </div>

      <div className="card-body">
        <code className="command">{command}</code>

        {target.toLowerCase() === 'docker' && (dockerPorts?.length || dockerVolumes?.length) && (
          <div className="docker-info">
            {dockerPorts && dockerPorts.length > 0 && (
              <span className="meta-item" title="Ports">🔌 {dockerPorts.join(', ')}</span>
            )}
            {dockerVolumes && dockerVolumes.length > 0 && (
              <span className="meta-item" title="Volumes">💾 {dockerVolumes.join(', ')}</span>
            )}
          </div>
        )}

        {isRunning && (
          <div className="card-meta">
            {state?.pid && (
              <span className="meta-item">
                <Hash size={10} /> {state.pid}
              </span>
            )}
            {state?.started_at && (
              <span className="meta-item">
                <Clock size={10} /> {formatUptime(state.started_at)}
              </span>
            )}
            {state?.url && (
              <a className="meta-item url" href={state.url} target="_blank" rel="noopener">
                <Globe size={10} /> {state.url}
              </a>
            )}
          </div>
        )}

        {/* Mini-log preview */}
        {isRunning && lastLog && (
          <div className="card-mini-log" onClick={onViewLogs} title={lastLog}>
            <span className="mini-log-dot" />
            <span className="mini-log-text">{lastLog}</span>
          </div>
        )}
      </div>

      <div className="card-actions">
        {isRunning ? (
          <button className="btn btn-danger btn-sm" onClick={onStop} disabled={loading}>
            {loading ? <span className="loading-spinner" /> : <Square size={10} />}
            {' '}{t('services.stop')}
          </button>
        ) : (
          <button className="btn btn-success btn-sm" onClick={onStart} disabled={loading}>
            {loading ? <span className="loading-spinner" /> : <Play size={10} />}
            {' '}{t('services.start')}
          </button>
        )}
        <button className="btn btn-sm" onClick={onViewLogs}>
          <FileText size={10} /> {t('services.logs')}
        </button>
        {onRemove && !isRunning && (
          confirmRemove ? (
            <div className="confirm-remove">
              <button className="btn btn-danger btn-sm" onClick={() => { onRemove(); setConfirmRemove(false) }}>
                {t('common.delete')}
              </button>
              <button className="btn btn-sm" onClick={() => setConfirmRemove(false)}>
                {t('common.cancel')}
              </button>
            </div>
          ) : (
            <button className="btn btn-sm btn-icon" onClick={() => setConfirmRemove(true)} title={t('common.delete')}>
              <Trash2 size={10} />
            </button>
          )
        )}
      </div>
    </div>
  )
}
