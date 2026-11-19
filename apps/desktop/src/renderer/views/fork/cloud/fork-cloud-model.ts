// D171/D163: a vista da janela sobre o acesso pela nuvem do processo principal (`window.codync.forkCloud`).
// O Main decide tudo: registro, diretório, vínculo A1/A2, pin, conexão e o anexo qualificado. Este
// modelo só lê o estado (ao iniciar e quando a conta muda) e envia as intenções que a pessoa pediu,
// uma de cada vez; não tem temporizador, não repete uma ação e não registra nem reivindica nada no login.
// A primeira vinculação (ACS4) segue o mesmo jeito: só o retrato `bootstrap` da resposta atual, e cada
// intenção só quando o Main a oferece (`canStart`/`canCheck`/`canProve`/`canReconcile`). Um pedido
// encerrado (`closed`, cancelado ou vencido) fica só como histórico: nunca é vínculo nem oferece ação.
import type { ForkBootstrapPhase, ForkBootstrapState, ForkCloudAttachment, ForkCloudComputer, ForkCloudIntent, ForkCloudResult, ForkCloudState } from '@shared/fork-cloud'
import { Observable } from '../../../lib/observable'

/** O `forkCloud` da ponte que o modelo usa. */
export interface ForkCloudBridge {
  invoke(intent: ForkCloudIntent): Promise<ForkCloudResult>
  onChange(cb: (result: ForkCloudResult) => void): () => void
}

/** A parte da sessão da conta (store/fork-account-session) que cerca as respostas. */
export interface ForkCloudAccount {
  /** Muda a cada troca de conta, origem ou configuração. */
  readonly sessionRevision: number
  subscribe(listener: () => void): () => void
}

export type Registration = ForkCloudResult['registration']
export type Recovery = 'registration' | 'enrollment' | null

/** O que a pessoa começou nesta janela, para a tela contar o passo certo; não é estado do Main. */
export interface Flow {
  /** `local`: pelo SSH já conectado (A1). `remote`: pedido ao dono (A2). `connect`: conexão de um Hub vinculado. */
  readonly kind: 'local' | 'remote' | 'connect'
  readonly computerId: string
  /** O nome real do Hub, ou null enquanto não há um (R1-01 do #596: nunca um nome provisório já traduzido). */
  readonly name: string | null
}

/** O que aconteceu com uma intenção enviada. */
export type Outcome = 'accepted' | 'notSent' | 'unconfirmed' | 'superseded'

/**
 * DON-917/D507: o aviso é um código; a tela escolhe a frase no idioma dela (`nuvem.aviso.*`).
 * `notSent` ainda depende do aparelho, escolhido pela tela.
 */
export type Notice = 'unconfirmed' | 'notSent' | 'expired' | 'noRegistration' | 'bootstrapNoEffect' | 'bootstrapSameStep'
export const UNCONFIRMED = 'unconfirmed' satisfies Notice
export const NOT_SENT = 'notSent' satisfies Notice
export const EXPIRED = 'expired' satisfies Notice
export const NO_REGISTRATION = 'noRegistration' satisfies Notice
export const BOOTSTRAP_NO_EFFECT = 'bootstrapNoEffect' satisfies Notice
export const BOOTSTRAP_SAME_STEP = 'bootstrapSameStep' satisfies Notice

const REGISTRATIONS: readonly string[] = ['unavailable', 'unregistered', 'registered']
/** Estados do pedido de vínculo: o único `attemptId` que `check`, `confirm` e `cancel` podem nomear. */
const ENROLLING: readonly string[] = ['linking', 'waitingOwnerApproval', 'linked']
const BOOTSTRAP_PHASES: readonly string[] = ['available', 'prepared', 'createUnknown', 'requested', 'openingUnknown', 'challengeUnknown', 'challenged', 'proofUnknown', 'receipt', 'reconcileUnknown', 'published', 'pinUnknown', 'complete', 'closed']
/** Por que um pedido encerrado acabou; outro motivo não é copiado, e o pedido continua encerrado. */
const TERMINAL_REASONS: readonly string[] = ['canceled', 'expired']
/** As intenções da primeira vinculação: enquanto ela bloqueia, o Main recusa as outras. */
const BOOTSTRAP_INTENTS: readonly string[] = ['startBootstrap', 'checkBootstrap', 'proveBootstrap', 'reconcileBootstrap']

