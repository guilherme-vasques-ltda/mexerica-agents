import type { MemoryOrigin as Origin } from '@shared/models'
import { useT } from '../../../lib/fork-i18n/estado'
import { originLabel } from './presentation'
import './memory-auto.css'

export function MemoryOrigin({ origin }: { origin?: Origin | null }) {
  const t = useT()
  const tone = origin?.schema === 1 && (origin.external || origin.kind === 'external') ? 'external'
    : !origin || origin.schema !== 1 || origin.kind === 'unknown' ? 'unknown' : 'known'
  return <span className={`memory-origin ${tone}`}>{originLabel(t, origin)}</span>
}
