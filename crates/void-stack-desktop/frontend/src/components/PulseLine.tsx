import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { useProjectSignal } from '../projectSignals'

interface ReviewPayload { findings_on_changed_lines: number; suppressed: number; uncovered: number }
interface TestSuggestions { suggested: unknown[]; uncovered: unknown[]; changed_symbols_total: number }
interface DeadCodeReport { candidates: unknown[]; total_found: number }

export type PulseTarget = 'review' | 'tests' | 'deadcode' | 'security'

interface Props {
  project: string | null
  /** Risk score from the cached audit state (null if no audit run yet). */
  riskScore: number | null
  onNavigate: (target: PulseTarget) => void
}

type Cell<T> = { state: 'loading' | 'ok' | 'err'; data?: T }
interface PulseData {
  review: Cell<ReviewPayload>
  tests: Cell<TestSuggestions>
  dead: Cell<DeadCodeReport>
  ts: number
}

// Per-project cache so flipping projects and back doesn't re-run the
// 2–4 s analyses. A commit (git signal) invalidates it; otherwise it is
// refreshed when older than STALE_MS (uncommitted edits change the diff).
const cache = new Map<string, PulseData>()
const STALE_MS = 10 * 60 * 1000
/** Wait this long after a project switch before launching the analyses, so
 *  flipping through projects doesn't start three heavy jobs per stop. */
const SETTLE_MS = 1200
const loading = <T,>(): Cell<T> => ({ state: 'loading' })

export default function PulseLine({ project, riskScore, onNavigate }: Props) {
  const { t } = useTranslation()
  const [data, setData] = useState<PulseData | null>(null)
  // A commit invalidates the cached analyses for this project.
  const gitSignal = useProjectSignal('git')

  useEffect(() => {
    if (project && gitSignal > 0) cache.delete(project)
    const cached = project ? cache.get(project) : undefined
    const fresh = !!cached && Date.now() - cached.ts < STALE_MS

    let cancelled = false
    // Seed all three chips in their loading state; each resolves
    // independently so a slow review never holds back tests/dead-code.
    const seed: PulseData = { review: loading(), tests: loading(), dead: loading(), ts: Date.now() }

    const patch = (key: keyof Omit<PulseData, 'ts'>, cell: Cell<unknown>) => {
      if (cancelled || !project) return
      setData(prev => {
        const base = prev ?? seed
        const next = { ...base, [key]: cell }
        cache.set(project, next)
        return next
      })
    }

    // State updates run from timers (not synchronously in the effect body).
    const show = setTimeout(() => {
      if (cancelled) return
      setData(!project ? null : fresh ? cached! : seed)
    }, 0)

    const launch = !project || fresh ? undefined : setTimeout(() => {
      if (cancelled) return
      const tasks: Promise<void>[] = [
        invoke<ReviewPayload>('review_diff_cmd', { project })
          .then(d => patch('review', { state: 'ok', data: d }))
          .catch(() => patch('review', { state: 'err' })),
        invoke<TestSuggestions>('suggest_tests_cmd', { project })
          .then(d => patch('tests', { state: 'ok', data: d }))
          .catch(() => patch('tests', { state: 'err' })),
        invoke<DeadCodeReport>('find_dead_code_cmd', { project })
          .then(d => patch('dead', { state: 'ok', data: d }))
          .catch(() => patch('dead', { state: 'err' })),
      ]
      // allSettled: one failure never aborts the others.
      Promise.allSettled(tasks)
    }, SETTLE_MS)

    return () => {
      cancelled = true
      clearTimeout(show)
      if (launch) clearTimeout(launch)
    }
  }, [project, gitSignal])

  if (!project) return null

  const skel = <span className="vs-skel" aria-hidden="true" />

  const reviewChip = () => {
    const c = data?.review
    if (!c || c.state === 'loading') return skel
    if (c.state === 'err') return <span title={t('pulse.graphNeeded')}>{t('pulse.reviewLabel')} · —</span>
    const n = c.data!.findings_on_changed_lines
    return (
      <button onClick={() => onNavigate('review')}>
        {t('pulse.reviewLabel')} · <span className={n > 0 ? 'n-warn' : 'n-ok'}>{t('pulse.findings', { count: n })}</span>
      </button>
    )
  }

  const testsChip = () => {
    const c = data?.tests
    if (!c || c.state === 'loading') return skel
    if (c.state === 'err') return <span title={t('pulse.graphNeeded')}>{t('pulse.testsLabel')} · —</span>
    const sug = c.data!.suggested.length
    const unc = c.data!.uncovered.length
    return (
      <button onClick={() => onNavigate('tests')}>
        {t('pulse.testsLabel')} · <span className="n-ok">{t('pulse.suggested', { count: sug })}</span>
        {unc > 0 && <>, <span className="n-err">{t('pulse.uncovered', { count: unc })}</span></>}
      </button>
    )
  }

  const deadChip = () => {
    const c = data?.dead
    if (!c || c.state === 'loading') return skel
    if (c.state === 'err') return <span title={t('pulse.graphNeeded')}>{t('pulse.deadLabel')} · —</span>
    return (
      <button onClick={() => onNavigate('deadcode')}>
        {t('pulse.deadLabel')} · {c.data!.total_found}
      </button>
    )
  }

  const riskChip = () => {
    if (riskScore === null) return <span>{t('pulse.riskLabel')} · —</span>
    const cls = riskScore < 20 ? 'n-ok' : riskScore < 50 ? 'n-warn' : 'n-err'
    return (
      <button onClick={() => onNavigate('security')}>
        {t('pulse.riskLabel')} <span className={cls}>{Math.round(riskScore)}/100</span>
      </button>
    )
  }

  return (
    <div className="vs-pulse" aria-label={t('pulse.label')}>
      {reviewChip()}<span className="vs-sep">·</span>
      {testsChip()}<span className="vs-sep">·</span>
      {deadChip()}<span className="vs-sep">·</span>
      {riskChip()}
    </div>
  )
}
