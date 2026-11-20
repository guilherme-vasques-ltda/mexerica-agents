import type { ForkCloudCallResult, ForkCloudStreamError } from './fork-cloud.ts'
import type { BrowserScope } from './fork-browser.ts'
import type { BrowserFrame } from './relay-protocol/browser-frames.ts'

/**
 * Cliente do navegador ao vivo do bot (fatia 2, B2): consome os quadros binários do canal protegido
 * (`browserStream*`, docs/reference/remote-relay.md "Quadros binários do navegador") e manda a entrada
 * da pessoa. Isomórfico: o mesmo código roda no renderer do desktop e na web; só a ponte muda.
 *
 * O Hub é a fonte da verdade (autoridade, escopo, época, geração): aqui só se monta o que ele pede e
 * se fecha tudo quando ele recusa ou o canal cai. Nada é repetido sozinho depois de um resultado incerto.
 */

/** Quadro com mais que isso fica bloqueado para entrada (proposta de telas, "Conexão lenta"). */
export const STALE_AFTER_MS = 6_000
const CALL_TIMEOUT_MS = 10_000
const MAX_PENDING_INPUTS = 4

export type BrowserController = 'agent' | 'person'

/** Por que a tela fechou. Códigos estáveis; a interface escolhe o texto. */
export type LiveClosedReason =
  /** O Hub não aceitou a capacidade de quadros (Hub ou app sem suporte). */
  | 'unsupported'
  /** O Hub recusou o escopo: aparelho revogado, bot de outro Space, outro dono. */
  | 'denied'
  /** O canal caiu, a conta trocou ou a conexão foi encerrada. */
  | 'connectionLost'
  /** O Hub respondeu que o fluxo não está disponível para esta sessão. */
  | 'unavailable'
  /** Fechada por quem usa a tela. */
  | 'closed'

export interface LiveState {
  status: 'connecting' | 'live' | 'closed'
  reason: LiveClosedReason | null
  controller: BrowserController
  epoch: number
  target: string
  targetGeneration: number
  /** Há mudança de controle em curso: a entrada só segue depois da confirmação do Hub. */
  switching: boolean
}

export interface FramesHandle {
  cancel(): void
  /** O quadro entregue já foi mostrado: libera o próximo (o intermediário foi descartado). */
  consumed(): void
  /** `true` quando a assinatura local foi aceita; `false` se foi recusada, encerrada ou cancelada antes de abrir. */
  opened: Promise<boolean>
}

/** A ponte presa a UMA conexão (`window.codync.forkCloud` ou a ponte web, com o id da conexão já aplicado). */
export interface BrowserLiveBridge {
  call(method: string, body: unknown, timeoutMs: number): Promise<ForkCloudCallResult>
  /** `waitedMs`: quanto o quadro já esperou no cliente (fila do desktop/web) desde que chegou, medido só como duração. */
  subscribeFrames(onFrame: (frame: BrowserFrame, waitedMs: number) => void, onEnd: (error: ForkCloudStreamError | null) => void): FramesHandle
}

export type InputResult =
  | { ok: true }
  | { ok: false; reason: 'notLive' | 'notPerson' | 'noFrame' | 'stale' | 'busy' | 'refused' | 'unknown' | 'invalid' }

export interface LiveListener {
  onState(state: LiveState): void
  /** O quadro novo, já validado contra sessão, alvo e geração. Quem desenha chama `frameShown` com o resultado. */
  onFrame(frame: BrowserFrame): void
}

const record = (value: unknown): Record<string, unknown> | null => (value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null)
const counter = (value: unknown) => (typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : null)
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)

export function subscribeBody(scope: BrowserScope, session: string, target: string) {
  if (!scope.hubId || !scope.spaceId || !scope.botId || !session || !target) throw new Error('browser_context_required')
  return { hubId: scope.hubId, spaceId: scope.spaceId, botId: scope.botId, session, target }
}

/**
 * Entrega um quadro por vez à tela e guarda só o mais recente enquanto ela ainda mostra o anterior:
 * cliente lento descarta os intermediários e a fila nunca passa de um quadro. O Hub faz o mesmo do lado dele.
 * A chegada é marcada em `offer`, antes da fila, e cada entrega leva o tempo já esperado (`waitedMs`): só duração,
 * porque processos Electron não compartilham a origem de `performance.now()`.
 */
