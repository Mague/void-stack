import type { ProjectInfo, ServiceStateDto } from '../types'
import { FolderOpen, Plus, Trash2, Globe, Monitor, Terminal } from 'lucide-react'
import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { LANGUAGES } from '../i18n'
import WslBrowser from './WslBrowser'

interface Props {
  projects: ProjectInfo[]
  selected: string | null
  onSelect: (name: string) => void
  states: ServiceStateDto[]
}

export default function Sidebar({ projects, selected, onSelect, states }: Props) {
  const { t, i18n } = useTranslation()
  const [showAdd, setShowAdd] = useState(false)
  const [newName, setNewName] = useState('')
  const [newPath, setNewPath] = useState('')
  const [wsl, setWsl] = useState(false)
  const [wslDistros, setWslDistros] = useState<string[]>([])
  const [wslDistro, setWslDistro] = useState('')
  const [loadingDistros, setLoadingDistros] = useState(false)
  const [showWslBrowser, setShowWslBrowser] = useState(false)

  const runningCount = states.filter(s => s.status === 'RUNNING').length

  // Load WSL distros when WSL mode is toggled on
  useEffect(() => {
    if (wsl && wslDistros.length === 0) {
      setLoadingDistros(true)
      invoke<string[]>('list_wsl_distros')
        .then(distros => {
          setWslDistros(distros)
          if (distros.length > 0 && !wslDistro) {
            setWslDistro(distros[0])
          }
        })
        .catch(() => setWslDistros([]))
        .finally(() => setLoadingDistros(false))
    }
  }, [wsl])

  const pickFolder = async () => {
    if (wsl) {
      // Use custom WSL browser instead of native dialog
      setShowWslBrowser(true)
      return
    }
    const picked = await open({ directory: true, multiple: false })
    if (typeof picked === 'string') {
      setNewPath(picked)
      if (!newName) {
        const folderName = picked.replace(/\\/g, '/').split('/').filter(Boolean).pop() || ''
        setNewName(folderName)
      }
    }
  }

  const handleWslSelect = (uncPath: string) => {
    setNewPath(uncPath)
    setShowWslBrowser(false)
    if (!newName) {
      const folderName = uncPath.split(/[/\\]/).filter(Boolean).pop() || ''
      setNewName(folderName)
    }
  }

  const handleAdd = async () => {
    if (!newName || !newPath) return
    try {
      await invoke('add_project', { name: newName, path: newPath, wsl: wsl || undefined })
      setNewName('')
      setNewPath('')
      setWsl(false)
      setWslDistro('')
      setShowAdd(false)
      window.location.reload()
    } catch (e) {
      alert(e)
    }
  }

  const handleRemove = async (e: React.MouseEvent, name: string) => {
    e.stopPropagation()
    if (!confirm(t('sidebar.confirmRemove', { name }))) return
    try {
      await invoke('remove_project_cmd', { name })
      window.location.reload()
    } catch (err) {
      alert(err)
    }
  }

  const cycleLang = () => {
    const currentIdx = LANGUAGES.findIndex(l => l.code === i18n.language)
    const nextIdx = (currentIdx + 1) % LANGUAGES.length
    i18n.changeLanguage(LANGUAGES[nextIdx].code)
  }

  const currentLang = LANGUAGES.find(l => l.code === i18n.language)

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <h1 className="logo">
          <span className="logo-dot" />
          Void Stack
        </h1>
        <button
          className="lang-toggle"
          onClick={cycleLang}
          title={t('common.language')}
        >
          <Globe size={12} />
          <span>{currentLang?.code.toUpperCase()}</span>
        </button>
      </div>

      <div className="project-list">
        {projects.map(p => (
          <button
            key={p.name}
            className={`project-item ${selected === p.name ? 'active' : ''}`}
            onClick={() => onSelect(p.name)}
          >
            <FolderOpen size={14} className="project-icon" />
            <span className="project-name">{p.name}</span>
            {selected === p.name && runningCount > 0 && (
              <span className="running-badge">{runningCount}</span>
            )}
            <Trash2
              size={12}
              style={{ opacity: 0.3, cursor: 'pointer', flexShrink: 0 }}
              onClick={(e) => handleRemove(e, p.name)}
            />
          </button>
        ))}
      </div>

      <div className="sidebar-footer">
        {showAdd ? (
          <div className="add-form">
            <input
              placeholder={t('sidebar.namePlaceholder')}
              value={newName}
              onChange={e => setNewName(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleAdd()}
            />
            <div className="add-form-path-row">
              <input
                placeholder={t('sidebar.pathPlaceholder')}
                value={newPath}
                onChange={e => setNewPath(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleAdd()}
                style={{ flex: 1 }}
              />
              <button
                className="btn btn-sm btn-icon"
                onClick={pickFolder}
                title={t('sidebar.browse')}
              >
                <FolderOpen size={14} />
              </button>
            </div>
            <div className="add-form-target-row">
              <button
                className={`btn btn-sm btn-toggle ${!wsl ? 'active' : ''}`}
                onClick={() => setWsl(false)}
                title="Windows"
              >
                <Monitor size={12} /> Win
              </button>
              <button
                className={`btn btn-sm btn-toggle ${wsl ? 'active' : ''}`}
                onClick={() => setWsl(true)}
                title="WSL"
              >
                <Terminal size={12} /> WSL
              </button>
            </div>
            {wsl && (
              <div className="add-form-wsl-distro">
                {loadingDistros ? (
                  <span className="loading-spinner" />
                ) : wslDistros.length > 0 ? (
                  <select
                    className="wsl-distro-select"
                    value={wslDistro}
                    onChange={e => setWslDistro(e.target.value)}
                  >
                    {wslDistros.map(d => (
                      <option key={d} value={d}>{d}</option>
                    ))}
                  </select>
                ) : (
                  <span className="wsl-no-distros">{t('sidebar.noWsl')}</span>
                )}
              </div>
            )}
            <div className="add-form-buttons">
              <button className="btn btn-primary btn-sm" onClick={handleAdd}>{t('sidebar.add')}</button>
              <button className="btn btn-sm" onClick={() => { setShowAdd(false); setWsl(false); setWslDistro('') }}>{t('common.cancel')}</button>
            </div>
          </div>
        ) : (
          <button className="btn btn-add" onClick={() => setShowAdd(true)}>
            <Plus size={14} /> {t('sidebar.addProject')}
          </button>
        )}
      </div>

      {showWslBrowser && wslDistro && (
        <WslBrowser
          distro={wslDistro}
          onSelect={handleWslSelect}
          onClose={() => setShowWslBrowser(false)}
        />
      )}
    </aside>
  )
}
