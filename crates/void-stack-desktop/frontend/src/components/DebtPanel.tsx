import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { RefreshCw, ChevronRight, ChevronDown } from 'lucide-react'
import type { SnapshotDto, DebtComparisonDto, ServiceSnapshotDto } from '../types'
import CopyButton from './CopyButton'
import InfoTip from './InfoTip'

interface Props {
  project: string
  snapshots: SnapshotDto[]
  setSnapshots: (s: SnapshotDto[]) => void
  comparison: DebtComparisonDto | null
  setComparison: (c: DebtComparisonDto | null) => void
}

type ExpandKey = `${string}:${'god' | 'complex' | 'anti' | 'circular'}`

const trendIcon = (trend: string) => {
  switch (trend) {
    case 'Improving': return '\u2193'
    case 'Degrading': return '\u2191'
    default: return '\u2192'
  }
}

const trendColor = (trend: string) => {
  switch (trend) {
    case 'Improving': return 'var(--green)'
    case 'Degrading': return 'var(--red)'
    default: return 'var(--text-secondary)'
  }
}

const deltaColor = (delta: number, invert = false) => {
  if (delta === 0) return 'var(--text-secondary)'
  const isPositive = invert ? delta < 0 : delta > 0
  return isPositive ? 'var(--red)' : 'var(--green)'
}

const formatDelta = (delta: number) => {
  if (delta === 0) return '0'
  return delta > 0 ? `+${delta}` : `${delta}`
}

const formatDeltaFloat = (delta: number) => {
  if (delta === 0) return '0'
  return delta > 0 ? `+${delta.toFixed(1)}` : `${delta.toFixed(1)}`
}

const scoreColor = (value: number, thresholds: [number, number]) => {
  if (value <= thresholds[0]) return 'var(--green)'
  if (value <= thresholds[1]) return 'var(--yellow, #f0c040)'
  return 'var(--red)'
}

