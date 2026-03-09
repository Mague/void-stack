import { useState } from 'react'
import type { ProjectInfo, ServiceStateDto } from '../types'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import ServiceCard from './ServiceCard'
import { Play, Square, Plus, X, Monitor, Terminal, Container } from 'lucide-react'

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
  const [addTarget, setAddTarget] = useState<'windows' | 'wsl' | 'docker'>('windows')
  const [addPorts, setAddPorts] = useState<string[]>([])
  const [addVolumes, setAddVolumes] = useState<string[]>([])
  const [addError, setAddError] = useState<string | null>(null)

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
    setAddTarget('windows')
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
      window.location.reload()
    } catch (e) {
      setAddError(String(e))
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <div>
          <h2>{project.name}</h2>
          <span className="project-path">{project.path}</span>
        </div>
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
          <button className="btn btn-sm" onClick={() => setShowAddForm(!showAddForm)}>
            {showAddForm ? <X size={12} /> : <Plus size={12} />}
            {' '}{t('services.addService')}
          </button>
        </div>
      </div>

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

      <div className="service-grid">
        {project.services.map(svc => {
          const svcState = states.find(s => s.service_name === svc.name)
          return (
            <ServiceCard
              key={svc.name}
              name={svc.name}
              command={svc.command}
              target={svc.target}
              state={svcState}
              loading={loadingServices.has(svc.name)}
              projectName={project.name}
              dockerPorts={svc.docker_ports}
              dockerVolumes={svc.docker_volumes}
              onStart={() => handleStart(svc.name)}
              onStop={() => handleStop(svc.name)}
              onViewLogs={() => onViewLogs(svc.name)}
            />
          )
        })}
      </div>
    </div>
  )
}
