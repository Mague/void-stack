import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { HardDrive, Trash2, RefreshCw, Database, Package, Cpu, FolderArchive } from 'lucide-react'

interface SpaceEntry {
  name: string
  category: string
  path: string
  size_bytes: number
  size_human: string
  deletable: boolean
  restore_hint: string
}

interface Props {
  project: string
  projectEntries: SpaceEntry[]
  setProjectEntries: (e: SpaceEntry[]) => void
  globalEntries: SpaceEntry[]
  setGlobalEntries: (e: SpaceEntry[]) => void
}

const categoryIcon = (cat: string) => {
  switch (cat) {
    case 'Dependencias': return <Package size={12} />
    case 'Build': return <FolderArchive size={12} />
    case 'Caché global': return <Database size={12} />
    case 'Modelos AI': return <Cpu size={12} />
    default: return <HardDrive size={12} />
  }
}

const categoryColor = (cat: string) => {
  switch (cat) {
    case 'Dependencias': return 'var(--cyan)'
    case 'Build': return 'var(--amber)'
    case 'Caché global': return 'var(--purple)'
    case 'Modelos AI': return 'var(--green)'
    default: return 'var(--text-secondary)'
  }
}

export default function SpacePanel({ project, projectEntries, setProjectEntries, globalEntries, setGlobalEntries }: Props) {
  const [scanning, setScanning] = useState(false)
  const [deleting, setDeleting] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)

  const scanAll = async () => {
    setScanning(true)
    setMessage(null)
    try {
      const [proj, global] = await Promise.all([
        invoke<SpaceEntry[]>('scan_project_space', { project }),
        invoke<SpaceEntry[]>('scan_global_space'),
      ])
      setProjectEntries(proj)
      setGlobalEntries(global)
    } catch (e) {
      console.error(e)
    } finally {
      setScanning(false)
    }
  }

  const handleDelete = async (entry: SpaceEntry) => {
    if (!confirm(`Eliminar "${entry.name}" (${entry.size_human})?\n\nPara restaurar: ${entry.restore_hint}`)) return
    setDeleting(entry.path)
    try {
      const result = await invoke<string>('delete_space_entry', { path: entry.path })
      setMessage(result)
      // Re-scan after deletion
      await scanAll()
    } catch (e) {
      setMessage(`Error: ${e}`)
    } finally {
      setDeleting(null)
    }
  }

  const totalProject = projectEntries.reduce((sum, e) => sum + e.size_bytes, 0)
  const totalGlobal = globalEntries.reduce((sum, e) => sum + e.size_bytes, 0)
  const totalAll = totalProject + totalGlobal
  const hasData = projectEntries.length > 0 || globalEntries.length > 0

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>Espacio en disco</h2>
        <button className="btn btn-primary" onClick={scanAll} disabled={scanning}>
          {scanning ? <><span className="loading-spinner" /> Escaneando...</> : <><RefreshCw size={12} /> Escanear</>}
        </button>
      </div>

      {message && (
        <div className="space-message">{message}</div>
      )}

      {!hasData && !scanning && (
        <div className="analysis-empty">
          <HardDrive size={32} style={{ opacity: 0.2 }} />
          <p>Presiona "Escanear" para analizar el uso de espacio</p>
        </div>
      )}

      {hasData && (
        <>
          {/* Summary */}
          <div className="space-summary">
            <div className="space-total">
              <span className="space-total-value">{formatSize(totalAll)}</span>
              <span className="space-total-label">total recuperable</span>
            </div>
            <div className="space-breakdown">
              {totalProject > 0 && (
                <span className="space-breakdown-item">
                  <span style={{ color: 'var(--cyan)' }}>Proyecto:</span> {formatSize(totalProject)}
                </span>
              )}
              {totalGlobal > 0 && (
                <span className="space-breakdown-item">
                  <span style={{ color: 'var(--purple)' }}>Global:</span> {formatSize(totalGlobal)}
                </span>
              )}
            </div>
          </div>

          {/* Project entries */}
          {projectEntries.length > 0 && (
            <div className="space-section">
              <h3 className="space-section-title">Proyecto</h3>
              <div className="space-entries">
                {projectEntries.map(entry => (
                  <SpaceRow
                    key={entry.path}
                    entry={entry}
                    deleting={deleting === entry.path}
                    onDelete={() => handleDelete(entry)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Global entries */}
          {globalEntries.length > 0 && (
            <div className="space-section">
              <h3 className="space-section-title">Caché global & Modelos AI</h3>
              <div className="space-entries">
                {globalEntries.map(entry => (
                  <SpaceRow
                    key={entry.path}
                    entry={entry}
                    deleting={deleting === entry.path}
                    onDelete={() => handleDelete(entry)}
                  />
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function SpaceRow({ entry, deleting, onDelete }: { entry: SpaceEntry, deleting: boolean, onDelete: () => void }) {
  return (
    <div className="space-row">
      <div className="space-row-icon" style={{ color: categoryColor(entry.category) }}>
        {categoryIcon(entry.category)}
      </div>
      <div className="space-row-info">
        <div className="space-row-name">{entry.name}</div>
        <div className="space-row-path">{entry.path}</div>
      </div>
      <div className="space-row-category">
        <span className="space-category-badge" style={{ borderColor: categoryColor(entry.category), color: categoryColor(entry.category) }}>
          {entry.category}
        </span>
      </div>
      <div className="space-row-size">{entry.size_human}</div>
      <div className="space-row-action">
        {entry.deletable && (
          <button
            className="btn btn-danger btn-sm"
            onClick={onDelete}
            disabled={deleting}
            title={`Restaurar: ${entry.restore_hint}`}
          >
            {deleting ? <span className="loading-spinner" /> : <Trash2 size={10} />}
          </button>
        )}
      </div>
    </div>
  )
}

function formatSize(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${bytes} B`
}
