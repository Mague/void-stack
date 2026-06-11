import { useTranslation } from 'react-i18next'

export type Zone = 'run' | 'intel' | 'map' | 'project'

interface Props {
  active: Zone
  onSelect: (zone: Zone) => void
}

const ICONS: Record<Zone, React.ReactNode> = {
  run: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M7 4v16l13-8z" /></svg>,
  intel: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><circle cx="12" cy="12" r="3" /><path d="M12 2v4M12 18v4M2 12h4M18 12h4M5 5l2.5 2.5M16.5 16.5L19 19M19 5l-2.5 2.5M7.5 16.5L5 19" /></svg>,
  map: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><circle cx="6" cy="6" r="2.5" /><circle cx="18" cy="6" r="2.5" /><circle cx="12" cy="18" r="2.5" /><path d="M8 7.5l3 8M16 7.5l-3 8M8.5 6h7" /></svg>,
  project: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z" /></svg>,
}

export default function Rail({ active, onSelect }: Props) {
  const { t } = useTranslation()
  const zones: Zone[] = ['run', 'intel', 'map', 'project']

  return (
    <nav className="vs-rail" aria-label={t('zones.label')}>
      {zones.map(z => (
        <button
          key={z}
          className={`vs-zone ${active === z ? 'active' : ''}`}
          onClick={() => onSelect(z)}
          aria-label={t(`zones.${z}`)}
          aria-current={active === z ? 'page' : undefined}
        >
          {ICONS[z]}
          <span className="vs-tip" role="tooltip">{t(`zones.${z}`)}</span>
        </button>
      ))}
      <span className="vs-spacer" />
    </nav>
  )
}
