import type { ProjectInfo, ServiceStateDto } from '../types'
import { FolderOpen, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface Props {
  projects: ProjectInfo[]
  selected: string | null
  onSelect: (name: string) => void
  states: ServiceStateDto[]
}

export default function Sidebar({ projects, selected, onSelect, states }: Props) {
  const [showAdd, setShowAdd] = useState(false)
  const [newName, setNewName] = useState('')
  const [newPath, setNewPath] = useState('')

  const runningCount = states.filter(s => s.status === 'RUNNING').length

  const handleAdd = async () => {
    if (!newName || !newPath) return
    try {
      await invoke('add_project', { name: newName, path: newPath })
      setNewName('')
      setNewPath('')
      setShowAdd(false)
      window.location.reload()
    } catch (e) {
      alert(e)
    }
  }

  const handleRemove = async (e: React.MouseEvent, name: string) => {
    e.stopPropagation()
    if (!confirm(`Eliminar proyecto "${name}"?`)) return
    try {
      await invoke('remove_project_cmd', { name })
      window.location.reload()
    } catch (err) {
      alert(err)
    }
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <h1 className="logo">
          <span className="logo-dot" />
          DevLaunch
        </h1>
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
              placeholder="nombre"
              value={newName}
              onChange={e => setNewName(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleAdd()}
            />
            <input
              placeholder="ruta del proyecto"
              value={newPath}
              onChange={e => setNewPath(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleAdd()}
            />
            <div className="add-form-buttons">
              <button className="btn btn-primary btn-sm" onClick={handleAdd}>Agregar</button>
              <button className="btn btn-sm" onClick={() => setShowAdd(false)}>Cancelar</button>
            </div>
          </div>
        ) : (
          <button className="btn btn-add" onClick={() => setShowAdd(true)}>
            <Plus size={14} /> Agregar proyecto
          </button>
        )}
      </div>
    </aside>
  )
}
