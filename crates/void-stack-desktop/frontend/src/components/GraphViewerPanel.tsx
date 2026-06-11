import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { RefreshCw, ExternalLink, Network } from 'lucide-react'

interface Props {
  project: string
}

/**
 * In-app interactive dependency-graph viewer. Loads the self-contained
 * graph.html (Cytoscape inlined) into an iframe via srcDoc so it's
 * explorable without leaving the app. "Open in browser" writes the file
 * and opens it externally for a full-window view.
 */
export default function GraphViewerPanel({ project }: Props) {
  const { t } = useTranslation()
  const [html, setHtml] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setHtml(await invoke<string>('get_graph_html_cmd', { project }))
    } catch (e) {
      setError(String(e))
      setHtml(null)
    } finally {
      setLoading(false)
    }
  }, [project])

  useEffect(() => { load() }, [load])

  // The embedded graph (sandboxed iframe) posts {source:'void-graph'} when a
  // node/file is clicked; open it in the user's editor at that line.
  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      const d = e.data
      if (d && d.source === 'void-graph' && d.type === 'open' && typeof d.file === 'string') {
        invoke('open_in_editor_cmd', { project, file: d.file, line: d.line ?? 1 })
          .catch(err => setError(String(err)))
      }
    }
    window.addEventListener('message', onMsg)
    return () => window.removeEventListener('message', onMsg)
  }, [project])

  const openInBrowser = async () => {
    try {
      const path = await invoke<string>('generate_graph_html', { project })
      const opener = await import('@tauri-apps/plugin-opener')
      await opener.openPath(path)
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <div className="vs-graphview">
      <div className="vs-graphview-bar">
        <Network size={14} />
        <span>{t('graphView.title')}</span>
        <span className="vs-spacer" style={{ flex: 1 }} />
        <button className="vs-btn" onClick={load} disabled={loading}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <RefreshCw size={13} />}
          {t('graphView.refresh')}
        </button>
        <button className="vs-btn" onClick={openInBrowser} disabled={!html}>
          <ExternalLink size={13} /> {t('graphView.openBrowser')}
        </button>
      </div>

      {error ? (
        <div className="vs-empty">
          <Network size={26} />
          <span>{t('graphView.error')}</span>
          <code style={{ fontSize: 11, color: 'var(--vs-text-3)', maxWidth: 520, textAlign: 'center' }}>{error}</code>
          <button className="vs-btn" onClick={load}>{t('graphView.retry')}</button>
        </div>
      ) : loading && !html ? (
        <div className="vs-empty"><RefreshCw size={20} className="vs-spin" /><span>{t('graphView.building')}</span></div>
      ) : html ? (
        <iframe
          className="vs-graphview-frame"
          title={t('graphView.title')}
          srcDoc={html}
          sandbox="allow-scripts"
        />
      ) : null}
    </div>
  )
}
