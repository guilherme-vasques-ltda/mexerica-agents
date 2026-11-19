// MX-214 (rodada 3): "nada é enviado com o aviso de senha colada aberto" vale para TODA entrada que manda mensagem
// ao bot, não só para o Composer: chamada de voz, painel de detalhes, Ilha, operador de voz. O Composer registra aqui a
// pendência do destino; BotStore.send recusa o envio e os botões de chamada se desligam. Só guarda quem bloqueia e para
// onde, nunca o texto escondido.
import { Observable } from '../lib/observable.ts'

/** O destino de um envio: o computador (Hub) e o bot. Todas as sessões do bot contam como o mesmo destino. */
export const destinoSenha = (computerId: string, botId: string) => `${computerId}␟${botId}`

/** O que `BotStore.send` e `retry` devolvem: o envio saiu (ou nada havia a enviar) ou foi barrado pelo aviso de senha. */
export type ResultadoEnvio = 'aceito' | 'bloqueado'

class BloqueioSenha extends Observable {
  private donos = new Map<string, string>()
  private barrados = new Set<string>()

  /** `dono`: quem abriu (a chave do rascunho da caixa). Abrir de novo com o mesmo dono só troca o destino. */
  abrir(dono: string, destino: string) {
    if (this.donos.get(dono) === destino) return
    this.donos.set(dono, destino)
    this.changed()
  }

  fechar(dono: string) {
    const destino = this.donos.get(dono)
    if (this.donos.delete(dono)) {
      if (destino !== undefined && !this.bloqueado(destino)) this.barrados.delete(destino)
      this.changed()
    }
  }

  /** Um envio a este destino foi recusado: o cartão do aviso mostra o motivo ali mesmo (aviso, não erro). */
  barrar(destino: string) {
    if (!this.bloqueado(destino) || this.barrados.has(destino)) return
    this.barrados.add(destino)
    this.changed()
  }

  /** Houve envio recusado enquanto esta pendência está aberta. */
  foiBarrado(destino: string): boolean {
    return this.barrados.has(destino)
  }

  bloqueado(destino: string): boolean {
    for (const d of this.donos.values()) if (d === destino) return true
    return false
  }
}

export const bloqueioSenha = new BloqueioSenha()
