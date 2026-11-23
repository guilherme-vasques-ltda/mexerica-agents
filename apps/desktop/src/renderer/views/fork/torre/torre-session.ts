import { parseTowerSnapshot, type TowerSnapshot } from '../../../../shared/fork-torre.ts'
import { parseAttentionQueue, type AttentionQueue } from '../../../../shared/fork-torre-esperando.ts'
import { Observable } from '../../../lib/observable.ts'

// VG-02 (docs/features/mission-board.md, "Atualização no canal atual"): a fotografia do Mission Board de um Hub e space.
// Assina o stream do space ANTES de ler; relê em `donnaControlTowerChanged` e nos eventos legados autorizados,
// juntando avisos seguidos e relendo de novo se um chegar durante a leitura. Toda resposta carrega a origem (Hub,
// link, geração): resposta de outra origem é descartada antes de pintar. Sem link, a última fotografia fica só em
// memória, marcada como antiga. Outro Hub no mesmo computador apaga tudo. Nada é gravado em disco ou no mirror.

export interface TowerEvent { type: string; spaceId?: string; hubId?: string }

export interface TowerLink {
  call(method: string, body?: unknown, timeoutMs?: number): Promise<unknown>
  inSpace(spaceId: string): { events(since: number, client: string, onEvent: (e: TowerEvent) => void, onEnd: (error: unknown) => void): () => void }
}

export interface TowerSource {
  readonly hello: { hostId: string; rev: number } | null
  readonly connectedClient: TowerLink | null
  readonly principalEpoch: number
  subscribe(listener: () => void): () => void
}

/** Sinais para reler (nunca fonte do cartão). */
export const REREAD_EVENTS = new Set(['donnaControlTowerChanged', 'donnaAttentionChanged', 'donnaProjectsChanged', 'bot', 'botDeleted', 'conversation', 'entry', 'session', 'spaces', 'hello', 'resync'])
const REREAD_DELAY = 250
const RELISTEN_MIN = 2_000
const RELISTEN_MAX = 30_000

export type TowerPhase =
  | { kind: 'loading' }
  | { kind: 'ready' }
  /** Sem fotografia para mostrar: sem conexão desde o início, Hub antigo, sem acesso ou resposta inválida. */
  | { kind: 'error'; codigo: TowerError }
  /** O computador passou a ser outro Hub: a sessão para. */
  | { kind: 'moved' }

/** DON-917 (D507): a falha e o aviso são códigos; a tela escolhe a frase (`torre.erro.*`, `torre.aviso.*`). */
export type TowerError = 'semMetodo' | 'semAcesso' | 'invalido' | 'leitura' | 'semConexao'
export type TowerNotice = 'reread'
const LOST_ACCESS: TowerError = 'semAcesso'

interface Origin { source: TowerSource; hostId: string; client: TowerLink; epoch: number; gen: number }

/** O código da falha, pelo status do Hub (`api::error_status`): 404 método ausente, 403 sem acesso. */
export function towerErrorText(e: unknown): TowerError {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
  if (status === 404 || /unknown method/i.test(message)) return 'semMetodo'
  if (status === 403 || status === 401) return LOST_ACCESS
  if (/torre_invalid_answer/.test(message)) return 'invalido'
  return 'leitura'
}

export class TowerSession extends Observable {
  phase: TowerPhase = { kind: 'loading' }
  link: 'live' | 'offline' = 'live'
  /** Os avisos deste space pararam (só o stream); a sessão tenta de novo sozinha. */
  events: 'live' | 'paused' = 'live'
  snapshot: TowerSnapshot | null = null
  attention: AttentionQueue | null = null
  /** Quando a fotografia atual deixou de ser atualizável (sem conexão). */
  offlineSince: number | null = null
  notice: TowerNotice | null = null

  private origin: Origin | null = null
  private hostId: string | null = null
  private spaceId: string | null = null
  private gen = 0
  private reads = 0
  private reading = false
  private dirty = false
  private unwatch: (() => void) | null = null
  private stopEvents: (() => void) | null = null
  private rereadTimer: ReturnType<typeof setTimeout> | null = null
  private relistenTimer: ReturnType<typeof setTimeout> | null = null
  private relistenDelay = RELISTEN_MIN

  private now: () => number
  private readonly mode: 'tower' | 'badge'

  constructor(now: () => number = Date.now, mode: 'tower' | 'badge' = 'tower') {
    super()
    this.now = now
    this.mode = mode
  }

  get space() { return this.spaceId }
  /** A fotografia só vale como “agora” com link e eventos vivos. */
  get stale() { return this.link === 'offline' || this.events === 'paused' }

  currentAttention(source: TowerSource, spaceId: string) {
    const origin = this.origin
    return !this.stale && origin?.source === source && this.spaceId === spaceId && this.current(origin) ? this.attention : null
  }

