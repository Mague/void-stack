import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { RefreshCw, Download, Container, Database, Server, Globe } from 'lucide-react'
import type { DockerAnalysisDto, DockerGenerateResultDto } from '../types'
import CopyButton from './CopyButton'

interface Props {
  project: string
}

const kindIcon = (kind: string) => {
  switch (kind) {
    case 'database': return <Database size={14} />
    case 'cache': return <Server size={14} />
    case 'proxy': return <Globe size={14} />
    case 'app': return <Container size={14} />
    default: return <Container size={14} />
  }
}

const kindColor = (kind: string) => {
  switch (kind) {
    case 'database': return 'var(--cyan, #00d4ff)'
    case 'cache': return 'var(--red)'
    case 'proxy': return 'var(--green)'
    case 'queue': return 'var(--yellow, #f0c040)'
    case 'app': return 'var(--text-bright)'
    default: return 'var(--text-secondary)'
  }
}

export default function DockerPanel({ project }: Props) {
  const { t } = useTranslation()
  const [analysis, setAnalysis] = useState<DockerAnalysisDto | null>(null)
  const [loading, setLoading] = useState(false)
  const [generated, setGenerated] = useState<DockerGenerateResultDto | null>(null)
  const [generating, setGenerating] = useState(false)
  const [saving, setSaving] = useState(false)
  const [activeTab, setActiveTab] = useState<'analysis' | 'dockerfile' | 'compose'>('analysis')

  const runAnalysis = async () => {
    setLoading(true)
    try {
      const result = await invoke<DockerAnalysisDto>('docker_analyze', { project })
      setAnalysis(result)
    } catch (e) {
      console.error('docker analysis failed:', e)
    }
    setLoading(false)
  }

  useEffect(() => { runAnalysis() }, [project])

  const generateFiles = async (genDockerfile: boolean, genCompose: boolean, save: boolean) => {
    if (save) setSaving(true)
    else setGenerating(true)
    try {
      const result = await invoke<DockerGenerateResultDto>('docker_generate', {
        project,
        generateDockerfile: genDockerfile,
        generateCompose: genCompose,
        save,
      })
      setGenerated(result)
      if (save && result.saved_paths.length > 0) {
        runAnalysis() // Refresh analysis after saving
      }
    } catch (e) {
      console.error('docker generate failed:', e)
    }
    setGenerating(false)
    setSaving(false)
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('docker.title')}</h2>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <button className="btn btn-primary btn-sm" onClick={runAnalysis} disabled={loading}>
            {loading ? <><span className="loading-spinner" /> {t('common.loading')}</> : <><RefreshCw size={12} /> {t('docker.analyze')}</>}
          </button>
        </div>
      </div>

      {/* Tab navigation */}
      <div className="docker-tabs">
        <button className={`docker-tab ${activeTab === 'analysis' ? 'active' : ''}`} onClick={() => setActiveTab('analysis')}>
          {t('docker.analysis')}
        </button>
        <button className={`docker-tab ${activeTab === 'dockerfile' ? 'active' : ''}`} onClick={() => setActiveTab('dockerfile')}>
          Dockerfile
        </button>
        <button className={`docker-tab ${activeTab === 'compose' ? 'active' : ''}`} onClick={() => setActiveTab('compose')}>
          Compose
        </button>
      </div>

      {/* Analysis tab */}
      {activeTab === 'analysis' && analysis && (
        <div className="docker-analysis">
          <div className="docker-status-row">
            <span className={`docker-badge ${analysis.has_dockerfile ? 'found' : 'missing'}`}>
              {analysis.has_dockerfile ? 'Dockerfile' : `Dockerfile ${t('docker.notFound')}`}
            </span>
            <span className={`docker-badge ${analysis.has_compose ? 'found' : 'missing'}`}>
              {analysis.has_compose ? 'docker-compose' : `docker-compose ${t('docker.notFound')}`}
            </span>
          </div>

          {analysis.dockerfile && (
            <div className="docker-section">
              <h3>Dockerfile</h3>
              <div className="docker-stages">
                {analysis.dockerfile.stages.map((s, i) => (
                  <div key={i} className="docker-stage-item">
                    <span className="docker-stage-label">Stage {i}</span>
                    <span className="docker-stage-image">{s.base_image}</span>
                    {s.name && <span className="docker-stage-name">AS {s.name}</span>}
                  </div>
                ))}
              </div>
              {analysis.dockerfile.exposed_ports.length > 0 && (
                <div className="docker-info-row">
                  <span className="docker-info-label">Ports:</span>
                  <span>{analysis.dockerfile.exposed_ports.join(', ')}</span>
                </div>
              )}
              {analysis.dockerfile.cmd && (
                <div className="docker-info-row">
                  <span className="docker-info-label">CMD:</span>
                  <code>{analysis.dockerfile.cmd}</code>
                </div>
              )}
            </div>
          )}

          {analysis.compose && (
            <div className="docker-section">
              <h3>Docker Compose</h3>
              <div className="docker-compose-grid">
                {analysis.compose.services.map(svc => (
                  <div key={svc.name} className="docker-compose-card">
                    <div className="docker-compose-card-header" style={{ borderLeftColor: kindColor(svc.kind) }}>
                      <span className="docker-compose-icon">{kindIcon(svc.kind)}</span>
                      <span className="docker-compose-name">{svc.name}</span>
                      <span className="docker-compose-kind">{svc.kind}</span>
                    </div>
                    {svc.image && <div className="docker-compose-image">{svc.image}</div>}
                    {svc.ports.length > 0 && (
                      <div className="docker-compose-ports">
                        {svc.ports.map((p, i) => (
                          <span key={i} className="docker-port-badge">{p.host}:{p.container}</span>
                        ))}
                      </div>
                    )}
                    {svc.depends_on.length > 0 && (
                      <div className="docker-compose-deps">
                        {t('docker.dependsOn')}: {svc.depends_on.join(', ')}
                      </div>
                    )}
                    {svc.has_healthcheck && <span className="docker-health-badge">{t('docker.healthcheck')}</span>}
                  </div>
                ))}
              </div>
              {analysis.compose.volumes.length > 0 && (
                <div className="docker-info-row">
                  <span className="docker-info-label">Volumes:</span>
                  <span>{analysis.compose.volumes.join(', ')}</span>
                </div>
              )}
            </div>
          )}

          {!analysis.has_dockerfile && !analysis.has_compose && (
            <div className="analysis-empty">
              <p>{t('docker.noArtifacts')}</p>
              <button className="btn btn-primary" onClick={() => { setActiveTab('dockerfile'); generateFiles(true, true, false) }}>
                {t('docker.generateAll')}
              </button>
            </div>
          )}
        </div>
      )}

      {/* Dockerfile tab */}
      {activeTab === 'dockerfile' && (
        <div className="docker-generate-section">
          <div className="docker-generate-actions">
            <button className="btn btn-primary btn-sm" onClick={() => generateFiles(true, false, false)} disabled={generating}>
              {generating ? <><span className="loading-spinner" /> {t('common.loading')}</> : t('docker.generateDockerfile')}
            </button>
            {generated?.dockerfile && (
              <button className="btn btn-sm" onClick={() => generateFiles(true, false, true)} disabled={saving}>
                {saving ? <><span className="loading-spinner" /></> : <><Download size={12} /> {t('docker.save')}</>}
              </button>
            )}
            {generated?.dockerfile && <CopyButton text={generated.dockerfile} />}
          </div>
          {generated?.dockerfile && (
            <pre className="docker-code-block">{generated.dockerfile}</pre>
          )}
          {!generated?.dockerfile && !generating && (
            <div className="analysis-empty">
              <p>{t('docker.clickGenerate')}</p>
            </div>
          )}
        </div>
      )}

      {/* Compose tab */}
      {activeTab === 'compose' && (
        <div className="docker-generate-section">
          <div className="docker-generate-actions">
            <button className="btn btn-primary btn-sm" onClick={() => generateFiles(false, true, false)} disabled={generating}>
              {generating ? <><span className="loading-spinner" /> {t('common.loading')}</> : t('docker.generateCompose')}
            </button>
            {generated?.compose && (
              <button className="btn btn-sm" onClick={() => generateFiles(false, true, true)} disabled={saving}>
                {saving ? <><span className="loading-spinner" /></> : <><Download size={12} /> {t('docker.save')}</>}
              </button>
            )}
            {generated?.compose && <CopyButton text={generated.compose} />}
          </div>
          {generated?.compose && (
            <pre className="docker-code-block">{generated.compose}</pre>
          )}
          {!generated?.compose && !generating && (
            <div className="analysis-empty">
              <p>{t('docker.clickGenerate')}</p>
            </div>
          )}
        </div>
      )}

      {generated?.saved_paths && generated.saved_paths.length > 0 && (
        <div className="docker-saved-notice">
          {t('docker.savedTo')}: {generated.saved_paths.join(', ')}
        </div>
      )}
    </div>
  )
}
