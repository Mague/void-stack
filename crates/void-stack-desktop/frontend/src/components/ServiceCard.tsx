import { useState, useEffect } from 'react'
import type { ServiceStateDto } from '../types'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Play, Square, FileText, Globe, Clock, Hash, Trash2 } from 'lucide-react'
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

/** Windows logo mini SVG */
function WindowsIcon({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor">
      <path d="M0 2.3l6.5-.9v6.3H0V2.3zm7.3-1l8.7-1.3v8.7H7.3V1.3zM16 9.5v8.5l-8.7-1.2V9.5H16zM6.5 16.6L0 15.7V9.5h6.5v7.1z"/>
    </svg>
  )
}

/** Linux/Tux mini SVG */
function LinuxIcon({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 2C9.8 2 8.5 4 8.5 6.5c0 1.5.4 2.8 1 3.8-.8.5-2.5 1.8-3.2 3.5-.8 2-.3 4.2 1.5 5.2.6.3 1.2.5 1.8.5.8 0 1.5-.3 2-.7.5.4 1.3.7 2.4.7s1.9-.3 2.4-.7c.5.4 1.2.7 2 .7.6 0 1.2-.2 1.8-.5 1.8-1 2.3-3.2 1.5-5.2-.7-1.7-2.4-3-3.2-3.5.6-1 1-2.3 1-3.8C15.5 4 14.2 2 12 2zm-2 5c.6 0 1 .4 1 1s-.4 1-1 1-1-.4-1-1 .4-1 1-1zm4 0c.6 0 1 .4 1 1s-.4 1-1 1-1-.4-1-1 .4-1 1-1zm-3 3.5h2l-1 2-1-2z"/>
    </svg>
  )
}

/** Docker whale mini SVG */
function DockerIcon({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="#2496ED">
      <path d="M13.5 2.5h2v2h-2zm-3 0h2v2h-2zm-3 0h2v2h-2zm-3 2h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm-3 2h2v2h-2z"/>
      <path d="M23.5 9.8c-.7-.4-2.2-.6-3.4-.3-.2-1.3-.9-2.4-1.8-3.2l-.6-.5-.5.6c-.6.8-1 1.9-.9 2.8 0 .5.1 1 .4 1.5-.6.3-1.2.5-1.8.6H.8c-.3 1.6-.3 3.3.2 4.9.6 1.8 1.7 3.2 3.3 4.1 1.8 1 4.5 1.3 7 .5 1.9-.6 3.5-1.6 4.8-3.2 1-1.3 1.7-2.8 2.1-4.5h.2c1.1 0 2-.4 2.6-1.2.3-.4.5-1 .5-1.6v-.5z"/>
    </svg>
  )
}

function targetIcon(target: string) {
  const t = target.toLowerCase()
  if (t === 'wsl' || t === 'linux') return <LinuxIcon size={10} />
  if (t === 'docker') return <DockerIcon size={10} />
  return <WindowsIcon size={10} />
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

  const handleOpenUrl = (e: React.MouseEvent, url: string) => {
    e.preventDefault()
    openUrl(url)
  }

  return (
    <div className={`service-card status-${status.toLowerCase()}`}>
      <div className="card-header">
        <div className="card-title">
          <span className={`status-dot ${status.toLowerCase()} ${isTransitional ? 'pulse' : ''}`} />
          <span className={`tech-icon-wrap ${isRunning ? 'glow' : ''}`} data-tech={tech.toLowerCase()}>
            <TechIcon tech={tech} size={18} />
          </span>
          <h3 title={name}>{name}</h3>
        </div>
        <span className="target-badge-env">
          {targetIcon(target)}
          <span>{target}</span>
        </span>
      </div>

      <div className="card-body">
        <code className="command" title={command}>{command}</code>

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
              <a
                className="meta-item url"
                href={state.url}
                onClick={(e) => handleOpenUrl(e, state.url!)}
                title={state.url}
              >
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
