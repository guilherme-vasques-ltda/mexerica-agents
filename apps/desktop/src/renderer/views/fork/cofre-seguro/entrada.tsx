// MX-209: ponto de entrada da janela do cartão seguro (`?window=cofre-seguro`). Sem `window.codync`: só o tema e o idioma
// (lidos do armazenamento local, como nas outras janelas) e a ponte estreita `window.mxCofre` do preload dela.
import { createRoot } from 'react-dom/client'
import '../../../styles/tokens.css'
import '../../../styles/fonts.css'
import '../../../styles/theme.css'
import { startAppearance } from '../../../lib/appearance'
import { sairDaAbertura } from '../../../lib/abertura'
import { JanelaSegura } from './JanelaSegura'

export function abrir() {
  document.documentElement.dataset.window = 'cofre-seguro'
  startAppearance()
  sairDaAbertura()
  createRoot(document.getElementById('root')!).render(<JanelaSegura />)
}