/**
 * O retrato ACS4 do Main, só com os campos do contrato. Outra forma não oferece ação nenhuma; fora de
 * `available`, sem o identificador que o Main devolveu também não. `blocksOtherOperations` só é
 * copiado quando o Main o publica; a ausência não vira `false` (ver `bootstrapBlocks`). Encerrado
 * (`closed`) não oferece ação nenhuma, mesmo que o retrato diga o contrário.
 */
function bootstrapOf(raw: unknown): ForkBootstrapState | null {
  if (!raw || typeof raw !== 'object') return null
  const v = raw as Partial<Record<keyof ForkBootstrapState, unknown>>
  if (typeof v.bootstrapId !== 'string' || v.bootstrapId.length > 256 || !BOOTSTRAP_PHASES.includes(v.phase as string)) return null
  if (v.phase !== 'available' && !v.bootstrapId) return null
  if (![v.canStart, v.canCheck, v.canProve, v.canReconcile].every((flag) => typeof flag === 'boolean')) return null
  const closed = v.phase === 'closed'
  return {
    bootstrapId: v.bootstrapId,
    phase: v.phase as ForkBootstrapPhase,
    ...(typeof v.blocksOtherOperations === 'boolean' ? { blocksOtherOperations: v.blocksOtherOperations } : {}),
    canStart: !closed && v.canStart === true,
    canCheck: !closed && v.canCheck === true,
    canProve: !closed && v.canProve === true,
    canReconcile: !closed && v.canReconcile === true,
    ...(typeof v.computerId === 'string' && v.computerId ? { computerId: v.computerId } : {}),
    ...(typeof v.deadline === 'number' && Number.isFinite(v.deadline) ? { deadline: v.deadline } : {}),
    ...(v.readUnavailable === true ? { readUnavailable: true as const } : {}),
    ...(closed && TERMINAL_REASONS.includes(v.terminalReason as string) ? { terminalReason: v.terminalReason as 'canceled' | 'expired' } : {}),
  }
}

/** Um contexto de cache próprio da conta: nunca o da conexão direta (`local`), nunca vazio. */
const ownContext = (id: unknown): id is string => typeof id === 'string' && id.length > 0 && id.length <= 256 && id !== 'local' && !/[\x00-\x1f\x7f]/.test(id)

/** O nome que o Main aceita para um aparelho: 1 a 100 caracteres, sem caracteres de controle. */
export function deviceLabel(raw: string, fallback: string): string {
  const clean = raw.replace(/[\x00-\x1f\x7f]/g, '').trim().slice(0, 100).trim()
  return clean || fallback
}

export class ForkCloudModel extends Observable {
  registration: Registration = 'unavailable'
  state: ForkCloudState | null = null
  computers: readonly ForkCloudComputer[] = []
  /** O que o Main diz estar incerto (com `unknown`): `registration` só se confere com `reconcileRegistration`. */
  recovery: Recovery = null
  recoveryPhase: ForkCloudResult['recoveryPhase'] = null
  /** Como o Main publicou; a tela e o anexo usam só `readyAttachment`. */
  attachment: ForkCloudAttachment | null = null
  /** A primeira vinculação (ACS4) como o Main publicou; null quando ele não a oferece nesta versão ou conta. */
  bootstrap: ForkBootstrapState | null = null
  /** O Main não respondeu (sem serviço nesta versão): nunca é login concluído nem nuvem funcionando. */
  unreachable = false
  /** A intenção que esta janela enviou e ainda espera. */
  busy: ForkCloudIntent['kind'] | null = null
  notice: Notice | null = null
  flow: Flow | null = null
  /** O pedido cujo código a pessoa digitou e o Main aceitou (A2). */
  compared: string | null = null
  /** O pedido que já mostrou o código: um `linking` depois dele é o passo seguinte à aprovação (A1). */
  revealed: string | null = null
  /** O último pedido de vínculo visto. */
  private attempt: string | null = null
  /** Cerca as respostas: muda com a conta e com cancelar/desconectar, que encerram o que estava em voo. */
  private turn = 0
  private epoch = -1
  private revision: number
  private started = false
  private synchronized = false
  private bufferedEvent: ForkCloudResult | null = null
  private readonly bridge: ForkCloudBridge
  private readonly account: ForkCloudAccount
  private readonly deviceName: () => string

