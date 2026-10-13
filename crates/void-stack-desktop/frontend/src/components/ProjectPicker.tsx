import { useState, useRef, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { open, ask } from '@tauri-apps/plugin-dialog'
import { Search, FolderOpen, Pencil, Trash2, Plus, Check } from 'lucide-react'
import type { ProjectInfo } from '../types'
import { forgetProject, getRecentProjects, orderProjects } from '../projectMemory'
import { composeTags, labelTags, ownershipOf, parseTagText, sameTags, type Ownership } from '../projectTags'
import OwnershipChip from './OwnershipChip'

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
  const [editOwnership, setEditOwnership] = useState<Ownership>(null)
  const [editLabels, setEditLabels] = useState('')
  const [adding, setAdding] = useState(initialMode === 'add')
  const [addName, setAddName] = useState('')
  const [addPath, setAddPath] = useState('')
  const [busy, setBusy] = useState(false)
  // Keyboard highlight within the filtered list (↑/↓ + Enter).
  const [cursor, setCursor] = useState(0)
  const ref = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  // Click-outside closes the dropdown.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [onClose])

  // Selected first, then recently used, then the rest — the project you
  // want is almost always at the top without typing.
  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase().replace(/^#/, '')
    const ordered = orderProjects(projects, selected, getRecentProjects())
    if (!q) return ordered
    // Tags match by stored name or by the label shown ("tercero" / "propio").
    const tagText = (p: ProjectInfo) => {
      const own = ownershipOf(p.tags)
      const shown = own ? t(own === 'own' ? 'sidebar.tagOwn' : 'sidebar.tagThirdParty') : ''
      return [...(p.tags ?? []), shown].join(' ').toLowerCase()
    }
    // Name matches rank above path/tag-only matches.
    const byName = ordered.filter(p => p.name.toLowerCase().includes(q))
    const byOther = ordered.filter(p => !p.name.toLowerCase().includes(q)
      && (p.path.toLowerCase().includes(q) || tagText(p).includes(q)))
    return [...byName, ...byOther]
  }, [projects, selected, search, t])

  const onSearchChange = (value: string) => {
    setSearch(value)
    setCursor(0)
  }

  // Keep the highlighted row visible while arrowing through a long list.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-idx="${cursor}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  const choose = (name: string) => {
    onSelect(name)
    onClose()
  }

  const onSearchKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setCursor(c => Math.min(c + 1, Math.max(filtered.length - 1, 0)))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setCursor(c => Math.max(c - 1, 0))
    } else if (e.key === 'Enter') {
      const target = filtered[cursor]
      if (target) choose(target.name)
    } else if (e.key === 'Escape') {
      onClose()
    }
  }

  const startEdit = (p: ProjectInfo) => {
    setEditing(p.name); setEditName(p.name); setEditPath(p.path); setAdding(false)
    setEditOwnership(ownershipOf(p.tags)); setEditLabels(labelTags(p.tags).join(', '))
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
    const wanted = composeTags(editOwnership, parseTagText(editLabels))
    const tags = sameTags(wanted, orig?.tags ?? []) ? undefined : wanted
    if (!newName && !newPath && !tags) { setEditing(null); return }
    setBusy(true)
    try {
      const [updated] = await invoke<[ProjectInfo, string[]]>('update_project_cmd', { name: editing, newName, newPath, tags })
      setEditing(null)
      if (updated.name !== editing) forgetProject(editing, updated.name)
      window.dispatchEvent(new Event('void-refresh-projects'))
      if (updated.name !== editing && selected === editing) onSelect(updated.name)
      onToast(t('sidebar.editDone'))
    } catch (e) {
      onToast(String(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  const remove = async (name: string) => {
    // window.confirm() isn't reliable inside the Tauri webview (it can
    // resolve without showing anything, so delete did nothing) — use the
    // native dialog instead.
    const ok = await ask(t('sidebar.confirmRemove', { name }), { kind: 'warning' })
    if (!ok) return
    try {
      await invoke('remove_project_cmd', { name })
      forgetProject(name)
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
          onChange={e => onSearchChange(e.target.value)}
          onKeyDown={onSearchKey}
        />
      </div>

      {/* Add-project is the first thing visible — no scrolling past the list. */}
      <div className="vs-picker-add-top">{addBlock}</div>

      <div ref={listRef}>
        {filtered.length === 0 && <div className="vs-picker-empty">{t('sidebar.noMatches')}</div>}
        {filtered.map((p, idx) => (
          editing === p.name ? (
            <div className="vs-edit-form" key={p.name}>
              <input value={editName} onChange={e => setEditName(e.target.value)} placeholder={t('sidebar.namePlaceholder')} autoFocus />
              <div className="vs-edit-path">
                <input value={editPath} onChange={e => setEditPath(e.target.value)} placeholder={t('sidebar.pathPlaceholder')} onKeyDown={e => e.key === 'Enter' && saveEdit()} />
                <button className="vs-btn" onClick={() => pickFolder(setEditPath)} aria-label={t('sidebar.browse')}><FolderOpen size={14} /></button>
              </div>
              <span style={{ fontSize: 10, color: 'var(--vs-text-3)' }}>{t('sidebar.editHint')}</span>
              <div className="vs-seg" role="radiogroup" aria-label={t('sidebar.ownership')}>
                {([null, 'own', 'third-party'] as const).map(o => (
                  <button
                    key={o ?? 'none'}
                    type="button"
                    role="radio"
                    aria-checked={editOwnership === o}
                    className={editOwnership === o ? 'on' : ''}
                    onClick={() => setEditOwnership(o)}
                  >
                    {t(o === 'own' ? 'sidebar.tagOwn' : o === 'third-party' ? 'sidebar.tagThirdParty' : 'sidebar.ownershipNone')}
                  </button>
                ))}
              </div>
              <input value={editLabels} onChange={e => setEditLabels(e.target.value)} placeholder={t('sidebar.tagsPlaceholder')} onKeyDown={e => e.key === 'Enter' && saveEdit()} />
              <div className="vs-edit-btns">
                <button className="vs-btn primary" onClick={saveEdit} disabled={busy}>{busy ? '…' : t('sidebar.editSave')}</button>
                <button className="vs-btn" onClick={() => setEditing(null)}>{t('common.cancel')}</button>
              </div>
            </div>
          ) : (
            // The whole row selects (not just the label's text box), so a
            // click on the padding or the gap next to the name never misses.
            <div
              key={p.name}
              data-idx={idx}
              className={`vs-picker-item ${selected === p.name ? 'active' : ''} ${cursor === idx ? 'cursor' : ''}`}
              role="menuitemradio"
              aria-checked={selected === p.name}
              title={[p.path, ...labelTags(p.tags).map(l => `#${l}`)].join('  ')}
              onClick={() => choose(p.name)}
              onMouseMove={() => cursor !== idx && setCursor(idx)}
            >
              <span className="vs-pi-check" aria-hidden>{selected === p.name && <Check size={12} />}</span>
              <span className="vs-pi-name">{p.name}</span>
              <OwnershipChip tags={p.tags} />
              <button
                className="vs-pi-act"
                onClick={e => { e.stopPropagation(); startEdit(p) }}
                aria-label={`${t('sidebar.edit')} ${p.name}`}
                title={t('sidebar.edit')}
              >
                <Pencil size={12} />
              </button>
              <button
                className="vs-pi-act danger"
                onClick={e => { e.stopPropagation(); remove(p.name) }}
                aria-label={`${t('common.remove')} ${p.name}`}
                title={t('common.remove')}
              >
                <Trash2 size={12} />
              </button>
            </div>
          )
        ))}
      </div>
    </div>
  )
}
