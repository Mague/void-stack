import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Plus, Archive, Link2 } from 'lucide-react'

export interface BoardTask {
  id: string
  title: string
  priority?: string | null
  tags: string[]
  date?: string | null
  links: string[]
}
export interface BoardColumn {
  name: string
  tasks: BoardTask[]
}
export interface Board {
  project: string
  columns: BoardColumn[]
}

const PRIO_CLASS: Record<string, string> = { high: 'high', medium: 'medium', low: 'low' }

export default function BoardPanel({ project }: { project: string }) {
  const { t } = useTranslation()
  const [board, setBoard] = useState<Board | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [title, setTitle] = useState('')
  const [prio, setPrio] = useState('')
  const [dragOver, setDragOver] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    setError(null)
    invoke<Board>('board_get_cmd', { project })
      .then(setBoard)
      .catch(e => setError(String(e)))
  }, [project])

  useEffect(() => {
    setBoard(null)
    load()
  }, [load])

  const run = (cmd: string, args: Record<string, unknown>) => {
    setBusy(true)
    setError(null)
    invoke<Board>(cmd, { project, ...args })
      .then(setBoard)
      .catch(e => setError(String(e)))
      .finally(() => setBusy(false))
  }

  const addTask = () => {
    const trimmed = title.trim()
    if (!trimmed) return
    run('board_add_task_cmd', { title: trimmed, priority: prio || null })
    setTitle('')
  }

  const onDrop = (column: string, e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(null)
    const id = e.dataTransfer.getData('text/task-id')
    if (id) run('board_move_task_cmd', { id, column })
  }

  return (
    <div className="vs-board-panel">
      <div className="vs-board-toolbar">
        <input
          className="vs-input"
          value={title}
          placeholder={t('board.addPlaceholder')}
          onChange={e => setTitle(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && addTask()}
        />
        <select className="vs-input vs-board-prio" value={prio} onChange={e => setPrio(e.target.value)}>
          <option value="">{t('board.noPrio')}</option>
          <option value="high">{t('board.prioHigh')}</option>
          <option value="medium">{t('board.prioMedium')}</option>
          <option value="low">{t('board.prioLow')}</option>
        </select>
        <button className="vs-btn" onClick={addTask} disabled={busy || !title.trim()}>
          <Plus size={13} /> {t('board.add')}
        </button>
        <span className="vs-board-spacer" />
        <button className="vs-btn" onClick={() => run('board_archive_cmd', {})} disabled={busy || !board}>
          <Archive size={13} /> {t('board.archive')}
        </button>
      </div>
      {error && <p className="vs-search-err">{error}</p>}
      {!board && !error && <p className="vs-search-meta">{t('common.loading')}</p>}
      {board && (
        <div className="vs-board">
          {board.columns.map(col => (
            <div
              key={col.name}
              className={`vs-board-col ${dragOver === col.name ? 'dragover' : ''}`}
              onDragOver={e => {
                e.preventDefault()
                setDragOver(col.name)
              }}
              onDragLeave={() => setDragOver(d => (d === col.name ? null : d))}
              onDrop={e => onDrop(col.name, e)}
            >
              <div className="vs-board-col-head">
                <span>{col.name}</span>
                <span className="vs-board-count">{col.tasks.length}</span>
              </div>
              {col.tasks.map(task => (
                <div
                  key={task.id}
                  className="vs-board-task"
                  draggable
                  onDragStart={e => e.dataTransfer.setData('text/task-id', task.id)}
                >
                  <div className="vs-board-task-head">
                    <span className="vs-board-id">{task.id}</span>
                    {task.priority && (
                      <span className={`vs-board-prio-badge ${PRIO_CLASS[task.priority] ?? ''}`}>
                        {task.priority}
                      </span>
                    )}
                  </div>
                  <div className="vs-board-title">{task.title}</div>
                  {(task.tags.length > 0 || task.links.length > 0 || task.date) && (
                    <div className="vs-board-meta">
                      {task.tags.map(tag => (
                        <span key={tag} className="vs-board-tag">#{tag}</span>
                      ))}
                      {task.links.length > 0 && (
                        <span className="vs-board-links" title={task.links.join('\n')}>
                          <Link2 size={11} /> {task.links.length}
                        </span>
                      )}
                      {task.date && <span className="vs-board-date">{task.date}</span>}
                    </div>
                  )}
                </div>
              ))}
              {col.tasks.length === 0 && <div className="vs-board-empty">{t('board.empty')}</div>}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