  constructor(bridge: ForkCloudBridge, account: ForkCloudAccount, deviceName: () => string) {
    super()
    this.bridge = bridge
    this.account = account
    this.deviceName = deviceName
    this.revision = account.sessionRevision
  }

  start() {
    if (this.started) return
    this.started = true
    this.bridge.onChange((result) => {
      if (this.synchronized) this.accept(result)
      else if (!this.bufferedEvent || result.state.epoch >= this.bufferedEvent.state.epoch) this.bufferedEvent = result
    })
    this.account.subscribe(() => this.noteAccount())
    void this.read()
  }

  /** Só leitura: na abertura, na troca de conta, ao conferir de novo e depois de cancelar. */
  refresh() {
    if (this.started) void this.read()
  }

  private async read() {
    const turn = this.turn
    let result: ForkCloudResult
    try {
      result = await this.bridge.invoke({ kind: 'state' })
    } catch {
      if (turn !== this.turn) return
      this.unreachable = true
      this.changed()
      return
    }
    if (turn === this.turn && this.accept(result)) {
      // Anchor events to a fresh Main reply from this account turn before a
      // delayed event can recreate the previous account's cached store.
      this.synchronized = true
      const buffered = this.bufferedEvent
      this.bufferedEvent = null
      if (buffered && buffered.state.epoch >= this.epoch) this.accept(buffered)
    }
  }

  /** Outra conta (ou a mesma de novo): nada da anterior fica na tela nem anexado. */
  private noteAccount() {
    if (this.account.sessionRevision === this.revision) return
    this.revision = this.account.sessionRevision
    this.turn++
    this.synchronized = false
    this.bufferedEvent = null
    this.registration = 'unavailable'
    this.state = null
    this.recovery = null
    this.recoveryPhase = null
    this.computers = []
    this.attachment = null
    this.bootstrap = null
    this.unreachable = false
    this.busy = null
    this.notice = null
    this.forgetFlow()
    this.changed()
    void this.read()
  }

  private forgetFlow() {
    this.flow = null
    this.compared = null
    this.revealed = null
    this.attempt = null
  }

  /** Aceita um retrato do Main; um de época anterior descreve uma sessão que já acabou. */
  private accept(raw: unknown, keepState = false): boolean {
    if (!raw || typeof raw !== 'object') return false
    const result = raw as Partial<ForkCloudResult>
    const state = result.state
    if (!state || typeof state !== 'object' || typeof state.kind !== 'string' || typeof state.epoch !== 'number' || typeof state.attemptId !== 'string') return false
    if (state.epoch < this.epoch) return false
    if (this.epoch !== -1 && state.epoch > this.epoch) {
      // Nova sessão no Main: o que esta janela começou na anterior acabou junto.
      this.forgetFlow()
      this.notice = null
    }
    this.epoch = state.epoch
    this.registration = REGISTRATIONS.includes(result.registration as string) ? (result.registration as Registration) : 'unavailable'
    this.computers = Array.isArray(result.computers) ? result.computers : []
    this.attachment = result.attachment ?? null
    this.unreachable = false
    if (!keepState) {
      // A incerteza anda com o estado que o Main publicou: uma recusa não a cria nem a apaga.
      this.recovery = result.recovery === 'registration' || result.recovery === 'enrollment' ? result.recovery : null
      this.recoveryPhase = result.recoveryPhase ?? null
      // Ausente quando o Main não oferece ACS4: nada do retrato anterior continua valendo.
      this.bootstrap = bootstrapOf(result.bootstrap)
      this.take(state)
    }
    this.changed()
    return true
  }

