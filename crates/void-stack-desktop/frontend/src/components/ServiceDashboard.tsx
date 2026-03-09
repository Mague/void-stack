import { useState } from 'react'
import type { ProjectInfo, ServiceStateDto } from '../types'
import { useTranslation } from 'react-i18next'
import ServiceCard from './ServiceCard'
import { Play, Square } from 'lucide-react'

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
      // Remove after a delay to let status poll catch up
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
        </div>
      </div>

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
