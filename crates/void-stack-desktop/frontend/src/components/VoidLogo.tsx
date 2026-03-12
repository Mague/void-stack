interface Props {
  size?: number
  className?: string
}

export default function VoidLogo({ size = 28, className }: Props) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 100 100"
      width={size}
      height={size}
      className={className}
    >
      <defs>
        <linearGradient id="vs-glow-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#00b4ff" />
          <stop offset="50%" stopColor="#00ffe5" />
          <stop offset="100%" stopColor="#a855f7" />
        </linearGradient>
        <filter id="vs-glow">
          <feGaussianBlur stdDeviation="2" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      <polygon
        points="50,5 92,27 92,73 50,95 8,73 8,27"
        fill="none"
        stroke="#00b4ff"
        strokeWidth="2"
        filter="url(#vs-glow)"
      />
      <polygon
        points="50,20 75,50 50,80 25,50"
        fill="none"
        stroke="#00ffe5"
        strokeWidth="1.5"
        filter="url(#vs-glow)"
      />
      <line x1="50" y1="5" x2="50" y2="95" stroke="#00b4ff" strokeWidth="0.5" opacity="0.2" />
      <line x1="8" y1="50" x2="92" y2="50" stroke="#00b4ff" strokeWidth="0.5" opacity="0.2" />
      <circle cx="50" cy="50" r="6" fill="url(#vs-glow-gradient)" filter="url(#vs-glow)" />
      <circle cx="50" cy="50" r="3" fill="#ffffff" opacity="0.6" />
    </svg>
  )
}
