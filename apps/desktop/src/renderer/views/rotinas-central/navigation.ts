// Abre e fecha a tela central "Rotinas" (D85), que ocupa a janela no lugar das conversas.
import { Observable } from '../../lib/observable'

class RoutinesScreen extends Observable {
  /** `CODYNC_DEBUG_OPEN=rotinas` (builds de desenvolvimento) abre a tela ao iniciar, para prints. */
  open = window.codync?.debugOpen === 'rotinas'
  /** O bot já filtrado ao abrir (o atalho da lista de rotinas de um bot). */
  botId: string | null = null

  show(botId: string | null = null) {
    this.open = true
    this.botId = botId
    this.changed()
  }

  /** `restoreFocus`: o foco volta ao botão de Rotinas (as conversas montam de novo; o botão que abriu pode ter saído). */
  close(restoreFocus = true) {
    if (!this.open) return
    this.open = false
    this.changed()
    if (!restoreFocus) return
    // Dois quadros: a troca de tela e a montagem das conversas.
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const button = [...document.querySelectorAll<HTMLElement>('.rc-open')].find((b) => !b.closest('[inert]') && !b.hasAttribute('disabled'))
      button?.focus()
    }))
  }
}

export const routinesScreen = new RoutinesScreen()