  private take(state: ForkCloudState) {
    const next: ForkCloudState = { ...state }
    // O `linked` do fim da primeira vinculação traz o identificador dela, que nunca é um pedido A1/A2.
    const enrollment = !!next.attemptId && next.attemptId !== this.bootstrap?.bootstrapId
    if (ENROLLING.includes(next.kind) && enrollment && (next.kind !== 'linking' || next.deadline)) this.attempt = next.attemptId
    if (next.kind === 'waitingOwnerApproval' && next.attemptId) this.revealed = next.attemptId
    if (this.compared && this.compared !== next.attemptId) this.compared = null
    // Cancelado, desconectado ou não enviado pelo Main: nada desta janela continua em andamento.
    if (next.kind === 'notSent') this.forgetFlow()
    this.state = next
  }

  /**
   * Envia uma intenção pedida pela pessoa. Uma de cada vez; cancelar e desconectar passam na frente
   * e fazem a resposta do que estava em voo chegar como `superseded`. Nada aqui é reenviado.
   */
  private async act(intent: ForkCloudIntent, flow?: Flow): Promise<Outcome> {
    const quiet = intent.kind === 'cancel' || intent.kind === 'disconnect'
    if (!this.started || (this.busy && !quiet)) return 'notSent'
    // Enquanto a primeira vinculação bloqueia, só ela anda nesta janela; o Main também recusa o resto.
    if (!quiet && this.bootstrapBlocks && !BOOTSTRAP_INTENTS.includes(intent.kind)) {
      this.notice = NOT_SENT
      this.changed()
      return 'notSent'
    }
    if (quiet) this.turn++
    const turn = this.turn
    const before = this.flow
    this.busy = intent.kind
    this.notice = null
    if (flow) this.flow = flow
    this.changed()
    let reply: ForkCloudResult
    try {
      reply = await this.bridge.invoke(intent)
    } catch {
      if (turn !== this.turn) return 'superseded'
      // A intenção pode ter começado no Main: fica incerta e não é repetida.
      this.busy = null
      this.notice = UNCONFIRMED
      this.changed()
      return 'unconfirmed'
    }
    if (turn !== this.turn) return 'superseded'
    this.busy = null
    if (!quiet && reply?.state?.kind === 'notSent') {
      // Recusada antes de qualquer efeito: o estado do Main continua o de antes.
      // A denial is not an authoritative replacement snapshot. Keep registration,
      // computers, attachment and recovery; state events carry real changes.
      if (flow) this.flow = before
      this.notice = NOT_SENT
      this.changed()
      return 'notSent'
    }
    if (!this.accept(reply)) {
      this.notice = UNCONFIRMED
      this.changed()
      return 'unconfirmed'
    }
    return 'accepted'
  }

  // MARK: intenções (cada uma, só por um clique)

  registerDevice() {
    return this.act({ kind: 'registerDevice', name: this.deviceName() })
  }

  /**
   * Confere no Main, só com leituras e a chave que já existe, um registro incerto ou feito antes de
   * reabrir o app. Não registra de novo. Registrado só vale pelo `registration` que o Main publicar;
   * uma recusa deixa tudo como estava e o registro continua disponível.
   */
  async reconcileRegistration() {
    const outcome = await this.act({ kind: 'reconcileRegistration' })
    if (outcome === 'notSent' && this.notice === NOT_SENT) {
      this.notice = NO_REGISTRATION
      this.changed()
    }
    return outcome
  }

  /** Confere só por leituras a posse enviada, sem outro claim ou aprovação. */
  reconcileClaim() { return this.act({ kind: 'reconcileClaim' }) }

  discover() {
    return this.act({ kind: 'discover' })
  }

