import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Plus, Archive, Link2, History, X, GitCommitHorizontal } from 'lucide-react'

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
export interface TaskEvent {
  commit: string
  date: string
  author: string
  column: string
}
export interface TaskHistory {
  id: string
  title: string
  priority?: string | null
  tags: string[]
  date?: string | null
  links: string[]
  current_column?: string | null
  archived: boolean
  events: TaskEvent[]
}

const PRIO_CLASS: Record<string, string> = { high: 'high', medium: 'medium', low: 'low' }

function statusOf(h: TaskHistory): { label: string; kind: string } {
  if (h.current_column) return { label: h.current_column, kind: 'open' }
  return h.archived ? { label: 'archived', kind: 'archived' } : { label: 'removed', kind: 'removed' }
}

function TaskDetailModal({
  project,
  id,
  onClose,
}: {
  project: string
  id: string
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [detail, setDetail] = useState<TaskHistory | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setDetail(null)
    setError(null)
    invoke<TaskHistory>('board_task_history_cmd', { project, id })
      .then(setDetail)
      .catch(e => setError(String(e)))
  }, [project, id])

  return (
    <div className="vs-veil" onClick={onClose}>
      <div className="vs-task-modal" onClick={e => e.stopPropagation()}>
        <div className="vs-task-modal-head">
          <span className="vs-board-id">{id}</span>
          {detail && (
            <span className={`vs-board-status ${statusOf(detail).kind}`}>
              {statusOf(detail).label}
            </span>
          )}
          <button className="vs-task-modal-close" onClick={onClose} aria-label={t('board.close')}>
            <X size={14} />
          </button>
        </div>
        {error && <p className="vs-search-err">{error}</p>}
        {!detail && !error && <p className="vs-search-meta">{t('common.loading')}</p>}
        {detail && (
          <div className="vs-task-modal-body">
            <div className="vs-task-modal-title">{detail.title}</div>
            <div className="vs-board-meta">
              {detail.priority && (
                <span className={`vs-board-prio-badge ${PRIO_CLASS[detail.priority] ?? ''}`}>
                  {detail.priority}
                </span>
              )}
              {detail.tags.map(tag => (
                <span key={tag} className="vs-board-tag">#{tag}</span>
              ))}
              {detail.date && (
                <span className="vs-board-date">{t('board.created')} {detail.date}</span>
              )}
            </div>
            {detail.links.length > 0 && (
              <div className="vs-task-links">
                <div className="vs-task-section">{t('board.linksTitle')}</div>
                {detail.links.map(link => (
                  <div key={link} className="vs-task-link">
                    <Link2 size={11} /> <code>{link}</code>
                  </div>
                ))}
              </div>
            )}
            {detail.events.length > 0 && (
              <div className="vs-task-timeline">
                <div className="vs-task-section">{t('board.timeline')}</div>
                {detail.events.map((e, i) => (
                  <div key={i} className="vs-task-event">
                    <GitCommitHorizontal size={12} />
                    <code className="vs-task-commit">
                      {e.commit === '(uncommitted)' ? t('board.uncommitted') : e.commit}
                    </code>
                    <span className="vs-task-col">→ {e.column}</span>
                    <span className="vs-task-when">
                      {e.date}
                      {e.author ? ` · ${e.author}` : ''}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

export default function BoardPanel({ project }: { project: string }) {
  const { t } = useTranslation()
  const [board, setBoard] = useState<Board | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [title, setTitle] = useState('')
  const [prio, setPrio] = useState('')
  const [dragOver, setDragOver] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showHistory, setShowHistory] = useState(false)
  const [history, setHistory] = useState<TaskHistory[] | null>(null)
  const [detailId, setDetailId] = useState<string | null>(null)

  const load = useCallback(() => {
    setError(null)
    invoke<Board>('board_get_cmd', { project })
      .then(setBoard)
      .catch(e => setError(String(e)))
  }, [project])

  useEffect(() => {
    setBoard(null)
    setHistory(null)
    setShowHistory(false)
    setDetailId(null)
    load()
  }, [load])

  useEffect(() => {
    if (!showHistory) return
    setHistory(null)
    invoke<TaskHistory[]>('board_history_cmd', { project })
      .then(setHistory)
      .catch(e => setError(String(e)))
  }, [showHistory, project, board])

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
        <button
          className={`vs-btn ${showHistory ? 'active' : ''}`}
          onClick={() => setShowHistory(v => !v)}
        >
          <History size={13} /> {t('board.history')}
        </button>
        <button className="vs-btn" onClick={() => run('board_archive_cmd', {})} disabled={busy || !board}>
          <Archive size={13} /> {t('board.archive')}
        </button>
      </div>
      {error && <p className="vs-search-err">{error}</p>}
      {!board && !error && <p className="vs-search-meta">{t('common.loading')}</p>}
      {board && showHistory && (
        <div className="vs-board-history">
          {!history && <p className="vs-search-meta">{t('common.loading')}</p>}
          {history && history.length === 0 && (
            <p className="vs-search-meta">{t('board.noHistory')}</p>
          )}
          {history &&
            history.map(h => {
              const st = statusOf(h)
              return (
                <button key={h.id} className="vs-board-hrow" onClick={() => setDetailId(h.id)}>
                  <span className="vs-board-id">{h.id}</span>
                  <span className={`vs-board-status ${st.kind}`}>{st.label}</span>
                  <span className="vs-board-hrow-title">{h.title}</span>
                  <span className="vs-board-hrow-trail">
                    {h.events.map(e => e.column).join(' → ')}
                  </span>
                </button>
              )
            })}
        </div>
      )}
      {board && !showHistory && (
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
                  onClick={() => setDetailId(task.id)}
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
      {detailId && (
        <TaskDetailModal project={project} id={detailId} onClose={() => setDetailId(null)} />
      )}
    </div>
  )
}
