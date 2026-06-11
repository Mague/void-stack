import type { ProjectInfo, ServiceStateDto } from '../types'
import { FolderOpen, Plus, Trash2, Pencil, Globe, Monitor, Terminal, Search, ArrowDownAZ, ArrowUpAZ } from 'lucide-react'
import { confirm as tauriConfirm } from '@tauri-apps/plugin-dialog'

const isMac = navigator.platform.toLowerCase().includes('mac')
import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { LANGUAGES } from '../i18n'
import WslBrowser from './WslBrowser'
import VoidLogo from './VoidLogo'

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
  const [search, setSearch] = useState('')
  const [sortAsc, setSortAsc] = useState(true)
  const [editing, setEditing] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const [editPath, setEditPath] = useState('')
  const [editBusy, setEditBusy] = useState(false)

  const runningCount = states.filter(s => s.status === 'RUNNING').length

  const filteredProjects = projects
    .filter(p => p.name.toLowerCase().includes(search.toLowerCase()))
    .sort((a, b) => sortAsc
      ? a.name.localeCompare(b.name)
      : b.name.localeCompare(a.name)
    )

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

  const startEdit = (e: React.MouseEvent, p: ProjectInfo) => {
    e.stopPropagation()
    setEditing(p.name)
    setEditName(p.name)
    setEditPath(p.path)
  }

  const pickEditFolder = async () => {
    const picked = await open({ directory: true, multiple: false })
    if (typeof picked === 'string') setEditPath(picked)
  }

  const handleEditSave = async () => {
    if (!editing || editBusy) return
    const original = projects.find(p => p.name === editing)
    const newName = editName.trim() && editName.trim() !== editing ? editName.trim() : undefined
    const newPath = editPath.trim() && editPath.trim() !== original?.path ? editPath.trim() : undefined
    if (!newName && !newPath) {
      setEditing(null)
      return
    }
    setEditBusy(true)
    try {
      const [updated] = await invoke<[ProjectInfo, string[]]>('update_project_cmd', {
        name: editing,
        newName,
        newPath,
      })
      setEditing(null)
      // Refresh the list in place (no full reload) and follow the rename.
      window.dispatchEvent(new Event('void-refresh-projects'))
      if (updated.name !== editing) onSelect(updated.name)
    } catch (err) {
      alert(String(err))
    } finally {
      setEditBusy(false)
    }
  }

  const handleRemove = async (e: React.MouseEvent, name: string) => {
    e.stopPropagation()
    const confirmed = await tauriConfirm(
      t('sidebar.confirmRemove', { name }),
      { title: 'Void Stack', kind: 'warning' }
    )
    if (!confirmed) return
    try {
      await invoke('remove_project_cmd', { name })
      window.location.reload()
    } catch (err) {
      alert(String(err))
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
          <VoidLogo size={24} />
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

      <div className="project-search">
        <Search size={12} className="search-icon" />
        <input
          type="text"
          placeholder={t('sidebar.search') || 'Search...'}
          value={search}
          onChange={e => setSearch(e.target.value)}
          className="search-input"
        />
        <button
          className="sort-toggle"
          onClick={() => setSortAsc(!sortAsc)}
          title={sortAsc ? 'Z → A' : 'A → Z'}
        >
          {sortAsc ? <ArrowDownAZ size={14} /> : <ArrowUpAZ size={14} />}
        </button>
      </div>

      <div className="project-list">
        {filteredProjects.map(p => (
          editing === p.name ? (
            <div key={p.name} className="add-form" style={{ margin: '4px 8px' }}>
              <input
                placeholder={t('sidebar.namePlaceholder')}
                value={editName}
                onChange={e => setEditName(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleEditSave()}
                autoFocus
              />
              <div className="add-form-path-row">
                <input
                  placeholder={t('sidebar.pathPlaceholder')}
                  value={editPath}
                  onChange={e => setEditPath(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && handleEditSave()}
                  style={{ flex: 1 }}
                />
                <button
                  className="btn btn-sm btn-icon"
                  onClick={pickEditFolder}
                  title={t('sidebar.browse')}
                >
                  <FolderOpen size={14} />
                </button>
              </div>
              <span style={{ fontSize: 10, opacity: 0.6 }}>{t('sidebar.editHint')}</span>
              <div className="add-form-buttons">
                <button className="btn btn-primary btn-sm" onClick={handleEditSave} disabled={editBusy}>
                  {editBusy ? '…' : t('sidebar.editSave')}
                </button>
                <button className="btn btn-sm" onClick={() => setEditing(null)}>{t('common.cancel')}</button>
              </div>
            </div>
          ) : (
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
            <Pencil
              size={12}
              style={{ opacity: 0.3, cursor: 'pointer', flexShrink: 0, marginRight: 4 }}
              onClick={(e) => startEdit(e, p)}
            />
            <Trash2
              size={12}
              style={{ opacity: 0.3, cursor: 'pointer', flexShrink: 0 }}
              onClick={(e) => handleRemove(e, p.name)}
            />
          </button>
          )
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
            {!isMac && (
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
            )}
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
