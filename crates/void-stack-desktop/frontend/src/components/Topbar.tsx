import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { ask } from '@tauri-apps/plugin-dialog'
import { Folder, ChevronDown, Search, Plus, Globe, RotateCcw } from 'lucide-react'
import type { ProjectInfo } from '../types'
import ProjectPicker from './ProjectPicker'
import VoidLogo from './VoidLogo'
import { LANGUAGES } from '../i18n'
import { useProjectSignal } from '../projectSignals'

interface Vitals { index_created_at: string | null; graph_built_at: string | null }

interface Props {
  projects: ProjectInfo[]
  selected: string | null
  onSelect: (name: string) => void
  onOpenPalette: () => void
  onToast: (message: string, kind?: 'ok' | 'err') => void
}

const FRESH_MS = 10 * 60 * 1000

function ageLabel(iso: string): string {
  const min = Math.floor((Date.now() - new Date(iso).getTime()) / 60000)
  if (min < 1) return 'now'
  if (min < 60) return `${min} min`
  const h = Math.floor(min / 60)
  if (h < 24) return `${h} h`
  return `${Math.floor(h / 24)} d`
}

export default function Topbar({ projects, selected, onSelect, onOpenPalette, onToast }: Props) {
  const { t, i18n } = useTranslation()
  const [pickerOpen, setPickerOpen] = useState(false)
  const [pickerMode, setPickerMode] = useState<'normal' | 'add'>('normal')
  const [vitals, setVitals] = useState<Vitals | null>(null)
  const [rebuilding, setRebuilding] = useState(false)
  const [indexing, setIndexing] = useState(false)

  const cycleLang = () => {
    const idx = LANGUAGES.findIndex(l => l.code === i18n.language)
    i18n.changeLanguage(LANGUAGES[(idx + 1) % LANGUAGES.length].code)
  }
  const langCode = (i18n.language || 'es').slice(0, 2).toUpperCase()

  const loadVitals = useCallback(async () => {
    if (!selected) { setVitals(null); return }
    try {
      setVitals(await invoke<Vitals>('get_project_vitals_cmd', { project: selected }))
    } catch {
      setVitals({ index_created_at: null, graph_built_at: null })
    }
  }, [selected])

  // Re-read when the index or graph is rebuilt outside this window (watcher,
  // git hook, MCP) so the freshness chips don't lie.
  const indexSignal = useProjectSignal('index')
  const graphSignal = useProjectSignal('graph')
  useEffect(() => { loadVitals() }, [loadVitals, indexSignal, graphSignal])

  const rebuildGraph = async () => {
    if (!selected || rebuilding) return
    setRebuilding(true)
    try {
      await invoke('build_structural_graph_cmd', { project: selected, force: true })
      await loadVitals()
      onToast(t('vitals.graphRebuilt'))
    } catch (e) {
      onToast(String(e), 'err')
    } finally {
      setRebuilding(false)
    }
  }

  // Incremental by default: only files whose SHA-256 changed re-embed.
  // `full` deletes the whole index first and rebuilds it from scratch.
  const reindex = async (full = false) => {
    if (!selected || indexing) return
    if (full && !(await ask(t('vitals.reindexFullConfirm', { name: selected }), { kind: 'warning' }))) return
    setIndexing(true)
    try {
      await invoke('index_project_codebase_cmd', { projectName: selected, force: full })
      await loadVitals()
      onToast(t(full ? 'vitals.indexRebuiltFull' : 'vitals.indexRebuilt'))
    } catch (e) {
      onToast(String(e), 'err')
    } finally {
      setIndexing(false)
    }
  }

  const indexFresh = vitals?.index_created_at && Date.now() - new Date(vitals.index_created_at).getTime() < FRESH_MS
  const graphFresh = vitals?.graph_built_at && Date.now() - new Date(vitals.graph_built_at).getTime() < FRESH_MS

  return (
    <div className="vs-topbar">
      <div className="vs-brand"><VoidLogo size={20} /><b>void stack</b></div>

      <div className="vs-picker-wrap">
        <button className="vs-project" onClick={() => { setPickerMode('normal'); setPickerOpen(o => !o) }} aria-haspopup="menu" aria-expanded={pickerOpen}>
          <Folder size={14} />
          {selected ?? t('topbar.noProject')}
          <ChevronDown size={12} />
        </button>
        <button
          className="vs-project vs-project-add"
          onClick={() => { setPickerMode('add'); setPickerOpen(true) }}
          aria-label={t('sidebar.addProject')}
          title={t('sidebar.addProject')}
        >
          <Plus size={14} />
        </button>
        {pickerOpen && (
          <ProjectPicker
            projects={projects}
            selected={selected}
            onSelect={onSelect}
            onClose={() => setPickerOpen(false)}
            onToast={onToast}
            initialMode={pickerMode}
          />
        )}
      </div>

      <button className="vs-search-trigger" onClick={onOpenPalette} aria-label={t('palette.label')}>
        <Search size={14} />
        {t('palette.triggerHint')}
        <span className="vs-kbd">⌘K</span>
      </button>

      <div className="vs-health">
        {selected && (
          <>
            <button
              className={`vs-vital ${indexing ? 'busy' : vitals?.index_created_at ? (indexFresh ? 'fresh' : 'stale') : 'absent stale'}`}
              onClick={() => reindex()}
              disabled={indexing}
              title={vitals?.index_created_at ? t('vitals.indexBuilt', { age: ageLabel(vitals.index_created_at) }) : t('vitals.indexAbsent')}
            >
              <i />
              {indexing
                ? t('vitals.indexing')
                : `${t('vitals.index')}${vitals?.index_created_at && !indexFresh ? ` · ${ageLabel(vitals.index_created_at)}` : !vitals?.index_created_at ? ` · ${t('vitals.none')}` : ''}`}
            </button>
            {vitals?.index_created_at && !indexing && (
              <button
                className="vs-vital vs-vital-icon"
                onClick={() => reindex(true)}
                title={t('vitals.reindexFull')}
                aria-label={t('vitals.reindexFull')}
              >
                <RotateCcw size={11} />
              </button>
            )}
            <button
              className={`vs-vital ${rebuilding ? 'busy' : !vitals?.graph_built_at ? 'absent stale' : graphFresh ? 'fresh' : 'stale'}`}
              onClick={rebuildGraph}
              disabled={rebuilding}
              title={vitals?.graph_built_at ? t('vitals.graphBuilt', { age: ageLabel(vitals.graph_built_at) }) : t('vitals.graphAbsent')}
            >
              <i />
              {rebuilding
                ? t('vitals.rebuilding')
                : `${t('vitals.graph')}${vitals?.graph_built_at && !graphFresh ? ` · ${ageLabel(vitals.graph_built_at)}` : !vitals?.graph_built_at ? ` · ${t('vitals.none')}` : ''}`}
            </button>
          </>
        )}
        <button className="vs-vital vs-lang-btn" onClick={cycleLang} title={t('topbar.switchLang')} aria-label={t('topbar.switchLang')}>
          <Globe size={12} /> {langCode}
        </button>
      </div>
    </div>
  )
}
