import { Observable } from '../../../lib/observable'

/** Pedido local de foco: a conversa montada consome só o próprio destino. Não cria sessão nem envia mensagem. */
class FocoBerga extends Observable {
  request: { computerId: string; botId: string } | null = null
  show(computerId: string, botId: string) {
    this.request = { computerId, botId }
    this.changed()
  }
  clear() { this.request = null; this.changed() }
}
export const focoBerga = new FocoBerga()
