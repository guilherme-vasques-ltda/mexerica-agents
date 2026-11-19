// MX-217 (pedido do dono, 11/11/2026): uma queda curta do link de um computador remoto (túnel SSH ou canal da
// nuvem) não fecha a tela. A loja fica anexada, com o cache, os rascunhos, as mensagens não enviadas e a seleção;
// só o caminho até o Hub é trocado. Depois de QUEDA_TOLERADA_MS sem link, ou numa falha definitiva, a loja sai
// como antes (o detach de sempre), e a tela de desconectado aparece.
// Regras puras, sem React nem rede: views/settings/ssh-model.ts e views/fork/cloud/fork-cloud-attachments.ts usam.

import type { HostError, HostTransport } from '../client/host-client'

/** Quanto tempo uma queda temporária mantém a tela antes de tratá-la como desconexão. */
export const QUEDA_TOLERADA_MS = 5 * 60_000

/**
 * O caminho atual até o Hub de um computador remoto, trocável sem trocar a loja. Com o link caído (`null`),
 * o transporte é o `parado`: falha na hora, sem rede. Cada transporte entregue pertence à geração do link em que
 * nasceu: trocado o link (queda ou túnel novo), ele deixa de chamar e de entregar resposta ou evento, inclusive no
 * meio de uma operação, e falha fechado (`caido`), sem redirecionar nada ao caminho novo.
 */
export class LinkTrocavel<T> {
  private atual: T | null
  private geracao = 0
  private readonly abrir: (link: T) => HostTransport
  private readonly parado: HostTransport
  private readonly caido: () => HostError

  constructor(atual: T | null, abrir: (link: T) => HostTransport, parado: HostTransport, caido: () => HostError) {
    this.atual = atual
    this.abrir = abrir
    this.parado = parado
    this.caido = caido
  }

  get vivo() { return this.atual !== null }
  trocar(link: T | null) {
    this.atual = link
    this.geracao++
  }
  readonly transporte = (): HostTransport => {
    if (this.atual === null) return this.parado
    const real = this.abrir(this.atual)
    const geracao = this.geracao
    const vale = () => geracao === this.geracao
    return {
      call: async (method, body, timeoutMs) => {
        if (!vale()) throw this.caido()
        let resposta: unknown
        try { resposta = await real.call(method, body, timeoutMs) } catch (error) { throw vale() ? error : this.caido() }
        if (!vale()) throw this.caido()
        return resposta
      },
      stream: (path, onData, onEnd) => {
        if (!vale()) return this.parado.stream(path, onData, onEnd)
        return real.stream(path, (line) => { if (vale()) onData(line) }, (error) => onEnd(vale() ? error : this.caido()))
      },
    }
  }
}

/** O que a queda usa da loja (BotStore). */
export interface LojaQueda {
  restartStream(): void
  marcarQueda(desde: number | null): void
}

/** Várias lojas (a raiz e os spaces de um computador) caindo e voltando juntas. */
export function lojasJuntas(lojas: () => readonly LojaQueda[]): LojaQueda {
  return {
    restartStream: () => { for (const loja of lojas()) loja.restartStream() },
    marcarQueda: (desde) => { for (const loja of lojas()) loja.marcarQueda(desde) },
  }
}

export interface Relogio {
  agora(): number
  depois(fn: () => void, ms: number): unknown
  cancelar(handle: unknown): void
}

const relogioReal: Relogio = {
  agora: () => Date.now(),
  depois: (fn, ms) => setTimeout(fn, ms),
  cancelar: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
}

/** Uma loja anexada por um binder (SSH ou nuvem), com o link dela e o prazo da queda em curso. */
export class LojaSegura<T> {
  private prazo: unknown = null
  readonly loja: LojaQueda
  readonly link: LinkTrocavel<T>
  private readonly relogio: Relogio

  constructor(loja: LojaQueda, link: LinkTrocavel<T>, relogio: Relogio = relogioReal) {
    this.loja = loja
    this.link = link
    this.relogio = relogio
  }

  /** Em queda: estacionada, esperando o link voltar dentro do prazo. */
  get caida() { return this.prazo !== null }

  /**
   * O link caiu de forma temporária: estaciona a loja (para de usar o caminho antigo) e conta o prazo. No fim do
   * prazo, `soltar` (o detach de sempre). Uma segunda queda durante a primeira não reinicia o prazo.
   */
  cair(soltar: () => void, ms = QUEDA_TOLERADA_MS) {
    if (this.prazo !== null) return
    this.link.trocar(null)
    this.prazo = this.relogio.depois(() => {
      this.prazo = null
      soltar()
    }, ms)
    this.loja.restartStream()
    this.loja.marcarQueda(this.relogio.agora())
  }

  /** O link voltou: a mesma loja segue pelo caminho novo; a tela, a rolagem e o rascunho não mudam. */
  voltar(link: T) {
    this.encerrar()
    this.link.trocar(link)
    this.loja.marcarQueda(null)
    this.loja.restartStream()
  }

  /**
   * A loja sai de vez (desligada, removida, outro dono, outra identidade, fim do prazo): o link é revogado antes de
   * ela ser aposentada, e todo transporte já entregue falha fechado, inclusive no meio de uma operação.
   */
  revogar() {
    this.encerrar()
    this.link.trocar(null)
  }

  /** Para o prazo sem soltar (o link voltou). */
  encerrar() {
    if (this.prazo !== null) this.relogio.cancelar(this.prazo)
    this.prazo = null
  }
}

/**
 * SSH: o túnel sumiu, mas o Main ainda tenta reabrir (`connecting`/`retrying`/`connected`). Desligar à mão (`idle`),
 * perfil removido, falha definitiva (`failed`, `notInstalled`) ou chave a confirmar (`confirmHostKey`) soltam na hora.
 */
export function sshQuedaTemporaria(status: { kind: string } | undefined): boolean {
  return !!status && (status.kind === 'connecting' || status.kind === 'retrying' || status.kind === 'connected')
}

/**
 * Nuvem: o canal sumiu, mas o Main ainda reconecta sozinho ao mesmo Hub (`connecting`/`reconnecting`). `hubOffline`
 * é diagnóstico em que o Main parou de tentar; ele, acesso encerrado, revogação, saída da conta, Hub trocado ou não
 * verificado e qualquer outro estado soltam na hora (falha fechada).
 */
export function nuvemQuedaTemporaria(state: { kind: string; computerId?: string } | null | undefined, computerId: string): boolean {
  if (!state || state.computerId !== computerId) return false
  return state.kind === 'connecting' || state.kind === 'reconnecting'
}

/**
 * A faixa "Sem conexão — reconectando…" (views/fork/conexao): link caído e ainda dentro do prazo, ou loja sem
 * conexão por queda (offline, computador fora). Sem acesso (401/403) e versão incompatível têm a própria tela.
 */
export function emQueda(store: { connection?: { kind: string }; linkCaidoDesde?: number | null; mismatch?: unknown }): boolean {
  const kind = store.connection?.kind
  if (!kind || store.mismatch || kind === 'unauthorized' || kind === 'unpaired') return false
  return (store.linkCaidoDesde ?? null) !== null || kind === 'offline' || kind === 'computerOffline'
}