export function createFrameGate<T>(deliver: (frame: T, waitedMs: number) => void, now: () => number = () => performance.now()) {
  let busy = false
  let pending: { frame: T; at: number } | null = null
  let dropped = 0
  return {
    offer(frame: T) {
      const at = now()
      if (busy) {
        if (pending !== null) dropped += 1
        pending = { frame, at }
        return
      }
      busy = true
      deliver(frame, 0)
    },
    consumed() {
      const next = pending
      pending = null
      if (next === null) {
        busy = false
        return
      }
      deliver(next.frame, Math.max(0, now() - next.at))
    },
    clear() {
      pending = null
      busy = false
    },
    get dropped() {
      return dropped
    },
    get queued() {
      return pending === null ? 0 : 1
    },
  }
}

/** Transições da ficha, para diagnóstico e prova: o que aconteceu, com qual sessão. */
export interface HubSlotEvent {
  event: 'begin' | 'confirm' | 'reject' | 'abandon' | 'superseded' | 'release'
  session: string
}

/**
 * O Hub guarda UMA inscrição de navegador por canal: inscrever outra sessão derruba a anterior (e uma sessão ainda
 * inscrita recusa uma segunda inscrição igual). A ficha é por conexão e diz qual sessão tem autoridade no Hub.
 *
 * Antes de mandar uma inscrição a autoridade da conexão inteira é suspensa (`begin`): como o Hub troca a inscrição
 * antes de responder e confirmação, controle e aba valem para a inscrição ativa, nenhum painel pode mandar comando
 * enquanto a troca está em curso. Só a confirmação positiva da tentativa ainda vigente (`confirm`) dá a autoridade à
 * sessão nova. Recusa explícita do Hub (`reject`, que nunca troca a inscrição) devolve ao anterior, se ele era o único.
 * Resposta incerta ou tentativa já substituída (`abandon`) não devolve nada: ninguém tem autoridade até alguém confirmar.
 */
export class BrowserHubSlot {
  active: BrowserLiveSession | null = null
  readonly log: HubSlotEvent[] = []
  private attempt: object | null = null
  private restore: BrowserLiveSession | null = null
  private readonly displaced = new Set<BrowserLiveSession>()

  private note(event: HubSlotEvent['event'], session: BrowserLiveSession) {
    this.log.push({ event, session: session.id })
    if (this.log.length > 64) this.log.shift()
  }

  /** Suspende a autoridade na conexão inteira e abre a tentativa de `session`. */
  begin(session: BrowserLiveSession): object {
    const previous = this.active
    this.restore = this.attempt === null ? previous : null
    if (previous) this.displaced.add(previous)
    this.active = null
    this.attempt = {}
    this.note('begin', session)
    return this.attempt
  }

  isCurrent(attempt: object) {
    return this.attempt === attempt
  }

  /** Confirmação positiva da tentativa vigente: `session` passa a ter a autoridade e as derrubadas se desligam. */
  confirm(attempt: object, session: BrowserLiveSession): boolean {
    if (this.attempt !== attempt) return false
    this.attempt = null
    this.restore = null
    this.active = session
    this.note('confirm', session)
    this.dropDisplaced(session)
    return true
  }

  /** Recusa explícita: o Hub não trocou a inscrição, então o anterior (se era o único) volta a ter autoridade. */
  reject(attempt: object, session: BrowserLiveSession) {
    if (this.attempt !== attempt) return this.note('superseded', session)
    this.attempt = null
    const back = this.restore
    this.restore = null
    this.note('reject', session)
    if (back && this.displaced.has(back) && back !== session) {
      this.displaced.delete(back)
      this.active = back
    }
    this.dropDisplaced(session)
  }

  /** Resposta incerta ou fora de hora: o Hub pode ter trocado a inscrição, então ninguém recupera a autoridade. */
  abandon(attempt: object, session: BrowserLiveSession) {
    if (this.attempt !== attempt) return this.note('superseded', session)
    this.attempt = null
    this.restore = null
    this.note('abandon', session)
    this.dropDisplaced(session)
  }

