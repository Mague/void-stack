import { useState, useEffect, useRef, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

export type CommandGlyph = 'fn' | 'cls' | 'action' | 'service'

export interface CommandItem {
  group: string
  label: string
  hint?: string
  glyph: CommandGlyph
  run: () => void
}

interface Props {
  open: boolean
  onClose: () => void
  commands: CommandItem[]
  /** Fired on Enter when the query has no local match. */
  onSearchFallback?: (query: string) => void
}

const GLYPHS: Record<CommandGlyph, React.ReactNode> = {
  fn: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M8 4l-5 8 5 8M16 4l5 8-5 8" /></svg>,
  cls: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="4" y="4" width="16" height="16" rx="3" /><path d="M4 10h16" /></svg>,
  action: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M13 3L4 14h6l-1 7 9-11h-6z" /></svg>,
  service: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="5" width="18" height="6" rx="2" /><rect x="3" y="13" width="18" height="6" rx="2" /></svg>,
}

export default function CommandPalette({ open, onClose, commands, onSearchFallback }: Props) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [sel, setSel] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setQuery('')
      setSel(0)
      // Focus after the pop-in paint.
      requestAnimationFrame(() => inputRef.current?.focus())
    }
  }, [open])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return commands
    return commands.filter(c =>
      c.label.toLowerCase().includes(q) || (c.hint?.toLowerCase().includes(q) ?? false)
    )
  }, [commands, query])

  useEffect(() => { if (sel >= filtered.length) setSel(0) }, [filtered, sel])

  if (!open) return null

  const hasFallback = filtered.length === 0 && query.trim().length > 0

  const execute = (index: number) => {
    if (hasFallback) {
      onSearchFallback?.(query.trim())
      onClose()
      return
    }
    const c = filtered[index]
    if (c) { onClose(); c.run() }
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') { e.preventDefault(); onClose() }
    else if (e.key === 'ArrowDown') { e.preventDefault(); setSel(s => Math.min(s + 1, filtered.length - 1)) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSel(s => Math.max(s - 1, 0)) }
    else if (e.key === 'Enter') { e.preventDefault(); execute(sel) }
  }

  // Group while preserving catalog order.
  const groups: { group: string; items: { item: CommandItem; index: number }[] }[] = []
  filtered.forEach((item, index) => {
    let g = groups.find(x => x.group === item.group)
    if (!g) { g = { group: item.group, items: [] }; groups.push(g) }
    g.items.push({ item, index })
  })

  return (
    <div className="vs-veil" onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="vs-palette" role="dialog" aria-label={t('palette.label')} aria-modal="true">
        <input
          ref={inputRef}
          value={query}
          onChange={e => { setQuery(e.target.value); setSel(0) }}
          onKeyDown={onKeyDown}
          placeholder={t('palette.placeholder')}
          autoComplete="off"
          spellCheck={false}
        />
        <div className="vs-results">
          {hasFallback ? (
            <div className="vs-nores">↵ {t('palette.searchInCodebase')}</div>
          ) : filtered.length === 0 ? (
            <div className="vs-nores">{t('palette.empty')}</div>
          ) : (
            groups.map(g => (
              <div key={g.group}>
                <div className="vs-group-label">{g.group}</div>
                {g.items.map(({ item, index }) => (
                  <button
                    key={index}
                    className={`vs-result ${index === sel ? 'sel' : ''}`}
                    onMouseEnter={() => setSel(index)}
                    onClick={() => execute(index)}
                  >
                    <span className="vs-glyph">{GLYPHS[item.glyph]}</span>
                    {item.label}
                    {item.hint && <span className="vs-hint">{item.hint}</span>}
                  </button>
                ))}
              </div>
            ))
          )}
        </div>
        <div className="vs-palette-foot">
          <span><kbd>↑↓</kbd>{t('palette.navigate')}</span>
          <span><kbd>↵</kbd>{t('palette.run')}</span>
          <span><kbd>esc</kbd>{t('palette.close')}</span>
        </div>
      </div>
    </div>
  )
}