  /** A1: somente pelo perfil SSH já conectado; `claim` só quando a pessoa pediu para pôr o Hub na conta. */
  startLocal(profileId: string, computerId: string, name: string, claim: boolean) {
    return this.act({ kind: 'startLocal', profileId, claim }, { kind: 'local', computerId, name })
  }

  /** A2: um Hub do diretório que o Main listou nesta sessão. */
  startRemote(computer: ForkCloudComputer) {
    return this.act({ kind: 'startRemote', computerId: computer.computerId }, { kind: 'remote', computerId: computer.computerId, name: computer.name })
  }

  connect(computerId: string, name: string | null) {
    return this.act({ kind: 'connect', computerId }, { kind: 'connect', computerId, name })
  }

  /** O pedido atual, o único que `check` e `cancel` podem nomear. */
  get currentAttempt(): string | null {
    const id = this.state?.attemptId
    return id && id === this.attempt ? id : null
  }

  /** Busca o código ou a aprovação do pedido atual, uma vez. */
  check() {
    const attemptId = this.currentAttempt
    return attemptId ? this.act({ kind: 'check', attemptId }) : Promise.resolve<Outcome>('notSent')
  }

  /**
   * A2: os seis dígitos que a pessoa leu no aparelho do dono. O pedido é fixado agora; um código
   * vencido não sai. Nunca vale para A1, em que o Main confere o código pelo SSH.
   */
  async confirm(code: string): Promise<Outcome | 'expired'> {
    const s = this.state
    const attemptId = this.currentAttempt
    if (!s || s.kind !== 'waitingOwnerApproval' || !attemptId || this.flow?.kind === 'local' || !/^[0-9]{6}$/.test(code)) return 'notSent'
    if (typeof s.deadline === 'number' && Date.now() > s.deadline) {
      this.notice = EXPIRED
      this.changed()
      return 'expired'
    }
    const outcome = await this.act({ kind: 'confirm', attemptId, code })
    if (outcome === 'accepted' && this.state?.attemptId === attemptId) {
      this.compared = attemptId
      this.changed()
    }
    return outcome
  }

  /** Cancela o pedido atual e relê o estado: uma recusa do Main também volta como `notSent`. */
  async cancel() {
    const attemptId = this.currentAttempt
    if (!attemptId) return 'notSent' as const
    const outcome = await this.act({ kind: 'cancel', attemptId })
    if (outcome === 'accepted') this.refresh()
    return outcome
  }

  /** Verifica a gravação do vínculo do Hub atual, sem repetir posse nem aprovação. */
  reconcilePin() {
    const computerId = this.state?.computerId
    return computerId ? this.act({ kind: 'reconcilePin', computerId }) : Promise.resolve<Outcome>('notSent')
  }

  /** Encerra neste Mac a conexão ou o pedido em andamento; não muda nada no Hub. */
  disconnect() {
    return this.act({ kind: 'disconnect' })
  }

  // MARK: primeira vinculação (ACS4), cada passo só pelo clique e quando o Main o oferece

  /**
   * Pede o primeiro vínculo; o Main fixa identificador e nonce no journal antes de enviar. Em
   * `prepared` (nada saiu, ou recusa sabidamente anterior ao registro) o mesmo clique reenvia o
   * mesmo pedido pelo Main; incerteza nunca volta a oferecer isso.
   */
  async startBootstrap(): Promise<Outcome> {
    if (!this.bootstrap?.canStart) return 'notSent'
    const outcome = await this.act({ kind: 'startBootstrap' })
    const phase = this.bootstrap?.phase
    if (outcome === 'accepted' && (phase === 'available' || phase === 'prepared')) {
      this.notice = BOOTSTRAP_NO_EFFECT
      this.changed()
    }
    return outcome
  }

  /** Lê de novo o mesmo pedido (GET): não cria, não prova e não publica nada. */
  checkBootstrap() { return this.bootstrapStep('checkBootstrap', 'canCheck') }

  /** A primeira posse: o Main abre um canal de uso único, recebe o desafio e envia a prova, uma vez. */
  proveBootstrap() { return this.bootstrapStep('proveBootstrap', 'canProve') }

