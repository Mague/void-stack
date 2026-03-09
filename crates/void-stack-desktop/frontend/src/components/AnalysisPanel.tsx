import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import type { AnalysisResultDto, BpFindingDto } from '../types'
import { Microscope, AlertTriangle, Cpu, Shield, Zap } from 'lucide-react'

interface Props {
  project: string
  analysis: AnalysisResultDto | null
  setAnalysis: (a: AnalysisResultDto | null) => void
}

export default function AnalysisPanel({ project, analysis, setAnalysis }: Props) {
  const { t } = useTranslation()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [bpFilter, setBpFilter] = useState<'all' | 'Important' | 'Warning' | 'Suggestion'>('all')
  const [bpExpanded, setBpExpanded] = useState(true)

  const analyze = async (withBp = false) => {
    setLoading(true)
    setError(null)
    try {
      const result = await invoke<AnalysisResultDto>('analyze_project_cmd', {
        project,
        bestPractices: withBp || undefined,
      })
      setAnalysis(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  if (!analysis && !loading) {
    return (
      <div className="panel">
        <div className="panel-header">
          <h2>{t('analysis.title')}</h2>
          <div style={{ display: 'flex', gap: 8 }}>
            <button className="btn btn-primary" onClick={() => analyze(false)} disabled={loading}>
              <Microscope size={12} /> {t('analysis.analyze')}
            </button>
            <button className="btn btn-primary" onClick={() => analyze(true)} disabled={loading}>
              <Zap size={12} /> {t('analysis.bestPractices')}
            </button>
          </div>
        </div>
        {error && (
          <div className="warnings" style={{ marginBottom: 16 }}>
            <h3>{t('common.error')}</h3>
            <p style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{error}</p>
          </div>
        )}
        <div className="analysis-empty">
          <Microscope size={32} style={{ opacity: 0.2 }} />
          <p>{t('analysis.emptyPrompt')}</p>
        </div>
      </div>
    )
  }

  if (loading) {
    return (
      <div className="panel">
        <div className="panel-header">
          <h2>{t('analysis.title')}</h2>
        </div>
        <div className="analysis-empty">
          <span className="loading-spinner" style={{ width: 24, height: 24 }} />
          <p>{t('analysis.analyzing')}</p>
        </div>
      </div>
    )
  }

  const a = analysis!
  const maxLayerCount = Math.max(...a.layers.map(l => l.count), 1)
  const bp = a.best_practices

  const filteredBpFindings = bp?.findings.filter(f =>
    bpFilter === 'all' || f.severity === bpFilter
  ) || []

  const toolColors: Record<string, string> = {
    'react-doctor': '#61dafb',
    'ruff': '#d4aa00',
    'clippy': '#dea584',
    'golangci-lint': '#00add8',
    'dart-analyze': '#0175c2',
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('analysis.title')}</h2>
        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn btn-primary" onClick={() => analyze(false)}>
            <Microscope size={12} /> {t('analysis.reanalyze')}
          </button>
          <button className="btn btn-primary" onClick={() => analyze(true)}>
            <Zap size={12} /> {t('analysis.bestPractices')}
          </button>
        </div>
      </div>

      {/* Overview stats */}
      <div className="analysis-grid">
        {/* Architecture pattern */}
        <div className="analysis-card">
          <div className="analysis-card-title">{t('analysis.pattern')}</div>
          <div className="analysis-stat">{a.pattern}</div>
          <div className="analysis-stat-label">{t('analysis.confidence')}: {(a.confidence * 100).toFixed(0)}%</div>
          <div className="confidence-bar">
            <div className="confidence-fill" style={{ width: `${a.confidence * 100}%` }} />
          </div>
        </div>

        {/* Stats */}
        <div className="analysis-card">
          <div className="analysis-card-title">{t('analysis.metrics')}</div>
          <div className="stats-row">
            <div className="stat-inline">
              <div className="analysis-stat">{a.module_count}</div>
              <div className="analysis-stat-label">{t('analysis.modules')}</div>
            </div>
            <div className="stat-inline">
              <div className="analysis-stat">{a.total_loc.toLocaleString()}</div>
              <div className="analysis-stat-label">{t('analysis.loc')}</div>
            </div>
            <div className="stat-inline">
              <div className="analysis-stat">{a.anti_patterns.length}</div>
              <div className="analysis-stat-label">{t('analysis.antiPatterns')}</div>
            </div>
          </div>
        </div>

        {/* Layer distribution */}
        <div className="analysis-card">
          <div className="analysis-card-title">{t('analysis.layerDistribution')}</div>
          <div className="layer-bars">
            {a.layers.map(l => (
              <div key={l.name} className="layer-row">
                <span className="layer-name">{l.name}</span>
                <div className="layer-bar-track">
                  <div
                    className="layer-bar-fill"
                    style={{ width: `${(l.count / maxLayerCount) * 100}%` }}
                  />
                </div>
                <span className="layer-count">{l.count}</span>
              </div>
            ))}
          </div>
        </div>

        {/* Coverage */}
        <div className="analysis-card">
          <div className="analysis-card-title">
            <Shield size={12} style={{ display: 'inline', marginRight: 6, opacity: 0.5 }} />
            {t('analysis.coverage')}
          </div>
          {a.coverage ? (
            <>
              <div className="analysis-stat">{a.coverage.percent.toFixed(1)}%</div>
              <div className="analysis-stat-label">{a.coverage.tool}</div>
              <div className="coverage-bar-track">
                <div
                  className={`coverage-bar-fill ${a.coverage.percent >= 70 ? 'good' : a.coverage.percent >= 40 ? 'ok' : 'bad'}`}
                  style={{ width: `${Math.min(a.coverage.percent, 100)}%` }}
                />
              </div>
              <div className="coverage-stats">
                <span>{a.coverage.covered.toLocaleString()} {t('analysis.covered')}</span>
                <span>{a.coverage.total.toLocaleString()} {t('common.total')}</span>
              </div>
            </>
          ) : (
            <div style={{ color: 'var(--text-muted)', fontFamily: "'JetBrains Mono', monospace", fontSize: 11, marginTop: 8 }}>
              <div style={{ marginBottom: 6 }}>{t('analysis.noCoverage')}</div>
              {a.coverage_hint && (
                <div style={{ fontSize: 10, opacity: 0.7, lineHeight: 1.6 }}>
                  {a.coverage_hint.split('\n').map((line, i) => (
                    <div key={i}>{line}</div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Anti-patterns */}
        {a.anti_patterns.length > 0 && (
          <div className="analysis-card full-width">
            <div className="analysis-card-title">
              <AlertTriangle size={12} style={{ display: 'inline', marginRight: 6, color: 'var(--amber)' }} />
              {t('analysis.antiPatternsDetected', { count: a.anti_patterns.length })}
            </div>
            <div className="antipattern-list">
              {a.anti_patterns.map((ap, i) => (
                <div key={i} className={`antipattern-item severity-${ap.severity.toLowerCase()}`}>
                  <div className="antipattern-header">
                    <span className="antipattern-kind">{ap.kind}</span>
                    <span className={`severity-badge ${ap.severity.toLowerCase()}`}>{ap.severity}</span>
                  </div>
                  <div className="antipattern-desc">{ap.description}</div>
                  {ap.affected.length > 0 && (
                    <div style={{ marginTop: 4, fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: 'var(--text-muted)' }}>
                      {ap.affected.join(', ')}
                    </div>
                  )}
                  <div className="antipattern-suggestion">{ap.suggestion}</div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Complexity */}
        {a.top_complex.length > 0 && (
          <div className="analysis-card full-width">
            <div className="analysis-card-title">
              <Cpu size={12} style={{ display: 'inline', marginRight: 6, opacity: 0.5 }} />
              {t('analysis.topComplex', { count: a.top_complex.length })}
            </div>
            <table className="complexity-table">
              <thead>
                <tr>
                  <th>{t('analysis.colFile')}</th>
                  <th>{t('analysis.colFunction')}</th>
                  <th>{t('analysis.colLine')}</th>
                  <th>{t('analysis.colComplexity')}</th>
                </tr>
              </thead>
              <tbody>
                {a.top_complex.map((f, i) => (
                  <tr key={i}>
                    <td style={{ color: 'var(--text-secondary)', maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {f.file}
                    </td>
                    <td style={{ color: 'var(--text-bright)' }}>{f.name}</td>
                    <td>{f.line}</td>
                    <td className={f.complexity >= 15 ? 'complexity-high' : f.complexity >= 10 ? 'complexity-medium' : 'complexity-low'}>
                      {f.complexity}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Best Practices */}
        {bp && (
          <div className="analysis-card full-width">
            <div
              className="analysis-card-title"
              style={{ cursor: 'pointer', userSelect: 'none' }}
              onClick={() => setBpExpanded(!bpExpanded)}
            >
              <Zap size={12} style={{ display: 'inline', marginRight: 6, color: 'var(--accent)' }} />
              {t('analysis.bpTitle')}
              <span style={{ marginLeft: 8, opacity: 0.5, fontSize: 10 }}>{bpExpanded ? '▼' : '▶'}</span>
            </div>

            {bpExpanded && (
              <>
                {/* Score circle + tool chips */}
                <div style={{ display: 'flex', alignItems: 'center', gap: 20, marginBottom: 16 }}>
                  <div className="bp-score-circle" data-score={bp.overall_score >= 70 ? 'good' : bp.overall_score >= 50 ? 'ok' : 'bad'}>
                    <span className="bp-score-value">{bp.overall_score.toFixed(0)}</span>
                    <span className="bp-score-label">{t('analysis.bpScore')}</span>
                  </div>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                    {bp.tool_scores.map(ts => (
                      <span
                        key={ts.tool}
                        className="bp-tool-chip"
                        style={{ borderColor: toolColors[ts.tool] || 'var(--border)' }}
                      >
                        <span style={{ color: toolColors[ts.tool] || 'var(--text-secondary)' }}>{ts.tool}</span>
                        {' '}{ts.score.toFixed(0)}
                        {ts.native_score !== null && (
                          <span style={{ opacity: 0.5, fontSize: 10 }}> ({t('analysis.bpNative')}: {ts.native_score.toFixed(0)})</span>
                        )}
                      </span>
                    ))}
                  </div>
                </div>

                {/* Filter buttons */}
                <div style={{ display: 'flex', gap: 6, marginBottom: 12 }}>
                  {(['all', 'Important', 'Warning', 'Suggestion'] as const).map(f => (
                    <button
                      key={f}
                      className={`btn btn-sm ${bpFilter === f ? 'btn-primary' : ''}`}
                      onClick={() => setBpFilter(f)}
                      style={{ fontSize: 11, padding: '3px 10px' }}
                    >
                      {f === 'all'
                        ? `${t('common.all')} (${bp.findings.length})`
                        : `${t(`analysis.bp${f}`)} (${bp.findings.filter(x => x.severity === f).length})`
                      }
                    </button>
                  ))}
                </div>

                {/* Findings */}
                {filteredBpFindings.length === 0 ? (
                  <div style={{ color: 'var(--text-muted)', fontSize: 12, padding: 12 }}>
                    {bp.findings.length === 0
                      ? t('analysis.bpAllPassed', { count: bp.tools_used.length })
                      : t('analysis.bpNoMatch')
                    }
                  </div>
                ) : (
                  <div className="bp-findings-list">
                    {filteredBpFindings.map((f, i) => (
                      <BpFindingCard key={i} finding={f} toolColors={toolColors} />
                    ))}
                  </div>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function BpFindingCard({ finding: f, toolColors }: { finding: BpFindingDto; toolColors: Record<string, string> }) {
  const severityIcon = f.severity === 'Important' ? '🔴' : f.severity === 'Warning' ? '⚠️' : '💡'
  const toolColor = toolColors[f.tool] || 'var(--text-secondary)'

  return (
    <div className="bp-finding-card">
      <div className="bp-finding-header">
        <span style={{ fontSize: 11 }}>{severityIcon}</span>
        <span className="bp-tool-badge" style={{ background: toolColor + '22', color: toolColor, borderColor: toolColor + '44' }}>
          {f.tool}
        </span>
        <code className="bp-rule-id">{f.rule_id}</code>
        {f.file && (
          <span className="bp-file-loc">
            {f.file}{f.line ? `:${f.line}` : ''}
          </span>
        )}
      </div>
      <div className="bp-finding-message">{f.message}</div>
      {f.fix_hint && (
        <div className="bp-finding-fix">Fix: {f.fix_hint}</div>
      )}
    </div>
  )
}