export default function DebtPanel({ project, snapshots, setSnapshots, comparison, setComparison }: Props) {
  const { t } = useTranslation()
  const [analyzing, setAnalyzing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [comparing, setComparing] = useState(false)
  const [current, setCurrent] = useState<SnapshotDto | null>(null)
  const [label, setLabel] = useState('')
  const [selectedA, setSelectedA] = useState<number | null>(null)
  const [selectedB, setSelectedB] = useState<number | null>(null)
  const [expanded, setExpanded] = useState<Set<ExpandKey>>(new Set())

  const toggleExpand = (key: ExpandKey) => {
    setExpanded(prev => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }

  const isExpanded = (key: ExpandKey) => expanded.has(key)

  const renderExpandableMetric = (
    svc: ServiceSnapshotDto,
    metricKey: 'god' | 'complex' | 'anti' | 'circular',
    label: React.ReactNode,
    value: number,
    color: string,
  ) => {
    const key: ExpandKey = `${svc.name}:${metricKey}`
    const hasDetail =
      (metricKey === 'god' && svc.god_classes_detail && svc.god_classes_detail.length > 0) ||
      (metricKey === 'complex' && svc.complex_functions_detail && svc.complex_functions_detail.length > 0) ||
      (metricKey === 'anti' && svc.anti_patterns_detail && svc.anti_patterns_detail.length > 0) ||
      (metricKey === 'circular' && svc.circular_deps_detail && svc.circular_deps_detail.length > 0)
    const open = isExpanded(key)

    return (
      <div className={`debt-metric-expandable ${open ? 'open' : ''}`}>
        <div
          className={`debt-metric-row ${hasDetail ? 'clickable' : ''}`}
          onClick={() => hasDetail && toggleExpand(key)}
        >
          <span className="debt-metric-label">
            {hasDetail && (
              <span className="debt-chevron">
                {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              </span>
            )}
            {label}
          </span>
          <span className="debt-metric-value" style={{ color }}>{value}</span>
        </div>
        {open && hasDetail && (
          <div className="debt-metric-detail">
            {metricKey === 'god' && svc.god_classes_detail?.map((g, i) => (
              <div key={i} className={`debt-detail-item severity-${g.severity.toLowerCase()}`}>
                <span className="debt-detail-file">{g.file}</span>
                <span className="debt-detail-meta">{g.loc} LOC · {g.functions} {t('debt.funcs')}</span>
              </div>
            ))}
            {metricKey === 'complex' && svc.complex_functions_detail?.map((f, i) => (
              <div key={i} className="debt-detail-item">
                <span className="debt-detail-file">{f.file}:{f.line}</span>
                <span className="debt-detail-fn">{f.name}()</span>
                <span className="debt-detail-score">{t('debt.cx')} {f.complexity}</span>
              </div>
            ))}
            {metricKey === 'anti' && svc.anti_patterns_detail?.map((a, i) => (
              <div key={i} className={`debt-detail-item severity-${a.severity.toLowerCase()}`}>
                <span className="debt-detail-kind">{a.kind}</span>
                <span className="debt-detail-desc">{a.description}</span>
                <span className="debt-detail-suggestion">{a.suggestion}</span>
              </div>
            ))}
            {metricKey === 'circular' && svc.circular_deps_detail?.map((c, i) => (
              <div key={i} className="debt-detail-item">
                <span className="debt-detail-cycle">{c.cycle.join(' → ')}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    )
  }

  const runAnalysis = async () => {
    setAnalyzing(true)
    try {
      const snap = await invoke<SnapshotDto>('analyze_debt', { project })
      setCurrent(snap)
    } catch (e) {
      console.error('debt analysis failed:', e)
    }
    try {
      const list = await invoke<SnapshotDto[]>('list_debt_snapshots', { project })
      setSnapshots(list)
    } catch (e) {
      console.error(e)
    }
    setAnalyzing(false)
  }

  // Auto-analyze on mount
  useEffect(() => {
    runAnalysis()
  }, [project])

  const saveSnapshot = async () => {
    setSaving(true)
    try {
      const labelVal = label.trim() || undefined
      const snap = await invoke<SnapshotDto>('save_debt_snapshot', { project, label: labelVal })
      setCurrent(snap)
      setLabel('')
      const list = await invoke<SnapshotDto[]>('list_debt_snapshots', { project })
      setSnapshots(list)
    } catch (e) {
      console.error(e)
    } finally {
      setSaving(false)
    }
  }

  const compareSnapshots = async () => {
    if (snapshots.length < 2) return
    setComparing(true)
    try {
      const args: { project: string; indexA?: number; indexB?: number } = { project }
      if (selectedA !== null) args.indexA = selectedA
      if (selectedB !== null) args.indexB = selectedB
      const result = await invoke<DebtComparisonDto>('compare_debt_snapshots', args)
      setComparison(result)
    } catch (e) {
      console.error(e)
    } finally {
      setComparing(false)
    }
  }

  const data = current

  const formatDebtText = () => {
    if (!data) return ''
    return data.services.map(svc =>
      `${svc.name} (${svc.pattern})\n  LOC: ${svc.total_loc} | Modules: ${svc.total_modules}\n  Anti-patterns: ${svc.anti_pattern_count} | Complexity: ${svc.avg_complexity.toFixed(1)} avg / ${svc.max_complexity} max\n  Complex functions: ${svc.complex_functions} | Coverage: ${svc.coverage_percent !== null ? svc.coverage_percent.toFixed(1) + '%' : '-'}\n  God Classes: ${svc.god_classes} | Circular Deps: ${svc.circular_deps}`
    ).join('\n\n')
  }

  return (
    <div className="panel">
      <div className="panel-header">
        <h2>{t('debt.title')}</h2>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          {data && <CopyButton text={formatDebtText()} />}
          <button className="btn btn-primary btn-sm" onClick={runAnalysis} disabled={analyzing}>
            {analyzing ? <><span className="loading-spinner" /> {t('common.loading')}</> : <><RefreshCw size={12} /> {t('analysis.reanalyze')}</>}
          </button>
        </div>
      </div>

      {/* ── Current metrics overview ── */}
      {data && data.services.length > 0 && (
        <div className="debt-current">
          <div className="debt-metrics-grid">
            {data.services.map(svc => (
              <div key={svc.name} className="debt-service-card">
                <div className="debt-service-card-header">{svc.name}</div>
                <div className="debt-service-card-pattern">{svc.pattern}</div>
                <div className="debt-metric-rows">
                  <div className="debt-metric-row">
                    <span className="debt-metric-label">{t('debt.loc')}</span>
                    <span className="debt-metric-value">{svc.total_loc.toLocaleString()}</span>
                  </div>
                  <div className="debt-metric-row">
                    <span className="debt-metric-label">{t('debt.modules')}</span>
                    <span className="debt-metric-value">{svc.total_modules}</span>
                  </div>
                  {renderExpandableMetric(
                    svc, 'anti',
                    <>{t('debt.antiPatterns')} <InfoTip text={t('tips.antiPattern')} /></>,
                    svc.anti_pattern_count,
                    scoreColor(svc.anti_pattern_count, [0, 3]),
                  )}
                  <div className="debt-metric-row">
                    <span className="debt-metric-label">{t('debt.complexity')} <InfoTip text={t('tips.complexity')} /></span>
                    <span className="debt-metric-value" style={{ color: scoreColor(svc.avg_complexity, [5, 10]) }}>
                      {svc.avg_complexity.toFixed(1)} avg / {svc.max_complexity} max
                    </span>
                  </div>
                  {renderExpandableMetric(
                    svc, 'complex',
                    <>{t('debt.complexFunctions')}</>,
                    svc.complex_functions,
                    scoreColor(svc.complex_functions, [0, 5]),
                  )}
                  <div className="debt-metric-row">
                    <span className="debt-metric-label">{t('debt.coverage')} <InfoTip text={t('tips.coverage')} /></span>
                    <span className="debt-metric-value">
                      {svc.coverage_percent !== null ? `${svc.coverage_percent.toFixed(1)}%` : '-'}
                    </span>
                  </div>
                  {renderExpandableMetric(
                    svc, 'god',
                    <>{t('debt.godClasses')} <InfoTip text={t('tips.godClass')} /></>,
                    svc.god_classes,
                    scoreColor(svc.god_classes, [0, 2]),
                  )}
                  {renderExpandableMetric(
                    svc, 'circular',
                    <>{t('debt.circularDeps')} <InfoTip text={t('tips.circularDep')} /></>,
                    svc.circular_deps,
                    scoreColor(svc.circular_deps, [0, 1]),
                  )}
                </div>
              </div>
            ))}
          </div>

          {/* Save snapshot inline */}
          <div className="debt-save-row">
            <input
              className="debt-label-input"
              type="text"
              value={label}
              onChange={e => setLabel(e.target.value)}
              placeholder={t('debt.labelPlaceholder')}
              onKeyDown={e => e.key === 'Enter' && saveSnapshot()}
            />
            <button
              className="btn btn-primary btn-sm"
              onClick={saveSnapshot}
              disabled={saving}
            >
              {saving ? <><span className="loading-spinner" /> {t('common.loading')}</> : t('debt.saveSnapshot')}
            </button>
          </div>
        </div>
      )}

      {!data && !analyzing && (
        <div className="analysis-empty">
          <p>{t('debt.noData')}</p>
        </div>
      )}

      {/* ── Snapshot history ── */}
      {snapshots.length > 0 && (
        <div className="debt-history-section">
          <div className="debt-history-header">
            <h3>{t('debt.history')}</h3>
            <button
              className="btn btn-sm"
              onClick={compareSnapshots}
              disabled={comparing || snapshots.length < 2}
              title={snapshots.length < 2 ? t('debt.needTwo') : ''}
            >
              {comparing ? <><span className="loading-spinner" /> {t('common.loading')}</> : t('debt.compare')}
            </button>
          </div>
          <div className="debt-snapshot-list">
            {snapshots.map((snap, idx) => ({ snap, idx })).reverse().map(({ snap, idx }) => {
              const isA = selectedA === idx
              const isB = selectedB === idx
              return (
                <div
                  key={idx}
                  className={`debt-snapshot-item ${isA ? 'selected-a' : ''} ${isB ? 'selected-b' : ''}`}
                  onClick={() => {
                    if (selectedA === null || selectedA === idx) {
                      setSelectedA(isA ? null : idx)
                    } else if (selectedB === null || selectedB === idx) {
                      setSelectedB(isB ? null : idx)
                    } else {
                      setSelectedA(idx)
                      setSelectedB(null)
                    }
                  }}
                >
                  <span className="debt-snapshot-time">{new Date(snap.timestamp).toLocaleString()}</span>
                  {snap.label && <span className="debt-snapshot-label">{snap.label}</span>}
                  <span className="debt-snapshot-services">{snap.services.length} svc</span>
                  {isA && <span className="debt-snapshot-marker marker-a">A</span>}
                  {isB && <span className="debt-snapshot-marker marker-b">B</span>}
                </div>
              )
            })}
          </div>
        </div>
      )}

      {/* ── Comparison view ── */}
      {comparison && (
        <div className="debt-comparison">
          <div className="debt-trend-header">
            <span
              className="debt-trend-icon"
              style={{ color: trendColor(comparison.overall_trend) }}
            >
              {trendIcon(comparison.overall_trend)}
            </span>
            <span className="debt-trend-text">
              {t('debt.trend')}: {t(`debt.${comparison.overall_trend.toLowerCase()}`)}
            </span>
            <span className="debt-period">
              {new Date(comparison.previous).toLocaleDateString()} &rarr; {new Date(comparison.current).toLocaleDateString()}
            </span>
          </div>

          <div className="debt-table-container">
            <table className="debt-table">
              <thead>
                <tr>
                  <th>{t('debt.service')}</th>
                  <th>{t('debt.loc')}</th>
                  <th>{t('debt.antiPatterns')}</th>
                  <th>{t('debt.complexity')}</th>
                  <th>{t('debt.coverage')}</th>
                  <th>{t('debt.godClasses')}</th>
                  <th>{t('debt.circularDeps')}</th>
                  <th>{t('debt.trend')}</th>
                </tr>
              </thead>
              <tbody>
                {comparison.services.map(svc => (
                  <tr key={svc.name}>
                    <td className="debt-service-name">{svc.name}</td>
                    <td style={{ color: deltaColor(svc.loc_delta) }}>
                      {formatDelta(svc.loc_delta)}
                    </td>
                    <td style={{ color: deltaColor(svc.antipattern_delta) }}>
                      {formatDelta(svc.antipattern_delta)}
                    </td>
                    <td style={{ color: deltaColor(svc.complexity_delta) }}>
                      {formatDeltaFloat(svc.complexity_delta)}
                    </td>
                    <td style={{ color: svc.coverage_delta !== null ? deltaColor(svc.coverage_delta, true) : 'var(--text-secondary)' }}>
                      {svc.coverage_delta !== null ? `${formatDeltaFloat(svc.coverage_delta)}%` : '-'}
                    </td>
                    <td style={{ color: deltaColor(svc.god_class_delta) }}>
                      {formatDelta(svc.god_class_delta)}
                    </td>
                    <td style={{ color: deltaColor(svc.circular_dep_delta) }}>
                      {formatDelta(svc.circular_dep_delta)}
                    </td>
                    <td>
                      <span style={{ color: trendColor(svc.trend) }}>
                        {trendIcon(svc.trend)}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}
