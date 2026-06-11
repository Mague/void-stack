import { useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import type { ProjectInfo, ServiceStateDto, DependencyStatusDto, DiagramResult, AnalysisResultDto, SnapshotDto, DebtComparisonDto } from './types'
import Topbar from './components/Topbar'
import Rail, { type Zone } from './components/Rail'
import PulseLine, { type PulseTarget } from './components/PulseLine'
import CommandPalette, { type CommandItem } from './components/CommandPalette'
import ServiceDashboard from './components/ServiceDashboard'
import LogViewer from './components/LogViewer'
import LogDrawer from './components/LogDrawer'
import DepsPanel from './components/DepsPanel'
import DiagramPanel from './components/DiagramPanel'
import AnalysisPanel from './components/AnalysisPanel'
import DocsPanel from './components/DocsPanel'
import SpacePanel from './components/SpacePanel'
import SecurityPanel from './components/SecurityPanel'
import DebtPanel from './components/DebtPanel'
import DockerPanel from './components/DockerPanel'
import StatsPanel from './components/StatsPanel'
import ReviewDiffPanel from './components/ReviewDiffPanel'
import SuggestTestsPanel from './components/SuggestTestsPanel'
import FindDeadCodePanel from './components/FindDeadCodePanel'
import GraphViewerPanel from './components/GraphViewerPanel'
import SearchPanel from './components/SearchPanel'
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

type Panel =
  | 'services' | 'logs' | 'docker'
  | 'search' | 'review' | 'tests' | 'deadcode' | 'analysis' | 'security' | 'debt'
  | 'graph' | 'diagrams' | 'stats'
  | 'deps' | 'docs' | 'space'

const ZONE_PANELS: Record<Zone, Panel[]> = {
  run: ['services', 'logs', 'docker'],
  intel: ['search', 'review', 'tests', 'deadcode', 'analysis', 'security', 'debt'],
  map: ['graph', 'diagrams', 'stats'],
  project: ['deps', 'docs', 'space'],
}

export default function App() {
  const { t } = useTranslation()
  const [projects, setProjects] = useState<ProjectInfo[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [states, setStates] = useState<ServiceStateDto[]>([])
  const [activeZone, setActiveZone] = useState<Zone>('run')
  const [panelByZone, setPanelByZone] = useState<Record<Zone, Panel>>({
    run: 'services', intel: 'search', map: 'graph', project: 'deps',
  })
  const [logService, setLogService] = useState<string | null>(null)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [toast, setToast] = useState<{ msg: string; kind: 'ok' | 'err' } | null>(null)

  // Per-panel cached data — reset on project switch
  const [deps, setDeps] = useState<DependencyStatusDto[]>([])
  const [diagram, setDiagram] = useState<DiagramResult | null>(null)
  const [analysis, setAnalysis] = useState<AnalysisResultDto | null>(null)
  const [readme, setReadme] = useState<string | null>(null)
  const [projectSpaceEntries, setProjectSpaceEntries] = useState<SpaceEntry[]>([])
  const [globalSpaceEntries, setGlobalSpaceEntries] = useState<SpaceEntry[]>([])
  const [auditResult, setAuditResult] = useState<AuditResult | null>(null)
  const [debtSnapshots, setDebtSnapshots] = useState<SnapshotDto[]>([])
  const [debtComparison, setDebtComparison] = useState<DebtComparisonDto | null>(null)

  const showToast = useCallback((msg: string, kind: 'ok' | 'err' = 'ok') => {
    setToast({ msg, kind })
    setTimeout(() => setToast(null), 3500)
  }, [])

  const loadProjects = useCallback(async () => {
    try {
      const list = await invoke<ProjectInfo[]>('list_projects')
      setProjects(list)
      if (list.length > 0 && !selected) setSelected(list[0].name)
    } catch (e) {
      console.error('Error loading projects:', e)
    }
  }, [selected])

  const loadStatus = useCallback(async () => {
    if (!selected) return
    try {
      setStates(await invoke<ServiceStateDto[]>('get_project_status', { project: selected }))
    } catch (e) {
      console.error('Error loading status:', e)
    }
  }, [selected])

  useEffect(() => { loadProjects() }, [loadProjects])

  useEffect(() => {
    const handleRefresh = () => {
      invoke<ProjectInfo[]>('list_projects').then(list => {
        setProjects(list)
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

  // Global ⌘K / Ctrl+K toggles the command palette.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen(o => !o)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const handleSelectProject = (name: string) => {
    if (name === selected) return
    setSelected(name)
    setStates([]); setDeps([]); setDiagram(null); setAnalysis(null); setReadme(null)
    setProjectSpaceEntries([]); setGlobalSpaceEntries([]); setAuditResult(null)
    setDebtSnapshots([]); setDebtComparison(null); setLogService(null)
  }

  const handleStartAll = async () => {
    if (!selected) return
    try { setStates(await invoke<ServiceStateDto[]>('start_all', { project: selected })) }
    catch (e) { console.error('Error starting:', e) }
  }
  const handleStopAll = async () => {
    if (!selected) return
    try { await invoke('stop_all', { project: selected }); setTimeout(loadStatus, 600) }
    catch (e) { console.error('Error stopping:', e) }
  }
  const handleStartService = async (service: string) => {
    if (!selected) return
    try { await invoke('start_service', { project: selected, service }); setTimeout(loadStatus, 600) }
    catch (e) { console.error('Error starting service:', e) }
  }
  const handleStopService = async (service: string) => {
    if (!selected) return
    try { await invoke('stop_service', { project: selected, service }); setTimeout(loadStatus, 600) }
    catch (e) { console.error('Error stopping service:', e) }
  }

  const goTo = (zone: Zone, panel: Panel) => {
    setActiveZone(zone)
    setPanelByZone(prev => ({ ...prev, [zone]: panel }))
  }

  const handleViewLogs = (service: string) => {
    setLogService(service)
    goTo('run', 'logs')
  }

  const buildGraph = useCallback(async () => {
    if (!selected) return
    try {
      await invoke('build_structural_graph_cmd', { project: selected, force: false })
      showToast(t('vitals.graphRebuilt'))
    } catch (e) {
      showToast(String(e), 'err')
    }
  }, [selected, showToast, t])

  const onPulseNavigate = (target: PulseTarget) => {
    const map: Record<PulseTarget, Panel> = { review: 'review', tests: 'tests', deadcode: 'deadcode', security: 'security' }
    goTo('intel', map[target])
  }

  const selectedProject = projects.find(p => p.name === selected) || null
  const serviceNames = selectedProject?.services.map(s => s.name) || []
  const riskScore = auditResult?.summary?.risk_score ?? null

  // Command palette catalog (Code group is left to the semantic-search
  // fallback; here we surface services + actions).
  const commands = useMemo<CommandItem[]>(() => {
    if (!selectedProject) return []
    const items: CommandItem[] = []
    for (const svc of selectedProject.services) {
      const running = states.find(s => s.service_name === svc.name)?.status === 'RUNNING'
      items.push({
        group: t('palette.services'),
        label: `${svc.name} — ${running ? t('services.stop') : t('services.start')}`,
        hint: svc.tech || svc.target,
        glyph: 'service',
        run: () => (running ? handleStopService(svc.name) : handleStartService(svc.name)),
      })
    }
    items.push(
      { group: t('palette.actions'), label: t('palette.reviewDiff'), hint: t('intel.review'), glyph: 'action', run: () => goTo('intel', 'review') },
      { group: t('palette.actions'), label: t('palette.suggestTests'), hint: t('intel.tests'), glyph: 'action', run: () => goTo('intel', 'tests') },
      { group: t('palette.actions'), label: t('palette.findDeadCode'), hint: t('intel.deadcode'), glyph: 'action', run: () => goTo('intel', 'deadcode') },
      { group: t('palette.actions'), label: t('palette.rebuildGraph'), hint: t('vitals.graph'), glyph: 'action', run: buildGraph },
      { group: t('palette.actions'), label: t('palette.runAudit'), hint: t('tabs.security'), glyph: 'action', run: () => goTo('intel', 'security') },
    )
    return items
  }, [selectedProject, states, t, buildGraph]) // eslint-disable-line react-hooks/exhaustive-deps

  const onSearchFallback = (query: string) => {
    if (!selected) return
    invoke<string>('semantic_search_cmd', { projectName: selected, query, topK: 8 })
      .then(json => {
        const results = JSON.parse(json) as unknown[]
        showToast(t('palette.searchHits', { count: Array.isArray(results) ? results.length : 0, query }))
      })
      .catch(e => showToast(String(e), 'err'))
  }

  const activePanel = panelByZone[activeZone]

  const renderPanel = (panel: Panel) => {
    if (!selected) return <div className="vs-empty"><span>{t('services.selectProject')}</span></div>
    switch (panel) {
      case 'services':
        return (
          <>
            <ServiceDashboard
              project={selectedProject}
              states={states}
              onStartAll={handleStartAll}
              onStopAll={handleStopAll}
              onStartService={handleStartService}
              onStopService={handleStopService}
              onViewLogs={handleViewLogs}
            />
            <LogDrawer project={selected} services={serviceNames} activeService={logService} onSelectService={setLogService} />
          </>
        )
      case 'logs':
        return <LogViewer project={selected} services={serviceNames} activeService={logService} onSelectService={setLogService} />
      case 'docker':
        return <DockerPanel project={selected} />
      case 'search':
        return <SearchPanel project={selected} projects={projects.map(p => p.name)} />
      case 'review':
        return <ReviewDiffPanel project={selected} onBuildGraph={buildGraph} />
      case 'tests':
        return <SuggestTestsPanel project={selected} onBuildGraph={buildGraph} />
      case 'deadcode':
        return <FindDeadCodePanel project={selected} onBuildGraph={buildGraph} />
      case 'analysis':
        return <AnalysisPanel project={selected} analysis={analysis} setAnalysis={setAnalysis} />
      case 'security':
        return <SecurityPanel project={selected} audit={auditResult} setAudit={setAuditResult} />
      case 'debt':
        return <DebtPanel project={selected} snapshots={debtSnapshots} setSnapshots={setDebtSnapshots} comparison={debtComparison} setComparison={setDebtComparison} />
      case 'graph':
        return <GraphViewerPanel project={selected} />
      case 'diagrams':
        return <DiagramPanel project={selected} diagram={diagram} setDiagram={setDiagram} />
      case 'stats':
        return <StatsPanel project={selected} />
      case 'deps':
        return <DepsPanel project={selected} deps={deps} setDeps={setDeps} />
      case 'docs':
        return <DocsPanel project={selected} readme={readme} setReadme={setReadme} />
      case 'space':
        return <SpacePanel project={selected} projectEntries={projectSpaceEntries} setProjectEntries={setProjectSpaceEntries} globalEntries={globalSpaceEntries} setGlobalEntries={setGlobalSpaceEntries} />
    }
  }

  return (
    <div className="vs-shell">
      <Topbar
        projects={projects}
        selected={selected}
        onSelect={handleSelectProject}
        onOpenPalette={() => setPaletteOpen(true)}
        onToast={showToast}
      />
      <div className="vs-body">
        <Rail active={activeZone} onSelect={setActiveZone} />
        <main className="vs-main">
          {activeZone === 'run' && (
            <PulseLine project={selected} riskScore={riskScore} onNavigate={onPulseNavigate} />
          )}
          <div className="vs-subnav">
            {ZONE_PANELS[activeZone].map(p => (
              <button
                key={p}
                className={`vs-pill ${activePanel === p ? 'active' : ''}`}
                onClick={() => setPanelByZone(prev => ({ ...prev, [activeZone]: p }))}
              >
                {t(`panels.${p}`)}
              </button>
            ))}
          </div>
          <div className="vs-panel-host">
            {renderPanel(activePanel)}
          </div>
        </main>
      </div>

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={commands}
        onSearchFallback={onSearchFallback}
      />

      {toast && <div className={`vs-toast ${toast.kind === 'err' ? 'err' : ''}`}>{toast.msg}</div>}
    </div>
  )
}
