// Auditoria D249 (E2): a ligação entre Ajustes e as outras telas fica com a janela, carregada ao abrir o app, e não
// com AjustesTela.tsx, que agora só carrega ao abrir Ajustes. Saiu de lá sem mudar o comportamento.
import { routinesScreen } from '../../rotinas-central/navigation'
import { timesScreen } from '../times/navigation'
import { settingsScreen } from './tela'

export { settingsScreen }

// Um lugar atual por vez: abrir Ajustes fecha Rotinas e Times, e abrir um deles fecha Ajustes (a faixa já fecha na
// mesma hora; a inscrição cobre os outros caminhos).
let focoAntes: Element | null = null
settingsScreen.aoAbrir = () => {
  if (!settingsScreen.open) focoAntes = document.activeElement
  routinesScreen.close(false)
  timesScreen.close(false)
}
routinesScreen.subscribe(() => { if (routinesScreen.open) settingsScreen.close(false) })
timesScreen.subscribe(() => { if (timesScreen.open) settingsScreen.close(false) })
// Voltar e Esc devolvem o foco a quem abriu (o botão da chamada, um link da página), senão à engrenagem da faixa.
// Dois quadros: a troca de tela e a volta da janela (sem `inert`).
settingsScreen.aoFechar = () => requestAnimationFrame(() => requestAnimationFrame(() => {
  const alvo = [focoAntes, document.querySelector('[data-ajustes-volta]')].find(podeFocar)
  focoAntes = null
  alvo?.focus()
}))

function podeFocar(el: Element | null | undefined): el is HTMLElement {
  return el instanceof HTMLElement && el !== document.body && el.isConnected && el.getClientRects().length > 0 && !el.closest('[inert], [role="menu"]')
}
