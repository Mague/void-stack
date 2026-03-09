import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { FolderOpen, ChevronRight, ArrowLeft, Check, X } from 'lucide-react'

interface BrowseEntry {
  name: string
  path: string
}

interface Props {
  distro: string
  onSelect: (linuxPath: string) => void
  onClose: () => void
}

export default function WslBrowser({ distro, onSelect, onClose }: Props) {
  const { t } = useTranslation()
  const [currentPath, setCurrentPath] = useState(`\\\\wsl.localhost\\${distro}`)
  const [entries, setEntries] = useState<BrowseEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const linuxPath = () => {
    const m = currentPath.match(/^\\\\(?:wsl[\$]|wsl\.localhost)\\[^\\]+(.*)$/)
    if (m) return m[1].replace(/\\/g, '/') || '/'
    return currentPath
  }

  const loadDir = async (path: string) => {
    setLoading(true)
    setError('')
    try {
      const list = await invoke<BrowseEntry[]>('browse_directory', { path })
      setEntries(list)
      setCurrentPath(path)
    } catch (e) {
      setError(String(e))
      setEntries([])
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadDir(`\\\\wsl.localhost\\${distro}`)
  }, [distro])

  const goUp = () => {
    const parts = currentPath.split('\\').filter(Boolean)
    if (parts.length <= 2) return // Don't go above \\wsl.localhost\distro
    parts.pop()
    const parent = '\\\\' + parts.join('\\')
    loadDir(parent)
  }

  const canGoUp = currentPath.split('\\').filter(Boolean).length > 2

  return (
    <div className="wsl-browser-overlay" onClick={onClose}>
      <div className="wsl-browser" onClick={e => e.stopPropagation()}>
        <div className="wsl-browser-header">
          <span className="wsl-browser-title">{distro}</span>
          <span className="wsl-browser-path">{linuxPath()}</span>
          <button className="btn btn-sm" onClick={onClose}><X size={14} /></button>
        </div>

        <div className="wsl-browser-toolbar">
          <button className="btn btn-sm" onClick={goUp} disabled={!canGoUp}>
            <ArrowLeft size={14} />
          </button>
          <span className="wsl-browser-breadcrumb">{linuxPath()}</span>
        </div>

        <div className="wsl-browser-list">
          {loading && <div className="wsl-browser-loading"><span className="loading-spinner" /></div>}
          {error && <div className="wsl-browser-error">{error}</div>}
          {!loading && entries.length === 0 && !error && (
            <div className="wsl-browser-empty">{t('sidebar.emptyDir')}</div>
          )}
          {entries.map(entry => (
            <button
              key={entry.name}
              className="wsl-browser-item"
              onClick={() => loadDir(entry.path)}
            >
              <FolderOpen size={14} />
              <span>{entry.name}</span>
              <ChevronRight size={12} className="wsl-browser-chevron" />
            </button>
          ))}
        </div>

        <div className="wsl-browser-footer">
          <button className="btn btn-primary btn-sm" onClick={() => onSelect(currentPath)}>
            <Check size={12} /> {t('sidebar.selectFolder')}
          </button>
        </div>
      </div>
    </div>
  )
}
