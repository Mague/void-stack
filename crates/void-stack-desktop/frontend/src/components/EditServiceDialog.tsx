import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { X } from 'lucide-react'
import type { ServiceInfo } from '../types'

const TARGETS = ['windows', 'macos', 'wsl', 'docker', 'ssh'] as const

interface Props {
  project: string
  service: ServiceInfo
  /** The service is running: changes apply on its next start. */
  running: boolean
  onClose: () => void
}

/** Lines of a textarea as a list, dropping blanks. */
function lines(text: string): string[] {
  return text.split('\n').map(l => l.trim()).filter(Boolean)
}

/**
 * Edit a registered service in place: fix an auto-detected command that
 * doesn't work, its working directory, target or Docker options, or rename
 * it — instead of removing and re-adding it.
 */
export default function EditServiceDialog({ project, service, running, onClose }: Props) {
  const { t } = useTranslation()
  const [name, setName] = useState(service.name)
  const [command, setCommand] = useState(service.command)
  const [dir, setDir] = useState(service.working_dir ?? '')
  const [target, setTarget] = useState(service.target.toLowerCase())
  const [ports, setPorts] = useState((service.docker_ports ?? []).join('\n'))
  const [volumes, setVolumes] = useState((service.docker_volumes ?? []).join('\n'))
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  const save = async () => {
    setSaving(true)
    setError(null)
    try {
      await invoke('update_service_cmd', {
        project,
        service: service.name,
        changes: {
          newName: name.trim() !== service.name ? name.trim() : undefined,
          command: command.trim(),
          workingDir: dir.trim(),
          target,
          dockerPorts: target === 'docker' ? lines(ports) : undefined,
          dockerVolumes: target === 'docker' ? lines(volumes) : undefined,
        },
      })
      window.dispatchEvent(new CustomEvent('void-refresh-projects'))
      onClose()
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="vs-veil" onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}>
      <div
        className="vs-palette vs-edit-service"
        role="dialog"
        aria-modal="true"
        aria-label={t('services.editTitle', { name: service.name })}
        onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      >
        <div className="vs-edit-service-head">
          <h3>{t('services.editTitle', { name: service.name })}</h3>
          <button className="vs-card-act" onClick={onClose} aria-label={t('common.cancel')}>
            <X size={13} />
          </button>
        </div>
        <div className="add-service-form">
          <label>{t('services.serviceName')}</label>
          <input value={name} onChange={e => setName(e.target.value)} />
          <label>{t('services.serviceCommand')}</label>
          <input value={command} onChange={e => setCommand(e.target.value)} autoFocus spellCheck={false} />
          <label>{t('services.serviceDir')}</label>
          <input value={dir} onChange={e => setDir(e.target.value)} spellCheck={false} />
          <label>{t('services.target')}</label>
          <div className="add-service-row">
            {TARGETS.map(tg => (
              <button
                key={tg}
                className={`btn btn-sm btn-toggle ${target === tg ? 'active' : ''}`}
                onClick={() => setTarget(tg)}
              >
                {tg}
              </button>
            ))}
          </div>
          {target === 'docker' && (
            <>
              <label>{t('services.ports')}</label>
              <textarea rows={2} value={ports} onChange={e => setPorts(e.target.value)} placeholder="8080:80" />
              <label>{t('services.volumes')}</label>
              <textarea rows={2} value={volumes} onChange={e => setVolumes(e.target.value)} placeholder="./data:/app/data" />
            </>
          )}
          {running && <div className="vs-edit-service-note">{t('services.restartToApply')}</div>}
          {error && <div className="add-service-error">{error}</div>}
          <div className="add-service-actions">
            <button className="btn btn-primary btn-sm" onClick={save} disabled={saving || !name.trim() || !command.trim()}>
              {t('services.saveChanges')}
            </button>
            <button className="btn btn-sm" onClick={onClose}>{t('common.cancel')}</button>
          </div>
        </div>
      </div>
    </div>
  )
}
