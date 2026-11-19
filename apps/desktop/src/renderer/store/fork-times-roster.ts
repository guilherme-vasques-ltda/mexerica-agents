import { parseTimesSnapshot, timesFailure, type TimesSnapshot } from '../../shared/fork-times.ts'
import { Observable } from '../lib/observable.ts'

/** Leitura salva da sidebar: nenhum rascunho, assinatura adicional ou cache de grafo. */
export interface TimesRosterClient {
  call(method: string, body?: unknown): Promise<unknown>
}
export interface TimesRosterOrigin {
  contextId: string
  computerId: string
  hostId: string
  spaceId: string
  client: TimesRosterClient
  epoch: number
}
export type TimesRosterPhase = 'loading' | 'ready' | 'offline' | 'error' | 'owner'
export const timesOriginKey = (o: Pick<TimesRosterOrigin, 'contextId' | 'computerId' | 'hostId' | 'spaceId'>) =>
  JSON.stringify([o.contextId, o.computerId, o.hostId, o.spaceId])

export class TimesRoster extends Observable {
  snapshot: TimesSnapshot | null = null
  phase: TimesRosterPhase = 'loading'
  readAt: number | null = null
  /** Invalida também a escolha de filtro; ordem/fold persistentes continuam por identidade. */
  scope = 0
  private identity: string | null = null
  private generation = 0
  private readSeq = 0
  private pending = false
  private busy = false
  private retired = false
  private timer: ReturnType<typeof setTimeout> | null = null

  private readonly source: () => TimesRosterOrigin | null

  constructor(source: () => TimesRosterOrigin | null) { super(); this.source = source }

  /** Chamado DEPOIS de aplicar hello/origem/epoch. Cada hello força nova leitura. */
  sync(event: { type: string; spaceId?: string } = { type: 'connection' }, unauthorized = false) {
    if (this.retired) return
    const origin = this.source()
    if (unauthorized) {
      this.clear('owner')
      return
    }
    if (!origin) {
      this.generation++
      this.cancelPending()
      this.phase = 'offline'
      this.changed()
      return
    }
    const key = timesOriginKey(origin)
    const moved = this.identity !== key
    if (moved) {
      this.clear('loading')
      this.identity = key
    }
    if (event.type === 'times' && event.spaceId !== origin.spaceId) return
    if (moved || ['hello', 'times', 'bot', 'botDeleted', 'resync', 'retry'].includes(event.type)) this.invalidate()
  }

  get domain() { return this.identity }

  retry() { this.sync({ type: 'retry' }) }

  suspend() {
    if (this.retired) return
    this.generation++
    this.cancelPending()
    this.phase = 'offline'
    this.changed()
  }

  /** Logout/retire/troca do Hub nunca deixam títulos ou resposta pendente sobreviverem. */
  clear(phase: TimesRosterPhase = 'loading') {
    this.generation++
    this.cancelPending()
    this.identity = null
    this.scope++
    this.snapshot = null
    this.readAt = null
    this.phase = phase
    this.changed()
  }

  retire() { this.clear('offline'); this.retired = true }

  private cancelPending() {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.pending = false
  }

  private invalidate() {
    this.generation++
    this.pending = true
    // No máximo uma chamada em voo e uma invalidação pendente para toda a rajada.
    if (!this.busy && !this.timer) this.timer = setTimeout(() => {
      this.timer = null
      void this.read()
    }, 250)
  }

  private same(origin: TimesRosterOrigin) {
    const current = this.source()
    return !this.retired && !!current && this.identity === timesOriginKey(origin)
      && timesOriginKey(current) === timesOriginKey(origin) && current.client === origin.client && current.epoch === origin.epoch
  }

  private async read() {
    const origin = this.source()
    if (!origin || this.retired || !this.pending || this.busy) return
    const gen = this.generation, seq = ++this.readSeq
    this.pending = false
    this.busy = true
    if (!this.snapshot) this.phase = 'loading'
    this.changed()
    try {
      const value = await origin.client.call('donnaTimes', { spaceId: origin.spaceId })
      if (!this.same(origin) || gen !== this.generation || seq !== this.readSeq) return
      const snapshot = parseTimesSnapshot(value, origin.spaceId)
      // Catálogo pode mudar sem revTimes: não tratar igualdade de revisão como igualdade de conteúdo.
      if (this.snapshot && snapshot.revision < this.snapshot.revision) throw new Error('stale_times_snapshot')
      this.snapshot = snapshot
      this.readAt = Date.now()
      this.phase = 'ready'
    } catch (error) {
      if (!this.same(origin)) return
      if (timesFailure(error) === 'owner') {
        this.clear('owner')
      } else if (gen === this.generation && seq === this.readSeq) {
        this.phase = 'error'
      }
    } finally {
      this.busy = false
      this.changed()
      if (this.pending && !this.retired && !this.timer) this.timer = setTimeout(() => {
        this.timer = null
        void this.read()
      }, 250)
    }
  }
}
