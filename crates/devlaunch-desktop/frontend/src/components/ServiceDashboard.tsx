import type { ProjectInfo, ServiceStateDto } from '../types'
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
  if (!project) {
    return (
      <div className="panel empty">
        <p>[ selecciona un proyecto ]</p>
      </div>
    )
  }

  const hasRunning = states.some(s => s.status === 'RUNNING')

  return (
    <div className="panel">
      <div className="panel-header">
        <div>
          <h2>{project.name}</h2>
          <span className="project-path">{project.path}</span>
        </div>
        <div className="toolbar">
          <button className="btn btn-success" onClick={onStartAll}>
            <Play size={12} /> Iniciar todo
          </button>
          <button className="btn btn-danger" onClick={onStopAll} disabled={!hasRunning}>
            <Square size={12} /> Detener todo
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
              onStart={() => onStartService(svc.name)}
              onStop={() => onStopService(svc.name)}
              onViewLogs={() => onViewLogs(svc.name)}
            />
          )
        })}
      </div>
    </div>
  )
}