  release(session: BrowserLiveSession) {
    if (this.active === session) this.active = null
    if (this.restore === session) this.restore = null
    this.displaced.delete(session)
    this.note('release', session)
  }

  private dropDisplaced(except: BrowserLiveSession) {
    const gone = [...this.displaced].filter((session) => session !== except)
    this.displaced.clear()
    for (const session of gone) session.hubReplaced()
  }
}

export class BrowserLiveSession {
  private readonly bridge: BrowserLiveBridge
  private readonly scope: BrowserScope
  private readonly session: string
  private readonly now: () => number
  private readonly hub: BrowserHubSlot
  private readonly receivedAt = new WeakMap<BrowserFrame, number>()
  private readonly listeners = new Set<LiveListener>()
  private handle: FramesHandle | null = null
  private current: LiveState
  private displayed: BrowserFrame | null = null
  private lastSequence = 0
  private lastFrameAt = 0
  private attachId = 0
  private inputSequence = 0
  private lostInput = false
  private pendingInputs = 0
  private inputChain: Promise<unknown> = Promise.resolve()
  private acking = false
  private ackAgain = false
  private paused = false
  private started = false

  /** `now` é um relógio monotônico em ms (padrão `performance.now`): a validade do quadro não depende do relógio de parede. */
  constructor(bridge: BrowserLiveBridge, scope: BrowserScope, session: string, target: string, now: () => number = () => performance.now(), hub: BrowserHubSlot = new BrowserHubSlot()) {
    this.bridge = bridge
    this.scope = scope
    this.session = session
    this.now = now
    this.hub = hub
    this.current = { status: 'connecting', reason: null, controller: 'agent', epoch: 0, target, targetGeneration: 0, switching: false }
  }

  get id(): string {
    return this.session
  }

  get state(): LiveState {
    return this.current
  }

  get frame(): BrowserFrame | null {
    return this.displayed
  }

  /** Idade do quadro pintado desde que ele CHEGOU (o tempo de decodificação conta), em ms; `null` enquanto não há quadro. */
  frameAgeMs(): number | null {
    return this.displayed === null ? null : Math.max(0, this.now() - this.lastFrameAt)
  }

  /** Sem quadro ou com quadro velho demais: a entrada fica bloqueada até chegar um novo. */
  isStale(): boolean {
    const age = this.frameAgeMs()
    return age === null || age > STALE_AFTER_MS
  }

