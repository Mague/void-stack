import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Shield, AlertTriangle, AlertCircle, Info, ShieldCheck, RefreshCw } from 'lucide-react'

interface SecurityFinding {
  id: string
  severity: string
  category: string
  title: string
  description: string
  file_path: string | null
  line_number: number | null
  remediation: string
}

interface AuditSummary {
  critical: number
  high: number
  medium: number
  low: number
  info: number
  total: number
  risk_score: number
}

export interface AuditResult {
  project_name: string
  timestamp: string
  findings: SecurityFinding[]
  summary: AuditSummary
}

interface Props {
  project: string
  audit: AuditResult | null
  setAudit: (a: AuditResult | null) => void
}

const severityIcon = (severity: string) => {
  switch (severity) {
    case 'critical': return <AlertCircle size={14} />
    case 'high': return <AlertTriangle size={14} />
    case 'medium': return <AlertTriangle size={12} />
    case 'low': return <Info size={12} />
    default: return <Info size={12} />
  }
}

const severityColor = (severity: string) => {
  switch (severity) {
    case 'critical': return 'var(--red)'
    case 'high': return 'var(--amber)'
    case 'medium': return '#e0a000'
    case 'low': return 'var(--cyan)'
    default: return 'var(--text-secondary)'
  }
}

const riskScoreColor = (score: number) => {
  if (score >= 60) return 'var(--red)'
  if (score >= 30) return 'var(--amber)'
  if (score >= 10) return '#e0a000'
  return 'var(--green)'
}

export default function SecurityPanel({ project, audit, setAudit }: Props) {
  const [loading, setLoading] = useState(false)

  const runAudit = async () => {
    setLoading(true)
    try {
      const result = await invoke<AuditResult>('run_security_audit', { project })
      setAudit(result)
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>Auditoría de seguridad</h2>
        <button className="btn btn-primary" onClick={runAudit} disabled={loading}>
          {loading ? <><span className="loading-spinner" /> Escaneando...</> : <><RefreshCw size={12} /> Auditar</>}
        </button>
      </div>

      {!audit && !loading && (
        <div className="analysis-empty">
          <Shield size={32} style={{ opacity: 0.2 }} />
          <p>Presiona "Auditar" para escanear vulnerabilidades, secrets y configuraciones</p>
        </div>
      )}

      {audit && (
        <>
          {/* Risk Score + Summary */}
          <div className="audit-summary">
            <div className="audit-risk-score">
              <div className="audit-risk-circle" style={{ borderColor: riskScoreColor(audit.summary.risk_score) }}>
                <span className="audit-risk-value" style={{ color: riskScoreColor(audit.summary.risk_score) }}>
                  {Math.round(audit.summary.risk_score)}
                </span>
                <span className="audit-risk-label">riesgo</span>
              </div>
            </div>
            <div className="audit-counts">
              {audit.summary.critical > 0 && (
                <div className="audit-count-item">
                  <span className="audit-count-dot" style={{ background: 'var(--red)' }} />
                  <span className="audit-count-value">{audit.summary.critical}</span>
                  <span className="audit-count-label">Critical</span>
                </div>
              )}
              {audit.summary.high > 0 && (
                <div className="audit-count-item">
                  <span className="audit-count-dot" style={{ background: 'var(--amber)' }} />
                  <span className="audit-count-value">{audit.summary.high}</span>
                  <span className="audit-count-label">High</span>
                </div>
              )}
              {audit.summary.medium > 0 && (
                <div className="audit-count-item">
                  <span className="audit-count-dot" style={{ background: '#e0a000' }} />
                  <span className="audit-count-value">{audit.summary.medium}</span>
                  <span className="audit-count-label">Medium</span>
                </div>
              )}
              {audit.summary.low > 0 && (
                <div className="audit-count-item">
                  <span className="audit-count-dot" style={{ background: 'var(--cyan)' }} />
                  <span className="audit-count-value">{audit.summary.low}</span>
                  <span className="audit-count-label">Low</span>
                </div>
              )}
              {audit.summary.info > 0 && (
                <div className="audit-count-item">
                  <span className="audit-count-dot" style={{ background: 'var(--text-secondary)' }} />
                  <span className="audit-count-value">{audit.summary.info}</span>
                  <span className="audit-count-label">Info</span>
                </div>
              )}
              {audit.summary.total === 0 && (
                <div className="audit-clean">
                  <ShieldCheck size={20} style={{ color: 'var(--green)' }} />
                  <span>Sin hallazgos de seguridad</span>
                </div>
              )}
            </div>
          </div>

          {/* Findings */}
          {audit.findings.length > 0 && (
            <div className="audit-findings">
              {audit.findings.map((f) => (
                <div key={f.id} className={`audit-finding severity-${f.severity}`}>
                  <div className="audit-finding-header">
                    <div className="audit-finding-title">
                      <span className="audit-finding-icon" style={{ color: severityColor(f.severity) }}>
                        {severityIcon(f.severity)}
                      </span>
                      <span className="audit-finding-name">{f.title}</span>
                    </div>
                    <div className="audit-finding-badges">
                      <span className={`severity-badge ${f.severity}`}>{f.severity}</span>
                      <span className="audit-category-badge">{f.category}</span>
                    </div>
                  </div>
                  <div className="audit-finding-desc">{f.description}</div>
                  {f.file_path && (
                    <div className="audit-finding-file">
                      {f.file_path}{f.line_number ? `:${f.line_number}` : ''}
                    </div>
                  )}
                  <div className="audit-finding-fix">
                    <span className="audit-fix-label">Fix:</span> {f.remediation}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  )
}
