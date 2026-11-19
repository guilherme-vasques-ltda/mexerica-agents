import type { ReactNode } from 'react'
import { useToquesMexerica } from '../../lib/toques-mexerica'

/** D530: decoração sem significado próprio, sempre no fluxo e fora da árvore acessível. */
export function CascaVazia({ fallback }: { fallback: ReactNode }) {
  const touches = useToquesMexerica()
  if (!touches) return <>{fallback}</>
  return <span className="mx-empty-icon" aria-hidden="true">
    <span className="mx-empty-fallback">{fallback}</span>
    <svg className="mx-decoration mx-peel" viewBox="0 0 80 80" focusable="false">
      <path d="M23 31C9 43 16 62 36 62c22 0 30-16 18-25-9-7-25-3-24 6 1 8 17 9 17 2M23 28c3-8 13-14 24-11" fill="none" stroke="currentColor" strokeWidth="7" strokeLinecap="round" />
      <path d="M21 32C10 45 21 59 37 59c16 0 23-9 19-17" fill="none" stroke="var(--raised)" strokeWidth="2" strokeLinecap="round" />
    </svg>
  </span>
}

export function Fiapos() {
  return <svg className="mx-decoration mx-pith" viewBox="0 0 32 32" aria-hidden="true" focusable="false">
    <path d="M6 5C0 9 8 14 4 19s-1 7 2 8M25 4c8 7-1 11 3 16s0 8-3 9" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </svg>
}

export function Gomo({ fill }: { fill?: string }) {
  return <svg className="mx-decoration mx-wedge" style={{ color: fill }} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <path d="M3 6h18c0 8-4 12-9 12S3 14 3 6Z" fill="currentColor" />
    <path d="M4 7h16c-1 7-4 10-8 10S5 14 4 7ZM12 7v9M12 7l-5 7M12 7l5 7" fill="none" stroke="var(--raised)" strokeWidth="1" strokeLinecap="round" />
  </svg>
}

/** Os três paths oficiais de brand/assets/logo/03-simbolo/svg/mexerica-simbolo.svg, sem redesenho. */
export function MarcaMexerica({ label }: { label: string }) {
  return <svg className="mx-logo" viewBox="257.09 -93.09 43.81 43.81" role="img" aria-label={label} focusable="false">
    <path className="mx-leaf" d="M278.86 -79.16Q281.91 -91.32 294.26 -89.20Q291.22 -77.04 278.86 -79.16Z" fill="#5C9B3A" />
    <path d="M263.05 -66.37a13.75 13.75 0 1 0 27.50 0a13.75 13.75 0 1 0 -27.50 0Z" fill="#F58A1F" />
    <path d="M270.39 -66.37a2.20 2.20 0 1 0 4.40 0a2.20 2.20 0 1 0 -4.40 0ZM278.81 -66.37a2.20 2.20 0 1 0 4.40 0a2.20 2.20 0 1 0 -4.40 0Z" fill="#2A1C14" />
  </svg>
}
