// VG-02 (98A): abre e fecha o Mission Board, um lugar próprio da faixa como Rotinas e Times.
import type { OpenTarget } from '@shared/fork-torre'
import { Observable } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'

class TorreScreen extends Observable {
  open = false
  /** O space da faixa ao abrir (null: Pessoal do Hub). */
  spaceId: string | null = null
  /** O computador cujo Hub o Mission Board lê; nunca troca para outro Hub. */
  computerId: string | null = null
  /** Cada abertura relê o Hub. */
  seq = 0
  /** D541: a seção dentro do Mission Board (Projetos do Space aberto); toda abertura começa no Board. */
  section: 'board' | 'projects' = 'board'
  /**
   * A navegação da janela (ChatWindow → use-space-view e seleção do Pessoal). Abrir só navega: não cria conversa,
   * não envia, não aprova, não retoma nem para nada.
   */
  navigate: ((store: BotStore, target: OpenTarget) => void) | null = null

  show(spaceId: string | null, computerId: string | null) {
    this.open = true
    this.spaceId = spaceId
    this.computerId = computerId
    this.section = 'board'
    this.seq++
    this.changed()
  }

  showSection(section: 'board' | 'projects') {
    if (this.section === section) return
    this.section = section
    this.changed()
  }

  close(restoreFocus = true) {
    if (!this.open) return
    this.open = false
    this.changed()
    if (!restoreFocus) return
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const button = [...document.querySelectorAll<HTMLElement>('.torre-open')].find((b) => !b.closest('[inert]') && !b.hasAttribute('disabled'))
      button?.focus()
    }))
  }
}

export const torreScreen = new TorreScreen()
