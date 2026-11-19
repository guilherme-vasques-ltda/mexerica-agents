// O botão do Mission Board na faixa (98A): o mesmo `.space-rail-button` dos vizinhos e o ponto `.space-rail-dot`
// da engrenagem. A regra do ponto está em badge.ts; aqui só se desenha.
import { Icon } from '../../../components/Icon'
import { useIdioma } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { railLabel, waitingYouCount } from './badge'
import { torreScreen } from './navigation'

export function TorreRailButton({ store, inPessoal, active, onOpen }: { store: BotStore | null; inPessoal: boolean; active: boolean; onOpen: () => void }) {
  useModel(store ?? torreScreen)
  const n = store ? waitingYouCount({
    online: !!store.connectedClient && !store.isOffline,
    inPessoal,
    personalSpaceId: store.spaces?.personalSpaceId ?? null,
    bots: store.bots.values(),
  }) : null
  const label = railLabel(n, useIdioma())
  return (
    <button className="space-rail-button torre-open" title={label} aria-label={label} aria-current={active ? 'page' : undefined} onClick={onOpen}>
      <Icon name="donna.torre" size={16 /* RAIL_ICON, views/shell/SpaceRail */} weight="light" />
      {n ? <span className="space-rail-dot" aria-hidden /> : null}
    </button>
  )
}