  /**
   * Confirma o vínculo na nuvem ou conclui a gravação local com os mesmos bytes. Depois de uma
   * confirmação sem resposta, o Main lê o mesmo pedido e só pede a mesma publicação idempotente do
   * mesmo recibo; nunca repete pedido, autorização ou prova.
   */
  reconcileBootstrap() { return this.bootstrapStep('reconcileBootstrap', 'canReconcile') }

  /** Nomeia só o identificador que o Main devolveu; um clique que não muda a etapa diz isso. */
  private async bootstrapStep(kind: 'checkBootstrap' | 'proveBootstrap' | 'reconcileBootstrap', allowed: 'canCheck' | 'canProve' | 'canReconcile'): Promise<Outcome> {
    const before = this.bootstrap
    if (!before || !before[allowed] || !before.bootstrapId) return 'notSent'
    const outcome = await this.act({ kind, bootstrapId: before.bootstrapId })
    const after = this.bootstrap
    if (outcome === 'accepted' && after?.bootstrapId === before.bootstrapId && after.phase === before.phase) {
      this.notice = BOOTSTRAP_SAME_STEP
      this.changed()
    }
    return outcome
  }

  // MARK: derivados

  /** Há um pedido de primeira vinculação começado e ainda sem desfecho: o registro dele continua na tela. */
  get bootstrapOpen(): boolean {
    const phase = this.bootstrap?.phase
    return !!phase && phase !== 'available' && phase !== 'complete' && phase !== 'closed'
  }

  /**
   * O pedido terminou sem virar vínculo (cancelado ou vencido): fica na tela só como histórico, com o
   * identificador e sem ação. A prova pode ter saído antes; o Main guarda o que já existia.
   */
  get bootstrapClosed(): boolean {
    return this.bootstrap?.phase === 'closed'
  }

  /**
   * O pedido impede as outras ações pela nuvem. Só o `false` explícito do Main libera um pedido
   * aberto ou encerrado; sem o campo, falha fechado. Prazo, fase, motivo ou relógio da tela nunca
   * liberam por inferência.
   */
  get bootstrapBlocks(): boolean {
    const blocks = this.bootstrap?.blocksOtherOperations
    return blocks === true || ((this.bootstrapOpen || this.bootstrapClosed) && blocks !== false)
  }

  /** O Hub da primeira vinculação concluída, pelo identificador que o Main devolveu; o nome não é inventado. */
  get bootstrapHub(): string | null {
    const b = this.bootstrap
    return b?.phase === 'complete' && b.computerId ? b.computerId : null
  }

  /** O `linked` que o Main publicou ao concluir a primeira vinculação: vínculo gravado, ainda sem conexão. */
  get bootstrapLinked(): string | null {
    const s = this.state, hub = this.bootstrapHub
    return hub && s?.kind === 'linked' && s.computerId === hub && s.attemptId === this.bootstrap?.bootstrapId ? hub : null
  }

  /** O anexo que o Main qualificou (hello conferido com o pin) e publicou com `ready`; mais nada conta como conectado. */
  get readyAttachment(): ForkCloudAttachment | null {
    const s = this.state, a = this.attachment
    if (!s || s.kind !== 'ready' || !s.computerId || !a || typeof a !== 'object') return null
    if (a.computerId !== s.computerId || !a.computer || a.computer.id !== a.computerId) return null
    if (typeof a.connectionId !== 'string' || !a.connectionId || typeof a.hostId !== 'string' || !a.hostId || !ownContext(a.contextId)) return null
    return a
  }

  /** O nome de um Hub pelo que o Main publicou nesta sessão ou pelo que a pessoa escolheu. */
  nameOf(computerId: string | undefined): string | null {
    if (!computerId) return null
    if (this.attachment?.computerId === computerId && this.attachment.computer?.name) return this.attachment.computer.name
    if (this.flow?.computerId === computerId && this.flow.name) return this.flow.name
    return this.computers.find((c) => c.computerId === computerId)?.name ?? null
  }
}
