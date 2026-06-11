import { useState, useEffect } from 'react'
import type { ProjectInfo, ServiceStateDto, DockerServicePreview } from '../types'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Play, Square, Plus, X, Monitor, Terminal, Container, Download, Apple, FileCode } from 'lucide-react'

/** Compact uptime from an ISO start timestamp. */
function fmtUptime(startedAt: string | null): string | null {
  if (!startedAt) return null
  const seconds = Math.floor((Date.now() - new Date(startedAt).getTime()) / 1000)
  if (seconds < 0) return null
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes} min`
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`
}

const playGlyph = <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M7 4v16l13-8z"/></svg>
const stopGlyph = <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>

type TargetType = 'windows' | 'wsl' | 'macos' | 'docker'

function detectPlatformTarget(): TargetType {
  const platform = navigator.platform.toLowerCase()
  if (platform.includes('mac')) return 'macos'
  return 'windows'
}

const defaultTarget = detectPlatformTarget()
const isMac = defaultTarget === 'macos'

interface Props {
  project: ProjectInfo | null
  states: ServiceStateDto[]
  onStartAll: () => void
  onStopAll: () => void
  onStartService: (name: string) => void
  onStopService: (name: string) => void
  onViewLogs: (name: string) => void
}

export default function ServiceDashboard({
  project, states, onStartAll, onStopAll,
  onStartService, onStopService, onViewLogs
}: Props) {
  const { t } = useTranslation()
  const [loadingServices, setLoadingServices] = useState<Set<string>>(new Set())
  const [showAddForm, setShowAddForm] = useState(false)
  const [addName, setAddName] = useState('')
  const [addCommand, setAddCommand] = useState('')
  const [addDir, setAddDir] = useState('')
  const [addTarget, setAddTarget] = useState<TargetType>(defaultTarget)
  const [addPorts, setAddPorts] = useState<string[]>([])
  const [addVolumes, setAddVolumes] = useState<string[]>([])
  const [addError, setAddError] = useState<string | null>(null)

  // Claudeignore state
  const [claudeignoreContent, setClaudeignoreContent] = useState<string | null>(null)
  const [claudeignoreLoading, setClaudeignoreLoading] = useState(false)
  const [claudeignoreToast, setClaudeignoreToast] = useState<string | null>(null)

  // Docker import state
  const [showImportDocker, setShowImportDocker] = useState(false)
  const [dockerPreviews, setDockerPreviews] = useState<DockerServicePreview[]>([])
  const [selectedImports, setSelectedImports] = useState<Set<string>>(new Set())
  const [importLoading, setImportLoading] = useState(false)
  const [importError, setImportError] = useState<string | null>(null)
  const [importSuccess, setImportSuccess] = useState<string | null>(null)

  // Last log line per service for the card tail (prototype). Light poll —
  // a handful of services, one line each.
  const [tails, setTails] = useState<Record<string, string>>({})
  const projectName = project?.name
  const serviceNames = (project?.services ?? []).map(s => s.name).join(',')
  useEffect(() => {
    if (!projectName || !serviceNames) return
    let cancelled = false
    const names = serviceNames.split(',')
    const fetchTails = async () => {
      const entries = await Promise.all(
        names.map(async name => {
          try {
            const lines = await invoke<string[]>('get_logs', { project: projectName, service: name, lines: 1 })
            return [name, lines[lines.length - 1] ?? ''] as const
          } catch {
            return [name, ''] as const
          }
        })
      )
      if (!cancelled) setTails(Object.fromEntries(entries))
    }
    fetchTails()
    const id = setInterval(fetchTails, 3000)
    return () => { cancelled = true; clearInterval(id) }
  }, [projectName, serviceNames])

  if (!project) {
    return (
      <div className="panel empty">
        <p>{t('services.selectProject')}</p>
      </div>
    )
  }

  const runningCount = states.filter(s => s.status === 'RUNNING').length
  const totalCount = project.services.length
  const hasRunning = runningCount > 0

  const handleStart = async (name: string) => {
    setLoadingServices(prev => new Set(prev).add(name))
    try {
      await onStartService(name)
    } finally {
      setTimeout(() => {
        setLoadingServices(prev => {
          const next = new Set(prev)
          next.delete(name)
          return next
        })
      }, 2000)
    }
  }

  const handleStop = async (name: string) => {
    setLoadingServices(prev => new Set(prev).add(name))
    try {
      await onStopService(name)
    } finally {
      setTimeout(() => {
        setLoadingServices(prev => {
          const next = new Set(prev)
          next.delete(name)
          return next
        })
      }, 2000)
    }
  }

  const resetAddForm = () => {
    setAddName('')
    setAddCommand('')
    setAddDir('')
    setAddTarget(defaultTarget)
    setAddPorts([])
    setAddVolumes([])
    setAddError(null)
    setShowAddForm(false)
  }

  const handleAddService = async () => {
    if (!addName || !addCommand) return
    setAddError(null)
    try {
      const workingDir = addDir || project.path
      await invoke('add_service_cmd', {
        project: project.name,
        name: addName,
        command: addCommand,
        workingDir,
        target: addTarget,
        dockerPorts: addTarget === 'docker' && addPorts.length > 0 ? addPorts.filter(Boolean) : null,
        dockerVolumes: addTarget === 'docker' && addVolumes.length > 0 ? addVolumes.filter(Boolean) : null,
        dockerExtraArgs: null,
      })
      resetAddForm()
      window.dispatchEvent(new CustomEvent('void-refresh-projects'))
    } catch (e) {
      setAddError(String(e))
    }
  }

  const handleDetectDocker = async () => {
    setImportLoading(true)
    setImportError(null)
    setImportSuccess(null)
    try {
      const previews = await invoke<DockerServicePreview[]>('detect_docker_services', {
        project: project.name,
      })
      setDockerPreviews(previews)
      setSelectedImports(new Set(previews.map(p => p.name)))
      setShowImportDocker(true)
    } catch (e) {
      setImportError(String(e))
    } finally {
      setImportLoading(false)
    }
  }

  const handleImportSelected = async () => {
    if (selectedImports.size === 0) return
    setImportLoading(true)
    setImportError(null)
    try {
      const count = await invoke<number>('import_docker_services', {
        project: project.name,
        serviceNames: Array.from(selectedImports),
      })
      setImportSuccess(t('services.importedCount', { count }))
      setTimeout(() => window.dispatchEvent(new CustomEvent('void-refresh-projects')), 1500)
    } catch (e) {
      setImportError(String(e))
    } finally {
      setImportLoading(false)
    }
  }

  const toggleImportSelection = (name: string) => {
    setSelectedImports(prev => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })
  }

  const handleRemoveService = async (serviceName: string) => {
    try {
      await invoke('remove_service_cmd', {
        project: project.name,
        service: serviceName,
      })
      window.dispatchEvent(new CustomEvent('void-refresh-projects'))
    } catch (e) {
      console.error('Failed to remove service:', e)
    }
  }

  const handleGenerateClaudeignore = async () => {
    setClaudeignoreLoading(true)
    try {
      const content = await invoke<string>('generate_claudeignore_cmd', {
        projectName: project!.name,
        dryRun: true,
      })
      setClaudeignoreContent(content)
    } catch (e) {
      setClaudeignoreToast(`Error: ${e}`)
      setTimeout(() => setClaudeignoreToast(null), 3000)
    } finally {
      setClaudeignoreLoading(false)
    }
  }

  const handleSaveClaudeignore = async () => {
    try {
      await invoke<string>('generate_claudeignore_cmd', {
        projectName: project!.name,
        dryRun: false,
      })
      setClaudeignoreContent(null)
      setClaudeignoreToast(t('services.claudeignoreSaved'))
      setTimeout(() => setClaudeignoreToast(null), 3000)
    } catch (e) {
      setClaudeignoreToast(`Error: ${e}`)
      setTimeout(() => setClaudeignoreToast(null), 3000)
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">
        {/* Project name + path now live in the topbar picker — kept minimal. */}
        <div className="toolbar">
          <span className="service-counter">
            <span className={`counter-value ${runningCount === totalCount ? 'all-running' : runningCount > 0 ? 'partial' : ''}`}>
              {runningCount}/{totalCount}
            </span>
            <span className="counter-label">{t('services.running')}</span>
          </span>
          <button className="btn btn-success" onClick={onStartAll}>
            <Play size={12} /> {t('services.startAll')}
          </button>
          <button className="btn btn-danger" onClick={onStopAll} disabled={!hasRunning}>
            <Square size={12} /> {t('services.stopAll')}
          </button>
          <button
            className="btn btn-sm"
            onClick={handleDetectDocker}
            disabled={importLoading}
          >
            <Download size={12} />
            {' '}{importLoading ? t('services.detecting') : t('services.importDocker')}
          </button>
          <button
            className="btn btn-sm"
            onClick={handleGenerateClaudeignore}
            disabled={claudeignoreLoading}
          >
            <FileCode size={12} />
            {' '}{claudeignoreLoading ? t('common.loading') : '.claudeignore'}
          </button>
          <button className="btn btn-sm" onClick={() => setShowAddForm(!showAddForm)}>
            {showAddForm ? <X size={12} /> : <Plus size={12} />}
            {' '}{t('services.addService')}
          </button>
        </div>
      </div>

      {claudeignoreToast && (
        <div className="toast-message">{claudeignoreToast}</div>
      )}

      {claudeignoreContent && (
        <div className="add-service-form">
          <div className="import-docker-header">
            <h3><FileCode size={14} /> .claudeignore</h3>
            <button className="btn btn-sm btn-icon" onClick={() => setClaudeignoreContent(null)}>
              <X size={12} />
            </button>
          </div>
          <pre style={{ maxHeight: '300px', overflow: 'auto', fontSize: '0.75rem', padding: '8px', background: 'var(--bg-darker, #111)', borderRadius: '4px' }}>
            {claudeignoreContent}
          </pre>
          <div style={{ display: 'flex', gap: '8px', marginTop: '8px' }}>
            <button className="btn btn-success btn-sm" onClick={handleSaveClaudeignore}>
              {t('services.claudeignoreSave')}
            </button>
            <button className="btn btn-sm" onClick={() => setClaudeignoreContent(null)}>
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {showImportDocker && (
        <div className="add-service-form">
          <div className="import-docker-header">
            <h3><Container size={14} /> {t('services.importDockerDesc')}</h3>
            <button className="btn btn-sm btn-icon" onClick={() => { setShowImportDocker(false); setImportSuccess(null) }}>
              <X size={12} />
            </button>
          </div>

          {dockerPreviews.length === 0 ? (
            <p className="import-docker-empty">{t('services.noDockerFiles')}</p>
          ) : (
            <div className="import-docker-list">
              {dockerPreviews.map(svc => (
                <label key={svc.name} className={`import-docker-item ${selectedImports.has(svc.name) ? 'selected' : ''}`}>
                  <input
                    type="checkbox"
                    checked={selectedImports.has(svc.name)}
                    onChange={() => toggleImportSelection(svc.name)}
                  />
                  <div className="import-docker-info">
                    <strong>{svc.name}</strong>
                    <span className="import-docker-source">{t(`services.from${svc.source === 'compose' ? 'Compose' : 'Dockerfile'}`)}</span>
                    {svc.image && <span className="import-docker-image">{svc.image}</span>}
                    {svc.source === 'compose' && svc.depends_on.length > 0 && (
                      <div className="import-docker-containers">
                        <span className="import-docker-detail">containers: </span>
                        {svc.depends_on.map(name => (
                          <span key={name} className="import-docker-container-badge">{name}</span>
                        ))}
                      </div>
                    )}
                    {svc.ports.length > 0 && (
                      <span className="import-docker-detail">ports: {svc.ports.join(', ')}</span>
                    )}
                    {svc.volumes.length > 0 && (
                      <span className="import-docker-detail">volumes: {svc.volumes.join(', ')}</span>
                    )}
                    {svc.already_exists && (
                      <span className="import-docker-exists">{t('services.willReplace')}</span>
                    )}
                  </div>
                </label>
              ))}
            </div>
          )}

          {importError && <div className="add-service-error">{importError}</div>}
          {importSuccess && <div className="import-docker-success">{importSuccess}</div>}

          {dockerPreviews.length > 0 && !importSuccess && (
            <div className="add-service-actions">
              <button
                className="btn btn-primary btn-sm"
                onClick={handleImportSelected}
                disabled={selectedImports.size === 0 || importLoading}
              >
                <Download size={12} /> {t('services.importSelected')} ({selectedImports.size})
              </button>
              <button className="btn btn-sm" onClick={() => setShowImportDocker(false)}>
                {t('common.cancel')}
              </button>
            </div>
          )}
        </div>
      )}

      {showAddForm && (
        <div className="add-service-form">
          <div className="add-service-row">
            <input
              placeholder={t('services.serviceName')}
              value={addName}
              onChange={e => setAddName(e.target.value)}
            />
            <input
              placeholder={t('services.serviceCommand')}
              value={addCommand}
              onChange={e => setAddCommand(e.target.value)}
              style={{ flex: 2 }}
            />
          </div>
          <div className="add-service-row">
            <input
              placeholder={t('services.serviceDir')}
              value={addDir}
              onChange={e => setAddDir(e.target.value)}
              style={{ flex: 1 }}
            />
            <div className="add-form-target-row">
              {isMac ? (
                <button
                  className={`btn btn-sm btn-toggle ${addTarget === 'macos' ? 'active' : ''}`}
                  onClick={() => setAddTarget('macos')}
                >
                  <Apple size={12} /> macOS
                </button>
              ) : (
                <>
                  <button
                    className={`btn btn-sm btn-toggle ${addTarget === 'windows' ? 'active' : ''}`}
                    onClick={() => setAddTarget('windows')}
                  >
                    <Monitor size={12} /> Win
                  </button>
                  <button
                    className={`btn btn-sm btn-toggle ${addTarget === 'wsl' ? 'active' : ''}`}
                    onClick={() => setAddTarget('wsl')}
                  >
                    <Terminal size={12} /> WSL
                  </button>
                </>
              )}
              <button
                className={`btn btn-sm btn-toggle ${addTarget === 'docker' ? 'active' : ''}`}
                onClick={() => setAddTarget('docker')}
              >
                <Container size={12} /> Docker
              </button>
            </div>
          </div>

          {addTarget === 'docker' && (
            <div className="add-service-docker">
              <div className="docker-field-group">
                <label>{t('services.ports')}</label>
                {addPorts.map((p, i) => (
                  <div key={i} className="docker-field-row">
                    <input
                      value={p}
                      placeholder="8080:80"
                      onChange={e => {
                        const next = [...addPorts]
                        next[i] = e.target.value
                        setAddPorts(next)
                      }}
                    />
                    <button className="btn btn-sm btn-icon" onClick={() => setAddPorts(addPorts.filter((_, j) => j !== i))}>
                      <X size={10} />
                    </button>
                  </div>
                ))}
                <button className="btn btn-sm" onClick={() => setAddPorts([...addPorts, ''])}>
                  {t('services.addPort')}
                </button>
              </div>
              <div className="docker-field-group">
                <label>{t('services.volumes')}</label>
                {addVolumes.map((v, i) => (
                  <div key={i} className="docker-field-row">
                    <input
                      value={v}
                      placeholder="./data:/app/data"
                      onChange={e => {
                        const next = [...addVolumes]
                        next[i] = e.target.value
                        setAddVolumes(next)
                      }}
                    />
                    <button className="btn btn-sm btn-icon" onClick={() => setAddVolumes(addVolumes.filter((_, j) => j !== i))}>
                      <X size={10} />
                    </button>
                  </div>
                ))}
                <button className="btn btn-sm" onClick={() => setAddVolumes([...addVolumes, ''])}>
                  {t('services.addVolume')}
                </button>
              </div>
            </div>
          )}

          {addError && <div className="add-service-error">{addError}</div>}

          <div className="add-service-actions">
            <button className="btn btn-primary btn-sm" onClick={handleAddService} disabled={!addName || !addCommand}>
              <Plus size={12} /> {t('sidebar.add')}
            </button>
            <button className="btn btn-sm" onClick={resetAddForm}>{t('common.cancel')}</button>
          </div>
        </div>
      )}

      <div className="vs-grid">
        {project.services.map(svc => {
          const st = states.find(s => s.service_name === svc.name)
          const status = st?.status ?? 'STOPPED'
          const isRunning = status === 'RUNNING'
          const isFailed = status === 'FAILED'
          const dot = isRunning ? 'run' : isFailed ? 'err' : status === 'STARTING' || status === 'STOPPING' ? 'starting' : 'stop'
          const uptime = fmtUptime(st?.started_at ?? null)
          const busy = loadingServices.has(svc.name)
          const tail = tails[svc.name]
          return (
            <div key={svc.name} className={`vs-card ${isFailed ? 'down' : ''}`}>
              <div className="vs-card-head">
                <span className={`vs-dot ${dot}`} title={status} />
                <h3>{svc.name} <span className="vs-lang">· {svc.tech || svc.target}</span></h3>
                <button
                  className="vs-card-act"
                  disabled={busy}
                  onClick={() => (isRunning ? handleStop(svc.name) : handleStart(svc.name))}
                  aria-label={`${isRunning ? t('services.stop') : t('services.start')} ${svc.name}`}
                  title={isRunning ? t('services.stop') : t('services.start')}
                >
                  {isRunning ? stopGlyph : playGlyph}
                </button>
                {!isRunning && (
                  <button
                    className="vs-card-act"
                    onClick={() => { if (confirm(t('services.confirmRemoveService', { name: svc.name }))) handleRemoveService(svc.name) }}
                    aria-label={`${t('common.remove')} ${svc.name}`}
                    title={t('common.remove')}
                  >
                    <X size={13} />
                  </button>
                )}
              </div>
              <div className="vs-card-meta">
                {isFailed ? (
                  <span className="bad">{t('services.exited')}{uptime ? ` · ${uptime}` : ''}</span>
                ) : (
                  <>
                    {st?.url && (
                      <a href={st.url} onClick={e => { e.preventDefault(); openUrl(st.url!) }} title={st.url}>
                        {st.url.replace(/^https?:\/\//, '')}
                      </a>
                    )}
                    {st?.url && uptime ? ' · ' : ''}
                    {uptime ? <span>{uptime}</span> : (!st?.url && <span>{isRunning ? '—' : t('services.stopped')}</span>)}
                  </>
                )}
                <button className="vs-card-logs-link" onClick={() => onViewLogs(svc.name)} aria-label={`${t('tabs.logs')} ${svc.name}`} style={{ marginLeft: 8, color: 'var(--vs-text-3)', fontSize: '11px' }}>
                  {t('tabs.logs')}
                </button>
              </div>
              {tail && <div className={`vs-card-tail ${isFailed ? 'bad' : ''}`}>{tail}</div>}
            </div>
          )
        })}
        <button className="vs-card ghost" onClick={() => setShowAddForm(true)}>
          <Plus size={15} /> {t('services.addService')}
        </button>
      </div>
    </div>
  )
}
