// Entrada exclusiva da QA via Vite (MEX-1391): renderiza a seção real "Trocas de conta" do Mission Board com um
// retrato de exemplo, no formato do Hub (`accountSwitches`). Não é importada pelo app nem usa um Hub real.
import './bridge'
import { createRoot } from 'react-dom/client'
import type { TowerSnapshot } from '@shared/fork-torre'
import { TrocasDeConta } from '../views/fork/torre/TrocasDeConta'
import '../styles/tokens.css'
import '../styles/fonts.css'
import '../styles/theme.css'
import '../views/fork/torre/torre.css'

const agora = Date.now()
const min = 60_000
const bot = (botId: string, name: string) => ({ botId, name, title: null, avatarColor: 'blue', avatarShape: 'circle' }) as never
const snapshot = {
  version: 1, hubId: 'qa', snapshotId: 'qa', collectedAt: agora, spaceId: 'pessoal',
  bots: [bot('berga', 'Berga'), bot('byte', 'Byte')],
  accountSwitches: {
    availability: 'available', source: 'fallback.trocas', coverage: 'accountSwitchesInSpaceLast7Days',
    value: {
      omitted: 0,
      items: [
        { id: '1', at: agora - 4 * min, botId: 'berga', fromLabel: 'Empresa', toLabel: 'Reserva', reason: 'perto_do_limite', backAt: agora + 142 * min },
        { id: '2', at: agora - 95 * min, botId: 'byte', fromLabel: 'Pessoal', toLabel: 'Reserva 2', reason: 'esgotada', backAt: agora + 26 * 60 * min },
        { id: '3', at: agora - 26 * 60 * min, botId: 'berga', fromLabel: 'Reserva', toLabel: 'Empresa', reason: 'voltou', backAt: null },
      ],
    },
  },
} as unknown as TowerSnapshot
const params = new URLSearchParams(location.search)
document.documentElement.dataset.theme = params.get('tema') ?? 'light'
createRoot(document.getElementById('root')!).render(<div className="torre-host" style={{ padding: 24, maxWidth: 900 }}><TrocasDeConta snapshot={snapshot} /></div>)
