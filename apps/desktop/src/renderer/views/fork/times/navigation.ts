// Abre e fecha a tela Times (components/TimesView), que ocupa a janela no lugar das conversas, como Rotinas.
import { Observable } from '../../../lib/observable'

class TimesScreen extends Observable {
  open = false
  /** O space em que Times abriu (null: Pessoal). Trocar de space fecha a tela. */
  spaceId: string | null = null
  /** O computador do contexto: o do space aberto (null no Pessoal, que segue o bot selecionado). */
  computerId: string | null = null
  /** O da faixa, só quando não há bot selecionado. */
  fallbackComputerId: string | null = null
  /** Cada abertura é uma sessão nova (o Hub é lido de novo). */
  seq = 0

  show(spaceId: string | null, computerId: string | null = null, fallbackComputerId: string | null = null) {
    this.open = true
    this.spaceId = spaceId
    this.computerId = computerId
    this.fallbackComputerId = fallbackComputerId
    this.seq++
    this.changed()
  }

  /** `restoreFocus`: o foco volta ao botão da engrenagem da faixa, que abriu a tela. */
  close(restoreFocus = true) {
    if (!this.open) return
    this.open = false
    this.changed()
    if (!restoreFocus) return
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const button = [...document.querySelectorAll<HTMLElement>('.times-open')].find((b) => !b.closest('[inert]') && !b.hasAttribute('disabled'))
      button?.focus()
    }))
  }
}

export const timesScreen = new TimesScreen()
