interface Props {
  tech: string
  size?: number
}

export default function TechIcon({ tech, size = 16 }: Props) {
  const s = size
  const t = tech.toLowerCase()

  if (t === 'python') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M11.9 2C6.5 2 6.9 4.3 6.9 4.3l.01 2.4h5.1v.7H5.1S2 7 2 12.3s2.7 5.1 2.7 5.1h1.6v-2.5s-.1-2.7 2.7-2.7h4.6s2.6 0 2.6-2.5V5.3S16.7 2 11.9 2zM9.6 3.7c.5 0 .8.4.8.8s-.4.8-.8.8-.8-.4-.8-.8.3-.8.8-.8z" fill="#3776AB"/>
      <path d="M12.1 22c5.4 0 5-2.3 5-2.3l-.01-2.4h-5.1v-.7h6.9S22 17 22 11.7s-2.7-5.1-2.7-5.1h-1.6v2.5s.1 2.7-2.7 2.7H10.4s-2.6 0-2.6 2.5v4.4S7.3 22 12.1 22zm2.3-1.7c-.5 0-.8-.4-.8-.8s.4-.8.8-.8.8.4.8.8-.3.8-.8.8z" fill="#FFD43B"/>
    </svg>
  )

  if (t === 'node') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M12 2l9.2 5.3v10.6L12 22l-9.2-5.1V7.3L12 2z" fill="#339933" opacity="0.15"/>
      <path d="M12 2l9.2 5.3v10.6L12 22l-9.2-5.1V7.3L12 2z" stroke="#339933" strokeWidth="1.5" fill="none"/>
      <text x="12" y="15" textAnchor="middle" fill="#339933" fontSize="8" fontWeight="bold" fontFamily="monospace">JS</text>
    </svg>
  )

  if (t === 'rust') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <circle cx="12" cy="12" r="9.5" stroke="#DEA584" strokeWidth="1.5" fill="none"/>
      <text x="12" y="15.5" textAnchor="middle" fill="#DEA584" fontSize="9" fontWeight="bold" fontFamily="monospace">R</text>
      <circle cx="12" cy="2.5" r="1" fill="#DEA584"/>
      <circle cx="20.2" cy="7.3" r="1" fill="#DEA584"/>
      <circle cx="20.2" cy="16.7" r="1" fill="#DEA584"/>
      <circle cx="12" cy="21.5" r="1" fill="#DEA584"/>
      <circle cx="3.8" cy="16.7" r="1" fill="#DEA584"/>
      <circle cx="3.8" cy="7.3" r="1" fill="#DEA584"/>
    </svg>
  )

  if (t === 'go') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M3 12c0-5 4-9 9-9s9 4 9 9-4 9-9 9-9-4-9-9z" fill="#00ADD8" opacity="0.15"/>
      <text x="12" y="15.5" textAnchor="middle" fill="#00ADD8" fontSize="10" fontWeight="bold" fontFamily="monospace">Go</text>
    </svg>
  )

  if (t === 'flutter') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M14.3 2L4 12.3l3.2 3.2L20.5 2h-6.2z" fill="#42A5F5"/>
      <path d="M14.3 12.5L7.2 19.6l3.2 3.2 10.1-10.3h-6.2z" fill="#42A5F5"/>
      <path d="M7.2 15.5l3.2 3.2 3.9-3.9-3.2-3.2-3.9 3.9z" fill="#0D47A1"/>
    </svg>
  )

  if (t === 'docker' || t === 'docker-compose') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M13.5 2.5h2v2h-2zm-3 0h2v2h-2zm-3 0h2v2h-2zm-3 2h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm3 0h2v2h-2zm-3 2h2v2h-2z" fill="#2496ED"/>
      <path d="M23.5 9.8c-.7-.4-2.2-.6-3.4-.3-.2-1.3-.9-2.4-1.8-3.2l-.6-.5-.5.6c-.6.8-1 1.9-.9 2.8 0 .5.1 1 .4 1.5-.6.3-1.2.5-1.8.6H.8c-.3 1.6-.3 3.3.2 4.9.6 1.8 1.7 3.2 3.3 4.1 1.8 1 4.5 1.3 7 .5 1.9-.6 3.5-1.6 4.8-3.2 1-1.3 1.7-2.8 2.1-4.5h.2c1.1 0 2-.4 2.6-1.2.3-.4.5-1 .5-1.6v-.5z" fill="#2496ED"/>
    </svg>
  )

  if (t === 'java') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <path d="M8.5 18.5s-1 .6.7.8c2.1.2 3.2.2 5.5-.3 0 0 .6.4 1.4.7-5.1 2.2-11.5-.1-7.6-1.2zm-.6-2.8s-1.2.9.6 1c2.3.2 4 .2 7-.3 0 0 .4.4 1.1.6-6.2 1.8-13-.1-8.7-1.3z" fill="#E76F00"/>
      <path d="M13.5 10.7c1.3 1.5-.3 2.8-.3 2.8s3.3-1.7 1.8-3.8c-1.4-2-2.5-3 3.4-6.4 0 0-9.2 2.3-4.9 7.4z" fill="#E76F00"/>
      <text x="12" y="21" textAnchor="middle" fill="#5382A1" fontSize="5" fontWeight="bold" fontFamily="monospace">JAVA</text>
    </svg>
  )

  if (t === 'dotnet') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <circle cx="12" cy="12" r="10" fill="#512BD4" opacity="0.15"/>
      <text x="12" y="15" textAnchor="middle" fill="#512BD4" fontSize="7" fontWeight="bold" fontFamily="monospace">.N</text>
    </svg>
  )

  if (t === 'php') return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none">
      <ellipse cx="12" cy="12" rx="11" ry="7" fill="#777BB4" opacity="0.15" stroke="#777BB4" strokeWidth="1"/>
      <text x="12" y="15" textAnchor="middle" fill="#777BB4" fontSize="8" fontWeight="bold" fontFamily="monospace">php</text>
    </svg>
  )

  // Unknown / generic
  return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="16 18 22 12 16 6"/>
      <polyline points="8 6 2 12 8 18"/>
    </svg>
  )
}