  subscribe(listener: LiveListener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private set(patch: Partial<LiveState>) {
    this.current = { ...this.current, ...patch }
    for (const listener of [...this.listeners]) listener.onState(this.current)
  }

  /** Fecha tudo: assinatura de quadros, fila de entrada, quadro mostrado. Idempotente. */
  close(reason: LiveClosedReason = 'closed') {
    if (this.current.status === 'closed') return
    const handle = this.handle
    this.handle = null
    this.attachId += 1
    this.displayed = null
    this.pendingInputs = 0
    this.hub.release(this)
    try {
      handle?.cancel()
    } catch {}
    this.set({ status: 'closed', reason, switching: false })
  }

  /** Outra sessão se inscreveu no Hub nesta conexão e derrubou esta inscrição. Com o painel fechado, ela se reinscreve ao reabrir. */
  hubReplaced() {
    if (this.current.status === 'closed' || this.paused) return
    this.close('unavailable')
  }

  private owns() {
    return this.hub.active === this
  }

  /**
   * Liga o consumidor local dos quadros. A inscrição local abre antes de qualquer chamada ao Hub: sem ela
   * (Hub ou app sem a capacidade) nada é inscrito do outro lado. `false` quando falhou, foi trocada ou a tela fechou.
   */
  private async attach(): Promise<boolean> {
    const id = ++this.attachId
    const handle = this.bridge.subscribeFrames(
      (frame, waitedMs) => {
        if (id === this.attachId) this.accept(frame, waitedMs)
      },
      (error) => {
        if (id !== this.attachId) return
        this.handle = null
        this.close(error?.status === 501 ? 'unsupported' : error?.status === 401 || error?.status === 403 ? 'denied' : 'connectionLost')
      },
    )
    this.handle = handle
    const opened = await handle.opened
    if (id !== this.attachId || this.current.status === 'closed') {
      handle.cancel()
      return false
    }
    if (opened) return true
    this.close('connectionLost')
    return false
  }

  /** Inscreve a sessão no Hub. Qualquer recusa fecha a tela; nada fica pela metade. */
  async start(): Promise<void> {
    if (this.started || this.current.status !== 'connecting') return
    this.started = true
    if (!(await this.attach())) {
      if (this.current.status !== 'connecting') return
      this.started = false
      // Painel fechado e reaberto durante a abertura local: aquela abertura foi invalidada, então abre de novo.
      if (!this.paused) await this.start()
      return
    }
    await this.subscribeAtHub()
  }

  /**
   * Inscreve a sessão no Hub, que troca a inscrição anterior desta conexão. Só depois de o Hub confirmar ESTA sessão
   * a tela fica ao vivo e libera controle, confirmação e entrada. Qualquer recusa fecha a tela.
   */
  private async subscribeAtHub(): Promise<void> {
    const body = subscribeBody(this.scope, this.session, this.current.target)
    // A autoridade da conexão inteira fica suspensa daqui até a resposta: o Hub troca a inscrição antes de responder.
    const attempt = this.hub.begin(this)
    const result: ForkCloudCallResult = await this.bridge.call('browserStreamSubscribe', body, CALL_TIMEOUT_MS).catch(() => ({ kind: 'unknown', reason: 'closed' }) as const)
    const ok = result.kind === 'ok' ? record(result.value) : null
    const confirmed = ok?.subscribed === true && ok.frames === true && ok.session === this.session
    if (!this.hub.isCurrent(attempt)) {
      this.hub.abandon(attempt, this)
      if (this.current.status === 'connecting') this.close('unavailable')
      return
    }
    // O Hub recusa antes de trocar a inscrição: só a recusa explícita é conhecida como "não trocou".
    if (result.kind === 'err' || result.kind === 'notSent') this.hub.reject(attempt, this)
    else if (!confirmed || this.current.status !== 'connecting') this.hub.abandon(attempt, this)
    if (this.current.status !== 'connecting') return
    if (confirmed) {
      this.hub.confirm(attempt, this)
      this.set({ status: 'live' })
      if (!this.paused && !this.handle && (await this.attach())) this.ack()
      return
    }
    if (ok) return this.close('unsupported')
    this.close(this.failure(result))
  }

  private failure(result: ForkCloudCallResult): LiveClosedReason {
    if (result.kind === 'err') return result.status === 403 || result.status === 401 ? 'denied' : 'unavailable'
    return 'connectionLost'
  }

  private accept(frame: BrowserFrame, waitedMs: number) {
    // O Hub já confere, mas quadro de outra sessão, alvo velho ou sequência antiga nunca chega à tela.
    if (this.current.status === 'closed') return
    if (this.paused) {
      this.handle?.consumed()
      return
    }
    if (frame.session !== this.session || frame.target !== this.current.target || frame.targetGeneration !== this.current.targetGeneration || frame.sequence <= this.lastSequence) {
      this.handle?.consumed()
      return
    }
    this.lastSequence = frame.sequence
    // A chegada no cliente vem de antes da fila do desktop/web: o tempo esperado lá conta, e um valor inválido conta como vencido.
    this.receivedAt.set(frame, this.now() - (Number.isFinite(waitedMs) && waitedMs >= 0 ? waitedMs : Number.POSITIVE_INFINITY))
    for (const listener of [...this.listeners]) listener.onFrame(frame)
  }

  /**
   * A tela terminou de tratar o quadro: libera o próximo e confirma ao Hub (vazão adaptativa).
   * Só um quadro realmente pintado (`painted`) vira o quadro de referência da entrada; o prazo de 6 s conta desde a
   * CHEGADA dele (decodificar e pintar gasta o prazo, não o reinicia). Se o desenho falhou, a tela limpa o canvas e a
   * entrada fica sem quadro até chegar outro.
   */
  frameShown(frame: BrowserFrame, painted: boolean) {
    this.handle?.consumed()
    const received = this.receivedAt.get(frame)
    if (!painted) this.displayed = null
    else if (received !== undefined && this.current.status !== 'closed' && !this.paused && frame.session === this.session && frame.target === this.current.target && frame.targetGeneration === this.current.targetGeneration && (this.displayed === null || frame.sequence > this.displayed.sequence)) {
      this.displayed = frame
      this.lastFrameAt = received
    }
    this.ack()
  }

  /**
   * Painel fechado, sessão mantida: o Hub não tem como cancelar a inscrição, então ela fica enquanto nenhuma outra
   * sessão assumir a conexão. O consumidor local sai, e com ele o slot único de quadros da conexão, para outra
   * sessão poder abrir. Sem quadro na memória, sem desenho, sem confirmação.
   */
  pause() {
    this.paused = true
    this.attachId += 1
    this.displayed = null
    const handle = this.handle
    this.handle = null
    try {
      handle?.cancel()
    } catch {}
  }

  /**
   * Painel aberto de novo. Se o Hub ainda tem esta sessão inscrita, religa só o consumidor local e pede o próximo
   * quadro. Se outra sessão da conexão a derrubou, inscreve de novo e recomeça do zero (controle com o bot, época e
   * geração novas), sem liberar nada antes de o Hub confirmar esta sessão.
   */
  async resume(): Promise<void> {
    if (!this.paused || this.current.status === 'closed') return
    this.paused = false
    if (this.current.status === 'connecting') return this.start()
    if (!(await this.attach())) return
    if (this.owns()) return this.ack()
    this.displayed = null
    this.lastSequence = 0
    this.inputSequence = 0
    this.lostInput = false
    this.pendingInputs = 0
    this.set({ status: 'connecting', controller: 'agent', epoch: 0, targetGeneration: 0, switching: false })
    await this.subscribeAtHub()
  }

  private ack() {
    if (this.current.status !== 'live' || !this.owns()) return
    if (this.acking) {
      this.ackAgain = true
      return
    }
    this.acking = true
    void this.bridge
      .call('browserStreamAck', {}, CALL_TIMEOUT_MS)
      .catch(() => null)
      .finally(() => {
        this.acking = false
        if (!this.ackAgain) return
        this.ackAgain = false
        this.ack()
      })
  }

  /** Muda quem controla. A pessoa só entra depois que o Hub confirma a nova época. */
  async setController(controller: BrowserController): Promise<boolean> {
    if (this.current.status !== 'live' || !this.owns() || this.current.switching) return false
    this.set({ switching: true })
    const result = await this.bridge.call('browserStreamControl', { controller }, CALL_TIMEOUT_MS)
    if (this.current.status !== 'live') return false
    const epoch = result.kind === 'ok' ? counter(record(result.value)?.epoch) : null
    if (epoch === null) {
      if (result.kind === 'err' && (result.status === 403 || result.status === 401)) this.close('denied')
      else if (result.kind === 'unknown' || result.kind === 'notSent') this.close('connectionLost')
      else this.set({ switching: false })
      return false
    }
    this.pendingInputs = 0
    this.set({ controller, epoch, switching: false })
    return true
  }

  /** Troca de aba: a geração avança e o quadro e a entrada da anterior são descartados. */
  async setTarget(target: string): Promise<boolean> {
    if (this.current.status !== 'live' || !this.owns() || !target || target.length > 256) return false
    const result = await this.bridge.call('browserStreamTarget', { target }, CALL_TIMEOUT_MS)
    if (this.current.status !== 'live') return false
    const generation = result.kind === 'ok' ? counter(record(result.value)?.targetGeneration) : null
    if (generation === null) {
      if (result.kind === 'err' && (result.status === 403 || result.status === 401)) this.close('denied')
      else if (result.kind === 'unknown' || result.kind === 'notSent') this.close('connectionLost')
      return false
    }
    this.displayed = null
    this.lastSequence = 0
    this.pendingInputs = 0
    this.set({ target, targetGeneration: generation })
    return true
  }

  /**
   * Clique da pessoa. As coordenadas são relativas à caixa inteira em que o quadro está desenhado
   * (`displayWidth`/`displayHeight`); o Hub desconta as margens e converte para a viewport.
   * Só com a pessoa no controle, quadro recente e sessão viva; as entradas saem em ordem.
   */
  click(x: number, y: number, displayWidth: number, displayHeight: number): Promise<InputResult> {
    const frame = this.displayed
    if (this.current.status !== 'live' || !this.owns()) return Promise.resolve({ ok: false, reason: 'notLive' })
    if (this.current.controller !== 'person' || this.current.switching) return Promise.resolve({ ok: false, reason: 'notPerson' })
    if (!frame) return Promise.resolve({ ok: false, reason: 'noFrame' })
    if (this.isStale()) return Promise.resolve({ ok: false, reason: 'stale' })
    if (![x, y, displayWidth, displayHeight].every(finite) || displayWidth <= 0 || displayHeight <= 0) return Promise.resolve({ ok: false, reason: 'invalid' })
    if (this.pendingInputs >= MAX_PENDING_INPUTS) return Promise.resolve({ ok: false, reason: 'busy' })
    this.pendingInputs += 1
    const epoch = this.current.epoch
    const generation = this.current.targetGeneration
    const run = this.inputChain.then(() => this.send(frame, epoch, generation, x, y, displayWidth, displayHeight))
    this.inputChain = run.catch(() => null)
    return run.finally(() => {
      this.pendingInputs = Math.max(0, this.pendingInputs - 1)
    })
  }

  /**
   * A entrada só sai se, neste instante, o quadro em que a pessoa clicou ainda for o que está pintado, em dia, com a
   * mesma época, aba e pessoa no controle. Vale para o envio depois da fila e para a nova tentativa.
   */
  private blockedInput(frame: BrowserFrame, epoch: number, generation: number): InputResult | null {
    const { status, epoch: nowEpoch, targetGeneration, controller, switching } = this.current
    if (status !== 'live' || !this.owns() || nowEpoch !== epoch || targetGeneration !== generation || controller !== 'person' || switching || this.displayed !== frame) return { ok: false, reason: 'refused' }
    return this.isStale() ? { ok: false, reason: 'stale' } : null
  }

  private async send(frame: BrowserFrame, epoch: number, generation: number, x: number, y: number, displayWidth: number, displayHeight: number): Promise<InputResult> {
    const attempt = async (sequence: number) =>
      this.bridge.call('browserStreamInput', { session: this.session, target: this.current.target, frameSequence: frame.sequence, targetGeneration: generation, sequence, epoch, controller: 'person', x, y, displayWidth, displayHeight }, CALL_TIMEOUT_MS)
    let blocked = this.blockedInput(frame, epoch, generation)
    if (blocked) return blocked
    let sequence = this.inputSequence + 1
    let result = await attempt(sequence)
    let retried = false
    const accepted = (value: ForkCloudCallResult) => value.kind === 'ok' && record(value.value)?.accepted === true
    // Depois de um resultado incerto o Hub pode ter avançado a sequência: uma única nova tentativa com a seguinte.
    if (this.lostInput && result.kind === 'ok' && !accepted(result) && this.current.status === 'live') {
      blocked = this.blockedInput(frame, epoch, generation)
      if (blocked) return blocked
      sequence += 1
      retried = true
      result = await attempt(sequence)
    }
    if (accepted(result)) {
      this.inputSequence = sequence
      this.lostInput = false
      return { ok: true }
    }
    if (result.kind === 'err') {
      if (result.status === 403 || result.status === 401) this.close('denied')
      return { ok: false, reason: 'refused' }
    }
    if (result.kind === 'ok') return { ok: false, reason: 'refused' }
    if (result.kind === 'notSent') return { ok: false, reason: 'refused' }
    this.lostInput = true
    // Incerteza também na nova tentativa: o Hub pode estar em qualquer uma das duas sequências e repetir os mesmos números nunca mais o alcança; a tela fecha para reinscrever e ressincronizar.
    if (result.reason === 'closed' || retried) this.close('connectionLost')
    return { ok: false, reason: 'unknown' }
  }
}
