import { useState } from 'react'
import { Copy, Check } from 'lucide-react'
import { useTranslation } from 'react-i18next'

interface Props {
  text: string
  className?: string
}

export default function CopyButton({ text, className = '' }: Props) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <button
      className={`btn btn-sm btn-copy ${copied ? 'copied' : ''} ${className}`}
      onClick={handleCopy}
      title={t('common.copy')}
    >
      {copied ? <><Check size={10} /> {t('common.copied')}</> : <><Copy size={10} /> {t('common.copy')}</>}
    </button>
  )
}
