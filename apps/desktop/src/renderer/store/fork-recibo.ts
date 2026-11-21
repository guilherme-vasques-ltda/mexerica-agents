// Rota-03b: recibos do turno lidos sob demanda, um por entrada, guardados em memória (fora do mirror).
import { ROUTING_PERSONAL, type RoutingReceiptResponse } from '../../shared/fork-roteamento.ts'
import { Observable } from '../lib/observable.ts'
import { readRoutingReceipt, type RoutingSource } from './fork-roteamento.ts'

/** Onde ler o recibo de uma resposta: a entrada do usuário do mesmo turno, na mesma sessão. */
/** `conversationId`: o chat da entrada (num Space, a conversa, não o bot). Obrigatório: o Hub recusa conversa divergente. */
export interface ReceiptRequest { botId: string; conversationId: string; sessionId: string; entryId: string }
export type ReceiptLoad =
  | { status: 'loading' }
  | { status: 'error'; code: string }
  | { status: 'loaded'; response: RoutingReceiptResponse }

/** Código do erro (`codigo: mensagem`). */
export function errorCode(error: unknown): string {
  const raw = typeof error === 'object' && error !== null && 'message' in error ? String(error.message) : String(error)
  return raw.replace(/^Error: /, '').split(':')[0]!.trim()
}
/** Sem autorização, Hub sem a RPC ou contexto trocado: nenhuma outra resposta terá recibo agora. */
const blocking = (code: string) => code === 'remote_setup_unavailable' || code === 'unknown method' || code === 'routing_context_changed'

const LIMIT = 3

/** Um cache por Hub conectado e principal; trocar qualquer um começa vazio. */
export class ReceiptCache extends Observable {
  private loads = new Map<string, ReceiptLoad>()
  private queue: (() => void)[] = []
  private running = 0
  private scope = ''
  private refreshing = new Set<string>()
  /** Sem autorização ou sem a RPC: os ícones não aparecem e nada mais é pedido por conta própria. */
  blocked = false
  /** Muda a cada troca de Hub, Space ou principal (um hello novo): as views releem com ela. */
  epoch = 0
  private readonly source: RoutingSource
  constructor(source: RoutingSource) {
    super()
    this.source = source
    // Reconectar limpa as leituras; avisa as views para pedirem de novo, mesmo sem outra mudança na conversa.
    source.subscribe(() => { if (this.sync()) this.changed() })
  }

  private context(botId: string) {
    return { botId, hubId: this.source.hello?.hostId, spaceId: this.source.spaceId ?? ROUTING_PERSONAL }
  }
  private sync() {
    const scope = `${this.source.hello?.hostId ?? ''}|${this.source.spaceId ?? ROUTING_PERSONAL}|${this.source.principalEpoch}`
    if (scope === this.scope) return false
    this.scope = scope
    this.loads.clear()
    this.blocked = false
    this.epoch++
    return true
  }
  private key = (r: ReceiptRequest) => `${this.scope}|${r.botId}|${r.conversationId}|${r.sessionId}|${r.entryId}`

  get(request: ReceiptRequest | null): ReceiptLoad | undefined {
    if (!request) return undefined
    this.sync()
    return this.loads.get(this.key(request))
  }

  /** Para o ícone: lê uma vez por entrada. Não repete erro nem pede com o recibo bloqueado. */
  prefetch(request: ReceiptRequest) {
    this.sync()
    if (this.blocked || this.loads.has(this.key(request))) return
    this.fetch(request)
  }

  /** Ao abrir o recibo: relê o que ainda pode mudar (na fila) ou falhou. */
  open(request: ReceiptRequest) {
    this.sync()
    const load = this.loads.get(this.key(request))
    if (load?.status === 'loading') return
    // O uso do turno chega depois da resposta: sem ele (null), reabrir o recibo pergunta de novo, sem trocar o recibo
    // já na tela por "lendo".
    if (load?.status === 'loaded' && load.response.state !== 'pending') { if (load.response.usage === null) this.refresh(request); return }
    this.fetch(request)
  }

  /**
   * Relê em segundo plano um recibo já lido, sem passar por "lendo" (o ícone e o popover abertos não piscam). Serve ao uso
   * do turno, que o Hub grava ao fim do turno: uma resposta publicada antes disso foi lida sem ele.
   */
  refresh(request: ReceiptRequest) {
    this.sync()
    const key = this.key(request)
    if (this.blocked || this.refreshing.has(key) || this.loads.get(key)?.status !== 'loaded') return
    this.refreshing.add(key)
    const run = () => {
      // Pedido que esperava na fila de outro Hub/principal (ou com o recibo bloqueado) não sai.
      if (this.blocked || this.loads.get(key)?.status !== 'loaded') { this.refreshing.delete(key); this.queue.shift()?.(); return }
      this.running++
      readRoutingReceipt(this.source, this.context(request.botId), { conversationId: request.conversationId, sessionId: request.sessionId, entryId: request.entryId })
        .then((response) => {
          if (this.loads.get(key)?.status !== 'loaded') return
          this.loads.set(key, { status: 'loaded', response })
          this.changed()
        }, (error) => {
          // Sem autorização, sem a RPC ou contexto trocado: vale o mesmo que numa leitura normal, mas só no escopo
          // que fez o pedido; o erro de um Hub anterior não bloqueia o atual.
          if (this.loads.get(key)?.status === 'loaded' && blocking(errorCode(error))) { this.blocked = true; this.changed() }
        })
        .finally(() => { this.running--; this.refreshing.delete(key); this.queue.shift()?.() })
    }
    if (this.running < LIMIT) run()
    else this.queue.push(run)
  }

  private fetch(request: ReceiptRequest) {
    const key = this.key(request)
    this.loads.set(key, { status: 'loading' })
    this.changed()
    const run = () => {
      // Pedido de outro Hub/principal que esperava na fila: não sai.
      if (this.loads.get(key)?.status !== 'loading') { this.queue.shift()?.(); return }
      this.running++
      readRoutingReceipt(this.source, this.context(request.botId), { conversationId: request.conversationId, sessionId: request.sessionId, entryId: request.entryId })
        .then((response): ReceiptLoad => ({ status: 'loaded', response }), (error): ReceiptLoad => ({ status: 'error', code: errorCode(error) }))
        .then((load) => {
          if (this.loads.get(key)?.status !== 'loading') return
          this.loads.set(key, load)
          if (load.status === 'error' && blocking(load.code)) this.blocked = true
          this.changed()
        })
        .finally(() => { this.running--; this.queue.shift()?.() })
    }
    if (this.running < LIMIT) run()
    else this.queue.push(run)
  }
}

const caches = new WeakMap<object, ReceiptCache>()
export function receiptsFor(source: RoutingSource): ReceiptCache {
  let cache = caches.get(source)
  if (!cache) caches.set(source, cache = new ReceiptCache(source))
  return cache
}
