import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { FileText, ChevronDown } from 'lucide-react'

interface Props {
  project: string
  readme: string | null
  setReadme: (r: string | null) => void
}

export default function DocsPanel({ project, readme, setReadme }: Props) {
  const [docFiles, setDocFiles] = useState<string[]>([])
  const [activeFile, setActiveFile] = useState<string | null>(null)
  const [content, setContent] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Load README and doc list on mount / project change
  useEffect(() => {
    setError(null)

    // Load README
    if (!readme) {
      invoke<string>('read_project_readme', { project })
        .then(r => { setReadme(r); setContent(r); setActiveFile(null) })
        .catch(e => setError(String(e)))
    } else {
      setContent(readme)
    }

    // Load available docs
    invoke<string[]>('list_project_docs', { project })
      .then(setDocFiles)
      .catch(() => setDocFiles([]))
  }, [project])

  const loadDoc = async (filename: string) => {
    setLoading(true)
    setError(null)
    try {
      const text = await invoke<string>('read_project_doc', { project, filename })
      setContent(text)
      setActiveFile(filename)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const showReadme = () => {
    setActiveFile(null)
    setContent(readme)
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>Documentación</h2>
        {docFiles.length > 1 && (
          <div className="doc-selector">
            <button
              className={`btn btn-sm ${!activeFile ? 'btn-primary' : ''}`}
              onClick={showReadme}
            >
              README
            </button>
            <div className="doc-dropdown">
              <button className="btn btn-sm">
                <ChevronDown size={10} /> Otros docs
              </button>
              <div className="doc-dropdown-content">
                {docFiles.filter(f => !f.toLowerCase().startsWith('readme')).map(f => (
                  <button
                    key={f}
                    className={`doc-dropdown-item ${activeFile === f ? 'active' : ''}`}
                    onClick={() => loadDoc(f)}
                  >
                    <FileText size={10} /> {f}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </div>

      {loading && (
        <div className="analysis-empty">
          <span className="loading-spinner" style={{ width: 24, height: 24 }} />
        </div>
      )}

      {error && !content && (
        <div className="analysis-empty">
          <FileText size={32} style={{ opacity: 0.2 }} />
          <p>{error}</p>
        </div>
      )}

      {content && !loading && (
        <div className="docs-content">
          {activeFile && (
            <div className="doc-filename">
              <FileText size={12} /> {activeFile}
            </div>
          )}
          <Markdown remarkPlugins={[remarkGfm]}>{content}</Markdown>
        </div>
      )}
    </div>
  )
}
