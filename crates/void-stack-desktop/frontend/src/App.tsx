import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import type { ProjectInfo, ServiceStateDto, DependencyStatusDto, DiagramResult, AnalysisResultDto, SnapshotDto, DebtComparisonDto } from './types'
import Sidebar from './components/Sidebar'
import ServiceDashboard from './components/ServiceDashboard'
import LogViewer from './components/LogViewer'
import DepsPanel from './components/DepsPanel'
import DiagramPanel from './components/DiagramPanel'
import AnalysisPanel from './components/AnalysisPanel'
import DocsPanel from './components/DocsPanel'
import SpacePanel from './components/SpacePanel'
import SecurityPanel from './components/SecurityPanel'
import DebtPanel from './components/DebtPanel'
import DockerPanel from './components/DockerPanel'
import StatsPanel from './components/StatsPanel'
import type { AuditResult } from './components/SecurityPanel'

interface SpaceEntry {
  name: string
  category: string
  path: string
  size_bytes: number
  size_human: string
  deletable: boolean
  restore_hint: string
}

type Tab = 'services' | 'logs' | 'deps' | 'diagrams' | 'analysis' | 'docs' | 'space' | 'security' | 'debt' | 'docker' | 'stats'

export default function App() {
  const { t } = useTranslation()
  const [projects, setProjects] = useState<ProjectInfo[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [states, setStates] = useState<ServiceStateDto[]>([])
  const [activeTab, setActiveTab] = useState<Tab>('services')
  const [logService, setLogService] = useState<string | null>(null)

  // Per-tab cached data — reset on project switch
  const [deps, setDeps] = useState<DependencyStatusDto[]>([])
  const [diagram, setDiagram] = useState<DiagramResult | null>(null)
  const [analysis, setAnalysis] = useState<AnalysisResultDto | null>(null)
  const [readme, setReadme] = useState<string | null>(null)
  const [projectSpaceEntries, setProjectSpaceEntries] = useState<SpaceEntry[]>([])
  const [globalSpaceEntries, setGlobalSpaceEntries] = useState<SpaceEntry[]>([])
  const [auditResult, setAuditResult] = useState<AuditResult | null>(null)
  const [debtSnapshots, setDebtSnapshots] = useState<SnapshotDto[]>([])
  const [debtComparison, setDebtComparison] = useState<DebtComparisonDto | null>(null)

  const loadProjects = useCallback(async () => {
    try {
      const list = await invoke<ProjectInfo[]>('list_projects')
      setProjects(list)
      if (list.length > 0 && !selected) {
        setSelected(list[0].name)
      }
    } catch (e) {
      console.error('Error loading projects:', e)
    }
  }, [selected])

  const loadStatus = useCallback(async () => {
    if (!selected) return
    try {
      const s = await invoke<ServiceStateDto[]>('get_project_status', { project: selected })
      setStates(s)
    } catch (e) {
      console.error('Error loading status:', e)
    }
  }, [selected])

  useEffect(() => {
    loadProjects()
  }, [loadProjects])

  // Listen for refresh events (e.g., after removing a service) without losing selection
  useEffect(() => {
    const handleRefresh = () => {
      invoke<ProjectInfo[]>('list_projects').then(list => {
        setProjects(list)
        // If current project was deleted, select the first available
        if (selected && !list.find(p => p.name === selected) && list.length > 0) {
          setSelected(list[0].name)
        }
      })
    }
    window.addEventListener('void-refresh-projects', handleRefresh)
    return () => window.removeEventListener('void-refresh-projects', handleRefresh)
  }, [selected])

  useEffect(() => {
    loadStatus()
    const interval = setInterval(loadStatus, 2000)
    return () => clearInterval(interval)
  }, [loadStatus])

  // Reset ALL tab data when switching projects
  const handleSelectProject = (name: string) => {
    if (name === selected) return
    setSelected(name)
    setStates([])
    setDeps([])
    setDiagram(null)
    setAnalysis(null)
    setReadme(null)
    setProjectSpaceEntries([])
    setGlobalSpaceEntries([])
    setAuditResult(null)
    setDebtSnapshots([])
    setDebtComparison(null)
    setLogService(null)
  }

  const handleStartAll = async () => {
    if (!selected) return
    try {
      const s = await invoke<ServiceStateDto[]>('start_all', { project: selected })
      setStates(s)
    } catch (e) {
      console.error('Error starting:', e)
    }
  }

  const handleStopAll = async () => {
    if (!selected) return
    try {
      await invoke('stop_all', { project: selected })
      setTimeout(loadStatus, 600)
    } catch (e) {
      console.error('Error stopping:', e)
    }
  }

  const handleStartService = async (service: string) => {
    if (!selected) return
    try {
      await invoke('start_service', { project: selected, service })
      setTimeout(loadStatus, 600)
    } catch (e) {
      console.error('Error starting service:', e)
    }
  }

  const handleStopService = async (service: string) => {
    if (!selected) return
    try {
      await invoke('stop_service', { project: selected, service })
      setTimeout(loadStatus, 600)
    } catch (e) {
      console.error('Error stopping service:', e)
    }
  }

  const handleViewLogs = (service: string) => {
    setLogService(service)
    setActiveTab('logs')
  }

  const selectedProject = projects.find(p => p.name === selected) || null

  const tabKeys: Tab[] = ['services', 'logs', 'deps', 'diagrams', 'analysis', 'docs', 'space', 'security', 'debt', 'docker', 'stats']

  return (
    <div className="app">
      <Sidebar
        projects={projects}
        selected={selected}
        onSelect={handleSelectProject}
        states={states}
      />
      <main className="main-content">
        <div className="tabs">
          {tabKeys.map(tab => (
            <button
              key={tab}
              className={activeTab === tab ? 'tab active' : 'tab'}
              onClick={() => setActiveTab(tab)}
            >
              {t(`tabs.${tab}`)}
            </button>
          ))}
        </div>

        {activeTab === 'services' && (
          <ServiceDashboard
            project={selectedProject}
            states={states}
            onStartAll={handleStartAll}
            onStopAll={handleStopAll}
            onStartService={handleStartService}
            onStopService={handleStopService}
            onViewLogs={handleViewLogs}
          />
        )}

        {activeTab === 'logs' && selected && (
          <LogViewer
            project={selected}
            services={selectedProject?.services.map(s => s.name) || []}
            activeService={logService}
            onSelectService={setLogService}
          />
        )}

        {activeTab === 'deps' && selected && (
          <DepsPanel project={selected} deps={deps} setDeps={setDeps} />
        )}

        {activeTab === 'diagrams' && selected && (
          <DiagramPanel project={selected} diagram={diagram} setDiagram={setDiagram} />
        )}

        {activeTab === 'analysis' && selected && (
          <AnalysisPanel project={selected} analysis={analysis} setAnalysis={setAnalysis} />
        )}

        {activeTab === 'docs' && selected && (
          <DocsPanel project={selected} readme={readme} setReadme={setReadme} />
        )}

        {activeTab === 'security' && selected && (
          <SecurityPanel project={selected} audit={auditResult} setAudit={setAuditResult} />
        )}

        {activeTab === 'debt' && selected && (
          <DebtPanel
            project={selected}
            snapshots={debtSnapshots}
            setSnapshots={setDebtSnapshots}
            comparison={debtComparison}
            setComparison={setDebtComparison}
          />
        )}

        {activeTab === 'docker' && selected && (
          <DockerPanel project={selected} />
        )}

        {activeTab === 'stats' && (
          <StatsPanel project={selected} />
        )}

        {activeTab === 'space' && selected && (
          <SpacePanel
            project={selected}
            projectEntries={projectSpaceEntries}
            setProjectEntries={setProjectSpaceEntries}
            globalEntries={globalSpaceEntries}
            setGlobalEntries={setGlobalSpaceEntries}
          />
        )}
      </main>
    </div>
  )
}
