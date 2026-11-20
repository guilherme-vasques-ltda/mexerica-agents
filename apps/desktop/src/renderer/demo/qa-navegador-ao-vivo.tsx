// Entrada exclusiva da QA via Vite; não é importada pelo app nem usa um Hub real.
// Cenários do painel ao vivo por query: ?cenario=agente|lento|negado|semcaps&theme=light|dark&perder=<segundos>
import './bridge'
import { createRoot } from 'react-dom/client'
import { HubNavegadorFalso, type CenarioAoVivo } from './fake-ao-vivo'
import { NavegadorAoVivo } from '../views/fork/navegador-ao-vivo/NavegadorAoVivo'
import { idiomaPref } from '../lib/fork-i18n/estado'
import { applyTextSize } from '../lib/prefs'
import { appearancePrefs, startAppearance } from '../lib/appearance'
import '../styles/tokens.css'
import '../styles/fonts.css'
import '../styles/theme.css'

const q = new URLSearchParams(location.search)
const cenario = (['agente', 'lento', 'negado', 'semcaps'].includes(q.get('cenario') ?? '') ? q.get('cenario') : 'agente') as CenarioAoVivo
const tema = q.get('theme') === 'light' ? 'light' : 'dark'

const hub = new HubNavegadorFalso(cenario)
// ?perder=N derruba o canal depois de N segundos (cenário de conexão perdida).
const perder = Number(q.get('perder'))
if (perder > 0) setTimeout(() => hub.perder(), perder * 1000)
idiomaPref.set('pt-BR')
appearancePrefs.theme.set(tema)
applyTextSize(13)
startAppearance()
document.body.style.cssText = 'margin:0;padding:24px;background:var(--canvas);'

const root = createRoot(document.getElementById('root')!)
root.render(<div style={{ maxWidth: 760 }}>
  <NavegadorAoVivo bridge={hub.bridge} scope={{ hubId: 'qa', spaceId: 'sp', botId: 'lia' }} session="s-qa" target="aba-1" conexao={`qa:${cenario}`} botName="Lia" onClose={() => root.render(null)} />
</div>)

Object.defineProperty(window, 'qaAoVivo', { value: { cenario, hub, tema } })
