// Auditoria D249 (E2): a ligação entre Extensões e as outras telas fica com a janela, carregada ao abrir o app, e
// não com ExtensoesTela.tsx, que agora só carrega ao abrir Extensões. Saiu de lá sem mudar o comportamento.
import { routinesScreen } from '../../rotinas-central/navigation'
import { settingsScreen } from '../ajustes/tela'
import { timesScreen } from '../times/navigation'
import { extensoesScreen } from './tela'

export { extensoesScreen }

// Um lugar atual por vez: abrir Extensões fecha Rotinas, Times e Ajustes, e abrir um deles fecha Extensões.
let focoAntes: Element | null = null
extensoesScreen.aoAbrir = () => {
  if (!extensoesScreen.open) focoAntes = document.activeElement
  routinesScreen.close(false)
  timesScreen.close(false)
  settingsScreen.close(false)
}
for (const outra of [routinesScreen, timesScreen, settingsScreen]) outra.subscribe(() => { if (outra.open) extensoesScreen.close(false) })
extensoesScreen.aoFechar = () => requestAnimationFrame(() => requestAnimationFrame(() => {
  const alvo = [focoAntes, document.querySelector('[data-ajustes-volta]')].find((el): el is HTMLElement =>
    el instanceof HTMLElement && el !== document.body && el.isConnected && el.getClientRects().length > 0 && !el.closest('[inert], [role="menu"]'))
  focoAntes = null
  alvo?.focus()
}))
