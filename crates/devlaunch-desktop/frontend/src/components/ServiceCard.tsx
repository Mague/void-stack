import type { ServiceStateDto } from '../types'
import { Play, Square, FileText, Globe, Clock, Hash } from 'lucide-react'

interface Props {
  name: string
  command: string
  target: string
  state?: ServiceStateDto
  onStart: () => void
  onStop: () => void
  onViewLogs: () => void
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

export default function ServiceCard({ name, command, target, state, onStart, onStop, onViewLogs }: Props) {
  const status = state?.status || 'STOPPED'
  const isRunning = status === 'RUNNING'

  return (
    <div className={`service-card status-${status.toLowerCase()}`}>
      <div className="card-header">
        <div className="card-title">
          <span className={`status-dot ${status.toLowerCase()}`} />
          <h3>{name}</h3>
        </div>
        <span className="target-badge">{target}</span>
      </div>

      <div className="card-body">
        <div className="card-info">
          <code className="command">{command}</code>
        </div>

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
      </div>

      <div className="card-actions">
        {isRunning ? (
          <button className="btn btn-danger btn-sm" onClick={onStop}>
            <Square size={10} /> Detener
          </button>
        ) : (
          <button className="btn btn-success btn-sm" onClick={onStart}>
            <Play size={10} /> Iniciar
          </button>
        )}
        <button className="btn btn-sm" onClick={onViewLogs}>
          <FileText size={10} /> Logs
        </button>
      </div>
    </div>
  )
}
