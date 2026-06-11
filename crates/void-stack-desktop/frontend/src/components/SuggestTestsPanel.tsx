import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { FlaskConical, RefreshCw } from 'lucide-react'

interface TestSuggestion {
  name: string
  file: string
  line: number
  hops: number
  covers: number
  language: string
}
interface ChangedSymbol { name: string; file: string; line: number }
interface TestSuggestions {
  suggested: TestSuggestion[]
  uncovered: ChangedSymbol[]
  commands: string[]
  changed_symbols_total: number
}

interface Props {
  project: string
  onBuildGraph: () => Promise<void>
}

export default function SuggestTestsPanel({ project, onBuildGraph }: Props) {
  const { t } = useTranslation()
  const [result, setResult] = useState<TestSuggestions | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const run = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setResult(await invoke<TestSuggestions>('suggest_tests_cmd', { project }))
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [project])

  useEffect(() => { run() }, [project]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="vs-intel-section" style={{ overflow: 'auto' }}>
      <div className="vs-intel-actions">
        <button className="vs-btn" onClick={run} disabled={loading}>
          {loading ? <RefreshCw size={13} className="vs-spin" /> : <FlaskConical size={13} />}
          {loading ? t('common.loading') : t('intel.runSuggestTests')}
        </button>
      </div>

      {error && (
        <div className="vs-empty">
          <span>{t('intel.graphNeeded')}</span>
          <button className="vs-btn" onClick={async () => { await onBuildGraph(); run() }}>{t('intel.buildGraph')}</button>
        </div>
      )}

      {result && !error && (
        <>
          <h2>{t('intel.testsForSymbols', { count: result.changed_symbols_total })}</h2>
          {result.suggested.map((s, i) => (
            <div className="vs-row" key={i}>
              <span className="sev hop">hop {s.hops}</span>
              <span className="what">{s.name} — {t('intel.coversN', { count: s.covers })}</span>
              <span className="where">{s.file}:{s.line}</span>
            </div>
          ))}
          {result.suggested.length === 0 && <div className="vs-row"><span className="what">{t('intel.noTests')}</span></div>}

          {result.uncovered.length > 0 && (
            <>
              <h2 style={{ marginTop: 14 }}>{t('intel.uncovered', { count: result.uncovered.length })}</h2>
              {result.uncovered.slice(0, 15).map((u, i) => (
                <div className="vs-row" key={i}>
                  <span className="sev medium">⚠</span>
                  <span className="what">{u.name}</span>
                  <span className="where">{u.file}:{u.line}</span>
                </div>
              ))}
            </>
          )}

          {result.commands.length > 0 && (
            <>
              <h2 style={{ marginTop: 14 }}>{t('intel.runCommands')}</h2>
              <pre className="vs-cmd-block">{result.commands.join('\n')}</pre>
            </>
          )}
        </>
      )}
    </div>
  )
}
