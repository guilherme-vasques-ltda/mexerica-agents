// voz-03 (plano §5, S8): limites de toda chamada Ao vivo, de qualquer fonte, desde o começo.
// Prazos absolutos no relógio de parede: um timer atrasado (janela oculta, Mac suspenso) encerra
// no primeiro tick depois do prazo, sem depender de contar segundos no renderer.

export const LIMITES = {
  avisoSilencio: 4 * 60_000 + 30_000,
  silencio: 5 * 60_000,
  avisoTeto: 29 * 60_000,
  teto: 30 * 60_000,
} as const

export type MotivoLimite = 'silencio' | 'teto'

export interface EstadoLimites {
  aviso: MotivoLimite | null
  fim: MotivoLimite | null
}

/** setTimeout/clearTimeout, injetáveis para teste. */
export interface Relogio {
  set: (fn: () => void, ms: number) => unknown
  clear: (timer: unknown) => void
}

/** Fatia 6b (D454): chaves do catálogo voz/chamada; a tela da chamada traduz ao desenhar. */
export const TEXTOS_LIMITE = {
  avisoSilencio: 'voz.chamada.limite.avisoSilencio',
  continuar: 'voz.chamada.limite.continuar',
  avisoTeto: 'voz.chamada.limite.avisoTeto',
  fimSilencio: 'voz.chamada.limite.fimSilencio',
  fimTeto: 'voz.chamada.limite.fimTeto',
  ligarDeNovo: 'voz.chamada.limite.ligarDeNovo',
} as const

/**
 * Um relógio por chamada: criado quando a pessoa liga, nunca reiniciado por queda, reconexão ou troca
 * de motor/fonte. "Conversa" é fala real da pessoa (texto reconhecido ou turno confirmado pelo provedor);
 * ruído e áudio do modelo não contam. Bot falando ou trabalhando suspende o relógio de silêncio, nunca o teto.
 */
export class LimitesChamada {
  private readonly inicio: number
  private ultimaConversa: number
  private ocupado = false
  private estado: EstadoLimites = { aviso: null, fim: null }
  private readonly agora: () => number
  private readonly aoMudar: (estado: EstadoLimites) => void
  private relogio: Relogio | null = null
  private timer: unknown = null
  private parado = false

  constructor(agora: () => number, aoMudar: (estado: EstadoLimites) => void) {
    this.agora = agora
    this.aoMudar = aoMudar
    this.inicio = agora()
    this.ultimaConversa = this.inicio
  }

  get atual(): EstadoLimites {
    return this.estado
  }

  /** A pessoa falou de verdade. */
  conversou() {
    if (this.estado.fim) return
    this.ultimaConversa = this.agora()
    this.conferir()
  }

  /** O bot está falando ou trabalhando: o silêncio não corre; ao terminar, recomeça dali. */
  botOcupado(ocupado: boolean) {
    if (this.estado.fim || ocupado === this.ocupado) return
    if (!ocupado) this.ultimaConversa = Math.max(this.ultimaConversa, this.agora())
    this.ocupado = ocupado
    this.conferir()
  }

  /** "Continuar" no aviso de silêncio. Não mexe no teto. */
  continuar() {
    this.conversou()
  }

  /**
   * Liga o relógio: confere em cada limiar e se reagenda sempre que um prazo muda (conversa, bot que
   * começa ou termina de trabalhar). Devolve a função que desliga.
   */
  iniciarRelogio(relogio: Relogio): () => void {
    this.relogio = relogio
    this.reagendar()
    return () => {
      if (this.timer !== null) relogio.clear(this.timer)
      this.timer = null
      this.relogio = null
    }
  }

  private reagendar() {
    if (!this.relogio) return
    if (this.timer !== null) this.relogio.clear(this.timer)
    this.timer = null
    const ms = this.proximoPrazo()
    if (ms === null) return
    this.timer = this.relogio.set(() => {
      this.timer = null
      this.conferir()
    }, Math.max(250, ms))
  }

  /** A chamada terminou por outro motivo (falha sem queda): os limites param onde estão, sem aviso nem fim novos. */
  parar() {
    this.parado = true
    if (this.timer !== null) this.relogio?.clear(this.timer)
    this.timer = null
  }

  /** Recalcula; idempotente depois do fim. Devolve o estado atual. */
  conferir(): EstadoLimites {
    if (this.estado.fim || this.parado) return this.estado
    const agora = this.agora()
    const total = agora - this.inicio
    const quieto = this.ocupado ? 0 : agora - this.ultimaConversa
    let next: EstadoLimites
    if (total >= LIMITES.teto) next = { aviso: null, fim: 'teto' }
    else if (quieto >= LIMITES.silencio) next = { aviso: null, fim: 'silencio' }
    else {
      const avisaSilencio = quieto >= LIMITES.avisoSilencio
      const avisaTeto = total >= LIMITES.avisoTeto
      // Os dois ao mesmo tempo: vale o que termina antes.
      const aviso: MotivoLimite | null = avisaSilencio && avisaTeto
        ? (LIMITES.silencio - quieto <= LIMITES.teto - total ? 'silencio' : 'teto')
        : avisaSilencio ? 'silencio' : avisaTeto ? 'teto' : null
      next = { aviso, fim: null }
    }
    const mudou = next.aviso !== this.estado.aviso || next.fim !== this.estado.fim
    this.estado = next
    this.reagendar()
    if (mudou) this.aoMudar(next)
    return this.estado
  }

  /** Milissegundos até o próximo limiar, para agendar a próxima conferência; null depois do fim. */
  proximoPrazo(): number | null {
    if (this.estado.fim) return null
    const agora = this.agora()
    const total = agora - this.inicio
    const quieto = agora - this.ultimaConversa
    const prazos = [LIMITES.avisoTeto - total, LIMITES.teto - total]
    if (!this.ocupado) prazos.push(LIMITES.avisoSilencio - quieto, LIMITES.silencio - quieto)
    const futuros = prazos.filter((ms) => ms > 0)
    return futuros.length ? Math.min(...futuros) : 0
  }
}
