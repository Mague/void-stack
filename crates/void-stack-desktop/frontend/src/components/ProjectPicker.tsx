import { useState, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { Search, FolderOpen, Pencil, Trash2, Plus } from 'lucide-react'
import type { ProjectInfo } from '../types'

interface Props {
  projects: ProjectInfo[]
  selected: string | null
  onSelect: (name: string) => void
  onClose: () => void
  onToast: (message: string, kind?: 'ok' | 'err') => void
  /** When 'add', the picker opens with the add-project form expanded. */
  initialMode?: 'normal' | 'add'
}

export default function ProjectPicker({ projects, selected, onSelect, onClose, onToast, initialMode = 'normal' }: Props) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const [editing, setEditing] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const [editPath, setEditPath] = useState('')
  const [adding, setAdding] = useState(initialMode === 'add')
  const [addName, setAddName] = useState('')
  const [addPath, setAddPath] = useState('')
  const [busy, setBusy] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  // Click-outside closes the dropdown.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [onClose])

  const filtered = projects.filter(p => p.name.toLowerCase().includes(search.toLowerCase()))

  const startEdit = (p: ProjectInfo) => {
    setEditing(p.name); setEditName(p.name); setEditPath(p.path); setAdding(false)
  }

  const pickFolder = async (setter: (v: string) => void) => {
    const picked = await open({ directory: true, multiple: false })
    if (typeof picked === 'string') setter(picked)
  }

  const saveEdit = async () => {
    if (!editing || busy) return
    const orig = projects.find(p => p.name === editing)
    const newName = editName.trim() && editName.trim() !== editing ? editName.trim() : undefined
    const newPath = editPath.trim() && editPath.trim() !== orig?.path ? editPath.trim() : undefined
    if (!newName && !newPath) { setEditing(null); return }
    setBusy(true)
    try {
      const [updated] = await invoke<[ProjectInfo, string[]]>('update_project_cmd', { name: editing, newName, newPath })
      setEditing(null)
      window.dispatchEvent(new Event('void-refresh-projects'))
      if (updated.name !== editing) onSelect(updated.name)
      onToast(t('sidebar.editDone'))
    } catch (e) {
      onToast(String(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  const remove = async (name: string) => {
    if (!confirm(t('sidebar.confirmRemove', { name }))) return
    try {
      await invoke('remove_project_cmd', { name })
      window.dispatchEvent(new Event('void-refresh-projects'))
    } catch (e) {
      onToast(String(e), 'err')
    }
  }

  const add = async () => {
    if (!addName.trim() || !addPath.trim() || busy) return
    setBusy(true)
    try {
      await invoke('add_project', { name: addName.trim(), path: addPath.trim() })
      setAdding(false); setAddName(''); setAddPath('')
      window.dispatchEvent(new Event('void-refresh-projects'))
      onSelect(addName.trim())
    } catch (e) {
      onToast(String(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  const addBlock = adding ? (
    <div className="vs-edit-form">
      <input value={addName} onChange={e => setAddName(e.target.value)} placeholder={t('sidebar.namePlaceholder')} autoFocus />
      <div className="vs-edit-path">
        <input value={addPath} onChange={e => setAddPath(e.target.value)} placeholder={t('sidebar.pathPlaceholder')} onKeyDown={e => e.key === 'Enter' && add()} />
        <button className="vs-btn" onClick={() => pickFolder(setAddPath)} aria-label={t('sidebar.browse')}><FolderOpen size={14} /></button>
      </div>
      <div className="vs-edit-btns">
        <button className="vs-btn primary" onClick={add} disabled={busy || !addName.trim() || !addPath.trim()}>{t('sidebar.add')}</button>
        <button className="vs-btn" onClick={() => setAdding(false)}>{t('common.cancel')}</button>
      </div>
    </div>
  ) : (
    <button className="vs-picker-add" onClick={() => { setAdding(true); setEditing(null) }}>
      <Plus size={14} /> {t('sidebar.addProject')}
    </button>
  )

  return (
    <div className="vs-picker" ref={ref} role="menu">
      <div className="vs-picker-search">
        <Search size={13} />
        <input
          autoFocus={initialMode !== 'add'}
          placeholder={t('sidebar.search') || 'Search…'}
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
      </div>

      {/* Add-project is the first thing visible — no scrolling past the list. */}
      <div className="vs-picker-add-top">{addBlock}</div>

      {filtered.map(p => (
        editing === p.name ? (
          <div className="vs-edit-form" key={p.name}>
            <input value={editName} onChange={e => setEditName(e.target.value)} placeholder={t('sidebar.namePlaceholder')} autoFocus />
            <div className="vs-edit-path">
              <input value={editPath} onChange={e => setEditPath(e.target.value)} placeholder={t('sidebar.pathPlaceholder')} onKeyDown={e => e.key === 'Enter' && saveEdit()} />
              <button className="vs-btn" onClick={() => pickFolder(setEditPath)} aria-label={t('sidebar.browse')}><FolderOpen size={14} /></button>
            </div>
            <span style={{ fontSize: 10, color: 'var(--vs-text-3)' }}>{t('sidebar.editHint')}</span>
            <div className="vs-edit-btns">
              <button className="vs-btn primary" onClick={saveEdit} disabled={busy}>{busy ? '…' : t('sidebar.editSave')}</button>
              <button className="vs-btn" onClick={() => setEditing(null)}>{t('common.cancel')}</button>
            </div>
          </div>
        ) : (
          <div key={p.name} className={`vs-picker-item ${selected === p.name ? 'active' : ''}`} role="menuitem">
            <button className="vs-pi-name" onClick={() => { onSelect(p.name); onClose() }} style={{ background: 'none', textAlign: 'left' }}>
              {p.name}
            </button>
            <button className="vs-pi-act" onClick={() => startEdit(p)} aria-label={`${t('sidebar.edit')} ${p.name}`} title={t('sidebar.edit')}><Pencil size={12} /></button>
            <button className="vs-pi-act" onClick={() => remove(p.name)} aria-label={`${t('common.remove')} ${p.name}`} title={t('common.remove')}><Trash2 size={12} /></button>
          </div>
        )
      ))}
    </div>
  )
}
