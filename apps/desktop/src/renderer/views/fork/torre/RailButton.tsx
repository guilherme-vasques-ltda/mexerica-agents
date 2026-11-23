import { useEffect, useState } from 'react'
import { Icon } from '../../../components/Icon'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { attentionCount, railLabel } from './badge'
import { torreScreen } from './navigation'
import { TowerSession } from './torre-session'

export function TorreRailButton({ store, spaceId, active, onOpen }: { store: BotStore | null; spaceId: string | null; active: boolean; onOpen: () => void }) {
  useModel(store ?? torreScreen)
  const [session] = useState(() => new TowerSession(Date.now, 'badge'))
  useModel(session)
  const selectedSpace = spaceId ?? store?.spaces?.personalSpaceId ?? null
  useEffect(() => {
    if (!store || !selectedSpace) return
    session.open(store, selectedSpace)
    return () => session.close()
  }, [session, store, selectedSpace])
  const queue = store && !store.isOffline && selectedSpace ? session.currentAttention(store, selectedSpace) : null
  const n = attentionCount(queue)
  const t = useT()
  const baseLabel = railLabel(n, useIdioma())
  const gap = !!queue?.coverageGaps.length
  const label = gap ? t('torre.esperandoVoce.trilhoParcial', { label: baseLabel }) : baseLabel
  return (
    <button className="space-rail-button torre-open" title={label} aria-label={label} aria-current={active ? 'page' : undefined} onClick={onOpen}>
      <Icon name="donna.torre" size={16} weight="light" />
      {n || gap ? <span className="space-rail-dot" aria-hidden /> : null}
    </button>
  )
}
