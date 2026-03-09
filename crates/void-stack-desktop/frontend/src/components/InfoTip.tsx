import { useState, useRef, useCallback } from 'react'
import { Info } from 'lucide-react'

interface Props {
  text: string
  size?: number
}

export default function InfoTip({ text, size = 12 }: Props) {
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null)
  const iconRef = useRef<HTMLSpanElement>(null)

  const show = useCallback(() => {
    if (!iconRef.current) return
    const rect = iconRef.current.getBoundingClientRect()
    setPos({ top: rect.bottom + 6, left: Math.min(rect.left, window.innerWidth - 290) })
  }, [])

  const hide = useCallback(() => setPos(null), [])

  return (
    <span className="info-tip" ref={iconRef} onMouseEnter={show} onMouseLeave={hide}>
      <Info size={size} />
      {pos && (
        <span
          className="info-tip-content"
          style={{ position: 'fixed', top: pos.top, left: pos.left }}
        >
          {text}
        </span>
      )}
    </span>
  )
}
