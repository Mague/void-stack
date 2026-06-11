import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Terminal, ChevronUp } from 'lucide-react'
import LogViewer from './LogViewer'

interface Props {
  project: string
  services: string[]
  activeService: string | null
  onSelectService: (name: string) => void
}

/**
 * Collapsible log drawer for the Run zone, anchored at the bottom of the
 * service grid. Closed by default; opening reveals the embedded LogViewer
 * (structured lines, level colors, follow mode) with a smooth max-height
 * transition.
 */
export default function LogDrawer({ project, services, activeService, onSelectService }: Props) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const current = activeService || services[0] || null

  return (
    <div className={`vs-drawer ${open ? 'open' : ''}`}>
      <button
        className="vs-drawer-bar"
        aria-expanded={open}
        onClick={() => setOpen(o => !o)}
      >
        <Terminal size={14} />
        {t('tabs.logs')}{current ? ` · ${current}` : ''}
        <ChevronUp className="vs-chev" size={14} />
      </button>
      <div className="vs-drawer-body">
        {open && current && (
          <LogViewer
            project={project}
            services={services}
            activeService={current}
            onSelectService={onSelectService}
            embedded
          />
        )}
      </div>
    </div>
  )
}