  open(source: TowerSource, spaceId: string) {
    this.release()
    if (this.spaceId !== spaceId) this.forget()
    this.spaceId = spaceId
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  close() {
    this.release()
    this.gen++
    this.origin = null
    this.changed()
  }

  reread() {
    const origin = this.origin
    if (origin && this.current(origin)) void this.read(origin)
  }

  private connect(source: TowerSource) {
    this.stopEvents?.()
    this.stopEvents = null
    const gen = ++this.gen
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    this.attention = null
    if (hostId && this.hostId && this.hostId !== hostId) this.forget()
    if (hostId) this.hostId = hostId
    if (!hostId || !client || !this.spaceId) {
      this.origin = null
      if (this.link === 'live') this.offlineSince = this.now()
      this.link = 'offline'
      if (!this.snapshot) this.phase = { kind: 'error', codigo: 'semConexao' }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, gen }
    this.origin = origin
    this.link = 'live'
    this.offlineSince = null
    this.events = 'live'
    this.relistenDelay = RELISTEN_MIN
    if (!this.snapshot) this.phase = { kind: 'loading' }
    this.changed()
    // Primeiro a inscrição, depois a leitura: um aviso entre as duas não se perde.
    this.listen(origin, this.spaceId)
    void this.read(origin)
  }

  private listen(origin: Origin, spaceId: string) {
    this.stopEvents = origin.client.inSpace(spaceId).events(origin.source.hello?.rev ?? 0, 'desktop', (e) => {
      if (!this.current(origin)) return
      this.relistenDelay = RELISTEN_MIN
      if (!REREAD_EVENTS.has(e.type)) return
      const scoped = e.type === 'donnaAttentionChanged'
      if (scoped && (e.spaceId !== spaceId || e.hubId !== origin.hostId)) return
      if (!scoped && e.spaceId !== undefined && e.spaceId !== spaceId) return
      if (!scoped && e.hubId !== undefined && e.hubId !== origin.hostId) return
      this.scheduleRead(origin)
    }, () => {
      if (!this.current(origin)) return
      this.events = 'paused'
      this.attention = null
      if (this.offlineSince === null) this.offlineSince = this.now()
      this.changed()
      if (this.relistenTimer) clearTimeout(this.relistenTimer)
      this.relistenTimer = setTimeout(() => {
        this.relistenTimer = null
        if (!this.current(origin)) return
        this.events = 'live'
        this.offlineSince = null
        this.stopEvents?.()
        this.listen(origin, spaceId)
        void this.read(origin)
      }, this.relistenDelay)
      this.relistenDelay = Math.min(RELISTEN_MAX, this.relistenDelay * 2)
    })
  }

  /** A store mudou: outro Hub encerra; link novo do mesmo Hub relê; sem link, só a última fotografia, como antiga. */
  private watch(source: TowerSource) {
    const origin = this.origin
    const hostId = source.hello?.hostId ?? null
    if (this.hostId && hostId && hostId !== this.hostId) {
      this.release()
      this.gen++
      this.origin = null
      this.forget()
      this.phase = { kind: 'moved' }
      this.changed()
      return
    }
    const client = source.connectedClient
    if (!client) {
      if (this.link === 'live') this.connect(source)
      return
    }
    if (!origin || origin.client !== client || origin.epoch !== source.principalEpoch || this.link === 'offline') this.connect(source)
  }

  private scheduleRead(origin: Origin) {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = setTimeout(() => {
      this.rereadTimer = null
      if (this.current(origin)) void this.read(origin)
    }, REREAD_DELAY)
  }

  private current(origin: Origin) {
    const { source } = origin
    return this.origin === origin && this.gen === origin.gen && source.hello?.hostId === origin.hostId
      && source.connectedClient === origin.client && source.principalEpoch === origin.epoch
  }

  private forget() {
    this.snapshot = null
    this.attention = null
    this.notice = null
    this.offlineSince = null
    this.phase = { kind: 'loading' }
  }

  private release() {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = null
    if (this.relistenTimer) clearTimeout(this.relistenTimer)
    this.relistenTimer = null
    this.stopEvents?.()
    this.stopEvents = null
    this.unwatch?.()
    this.unwatch = null
  }

  private async read(origin: Origin) {
    // Uma invalidação durante a leitura pede outra leitura depois desta.
    if (this.reading) {
      this.dirty = true
      return
    }
    this.reading = true
    this.dirty = false
    const read = ++this.reads
    const spaceId = this.spaceId!
    try {
      if (this.mode === 'badge') {
        const answer = await origin.client.call('donnaAttention', { spaceId, includeResolvedToday: false })
        if (!this.current(origin) || read !== this.reads) return
        this.attention = parseAttentionQueue(answer, { hubId: origin.hostId, spaceId })
        return
      }
      const answer = await origin.client.call('donnaControlTower', { hubId: origin.hostId, spaceId })
      if (!this.current(origin) || read !== this.reads) return
      this.snapshot = parseTowerSnapshot(answer, { hubId: origin.hostId, spaceId })
      this.phase = { kind: 'ready' }
      this.notice = null
    } catch (e) {
      if (!this.current(origin) || read !== this.reads) return
      this.attention = null
      const message = towerErrorText(e)
      // Perder o acesso remove a projeção, não só desabilita os botões.
      if (!this.snapshot || message === LOST_ACCESS) {
        this.snapshot = null
        this.phase = { kind: 'error', codigo: message }
      } else {
        this.notice = 'reread'
      }
    } finally {
      this.reading = false
      if (this.current(origin)) this.changed()
      // A leitura pedida durante esta (mesmo de um link novo) acontece agora, na origem atual.
      const latest = this.origin
      if (this.dirty && latest && this.current(latest)) this.scheduleRead(latest)
    }
  }
}
