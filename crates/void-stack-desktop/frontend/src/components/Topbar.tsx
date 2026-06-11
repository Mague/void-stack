import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Folder, ChevronDown, Search, Plus, Globe } from 'lucide-react'
import type { ProjectInfo } from '../types'
import ProjectPicker from './ProjectPicker'
import VoidLogo from './VoidLogo'
import { LANGUAGES } from '../i18n'

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

  useEffect(() => { loadVitals() }, [loadVitals])

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
            <span
              className={`vs-vital ${vitals?.index_created_at ? (indexFresh ? 'fresh' : 'stale') : 'absent'}`}
              title={vitals?.index_created_at ? t('vitals.indexBuilt', { age: ageLabel(vitals.index_created_at) }) : t('vitals.indexAbsent')}
            >
              <i />{t('vitals.index')}{vitals?.index_created_at && !indexFresh ? ` · ${ageLabel(vitals.index_created_at)}` : ''}
            </span>
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
