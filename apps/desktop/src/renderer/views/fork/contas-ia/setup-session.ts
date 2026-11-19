import type { HostClient } from '../../../client/host-client'
import type { AccountId } from '../../../../shared/fork-engine-accounts.ts'
import type { SetupAction, SetupOperation, SetupPhase, StartSetup } from '../../../../shared/fork-engine-setup.ts'
import { Observable } from '../../../lib/observable.ts'
import { createTerminalFilter, errorCode, expiresAtMs, isFinal, newRequestId, stepOf, utf8ToBase64 } from './regras.ts'

/** O que a sessão usa do `engineSetup` (o wrapper real em produção; um falso nos testes). */
export interface SetupApi {
  start(client: HostClient, request: StartSetup): Promise<SetupOperation>
  action(client: HostClient, request: SetupAction): Promise<SetupOperation>
}

export type SetupStep = 'install' | 'login'

/** Só identificadores, para retomar a mesma operação ao reabrir o painel (nada de URL, código ou saída). */
export interface OperationRef {
  operationId: string
  generation: number
  step: SetupStep
}

export interface SetupOptions {
  /** Preparar (instala ou reaproveita o programa compatível) antes do login. */
  installFirst: boolean
  onRef?: (ref: OperationRef | null) => void
  onFinished?: (operation: SetupOperation, step: SetupStep) => void
  now?: () => number
}

type DistributiveOmit<T, K extends keyof never> = T extends unknown ? Omit<T, K> : never
type OperationRequest = DistributiveOmit<SetupAction, 'operationId' | 'generation'>

const READ_MS = 1500
const READ_TERMINAL_MS = 700
const BACKOFF_MS = [1500, 3000, 5000, 8000]
const MAX_FAILURES = 10
/** A operação não existe mais no Hub (venceu, foi trocada ou o Hub reiniciou). */
const GONE = new Set(['operation_expired', 'operation_stale', 'operation_forbidden', 'remote_setup_unavailable', 'setup_authority_lost'])

/**
 * Uma conexão de conta no painel: inicia, acompanha e cancela a operação do Hub. O desafio
 * (URL/código) e a apresentação do terminal existem só aqui, em memória, e somem quando a
 * operação termina, vence, é cancelada ou o painel fecha. Fechar não cancela no Hub.
 */
export class SetupSession extends Observable {
  step: SetupStep | null = null
  operation: SetupOperation | null = null
  /** A maior etapa vista (0 Preparando … 3 Conectado), para mostrar onde parou. */
  reached = -1
  starting = false
  cancelling = false
  submitting = false
  /** Falha de pedido (não da operação), já em português, e o código do Hub. */
  /** DON-917: o erro cru (estado é dado); a tela o traduz com `errorMessage(erro, idioma)`. */
  error: unknown = null
  errorKind: string | null = null
  /** Leituras falhando em sequência. */
  stalled = false
  terminalOpen = false

  readonly accountId: AccountId
  /** A revisão atual da conta, lida na hora de cada início (renomear muda a revisão). */
  private readonly revision: () => number
  private readonly client: () => Promise<HostClient>
  private readonly api: SetupApi
  private readonly options: SetupOptions
  /** Revisão que a própria operação devolveu (Preparar sobe a revisão ao terminar). */
  private knownRevision = 0
  private disposed = false
  private timer: ReturnType<typeof setTimeout> | null = null
  private expiry: ReturnType<typeof setTimeout> | null = null
  private requestId: { step: SetupStep; id: string } | null = null
  private cursor = 0
  private failures = 0
  /** Cancelar e iniciar outra etapa invalidam todos os pedidos do ciclo anterior. */
  private cycle = 0
  private reading: number | null = null
  private filter = createTerminalFilter()
  private shown = ''
  private terminalListeners = new Set<(text: string) => void>()
  private lastSize = { cols: 0, rows: 0 }

  constructor(accountId: AccountId, revision: () => number, client: () => Promise<HostClient>, api: SetupApi, options: SetupOptions) {
    super()
    this.accountId = accountId
    this.revision = revision
    this.client = client
    this.api = api
    this.options = options
  }

  get phase(): SetupPhase | null {
    return this.operation?.phase ?? null
  }

  get active() {
    return this.operation !== null && !isFinal(this.operation.phase)
  }

  /** Passou do prazo e o Hub ainda não disse que venceu: nada mais é enviado. */
  get overdue() {
    const op = this.operation
    return !!op && !isFinal(op.phase) && op.expiresAt > 0 && this.now() >= expiresAtMs(op.expiresAt)
  }

  /** O Hub só oferece terminal ao login que espera um código colado (Claude). */
  get canOpenTerminal() {
    const op = this.operation
    return this.active && !this.overdue && op?.phase === 'awaiting_authorization' && op.challenge?.kind === 'paste_code'
  }

  /** O terminal aberto aceita o código agora. */
  get terminalWritable() {
    return this.terminalOpen && this.active && !this.overdue && !this.cancelling && !this.submitting && this.operation?.terminal?.available === true
  }

  /** Ação explícita da pessoa: começa (Preparar, depois Entrar). */
  begin() {
    if (this.disposed || this.starting || this.active) return
    void this.startStep(this.options.installFirst ? 'install' : 'login', true)
  }

  /** Reabrir o painel: lê a operação que já estava em andamento. */
  resume(ref: OperationRef) {
    if (this.disposed || this.operation) return
    this.step = ref.step
    this.operation = {
      operationId: ref.operationId, engineAccountId: this.accountId, generation: ref.generation, phase: 'preparing',
      revision: -1, expiresAt: 0, errorCode: null, challenge: null, terminal: null,
    }
    this.changed()
    void this.read()
  }

  /** Recomeça depois de terminar (vencido, cancelado, falhou), com pedido novo. */
  restart() {
    if (this.disposed || this.starting || this.active) return
    this.reached = -1
    void this.startStep(this.options.installFirst ? 'install' : 'login', true)
  }

  /** Tenta de novo a etapa que não chegou ao Hub (o mesmo pedido). */
  retryStart() {
    if (this.disposed || this.starting || this.active || !this.step) return
    void this.startStep(this.step, false)
  }

  private async startStep(step: SetupStep, fresh: boolean) {
    const cycle = this.advanceCycle()
    // Mesmo pedido em nova tentativa da mesma etapa (a rede caiu no meio); etapa nova, pedido novo.
    if (fresh || this.requestId?.step !== step) this.requestId = { step, id: newRequestId() }
    const clientRequestId = this.requestId.id
    this.starting = true
    this.setError(null)
    this.stalled = false
    this.step = step
    this.purge()
    this.operation = null
    this.reached = Math.max(this.reached, 0)
    this.changed()
    try {
      const client = await this.client()
      if (!this.currentCycle(cycle)) return
      const expectedRevision = Math.max(this.revision(), this.knownRevision)
      const operation = await this.api.start(client, { engineAccountId: this.accountId, expectedRevision, action: step, clientRequestId })
      if (!this.currentCycle(cycle)) return
      this.starting = false
      this.apply(operation)
    } catch (error) {
      if (!this.currentCycle(cycle)) return
      this.starting = false
      this.setError(error)
      this.changed()
    }
  }

  private apply(next: SetupOperation) {
    const current = this.operation
    if (next.engineAccountId !== this.accountId) return
    if (current) {
      if (current.operationId !== next.operationId || current.generation !== next.generation) return
      // revision é da conta, não do estado da operação: cancelled e um read antigo podem ser iguais.
      if (isFinal(current.phase) || next.revision < current.revision) return
    }
    this.knownRevision = Math.max(this.knownRevision, next.revision)
    // O prazo vale mesmo que o Hub ainda devolva a operação ativa (ex.: Start repetido): nada do desafio aparece.
    const lapsed = !isFinal(next.phase) && this.lapsed(next)
    // A apresentação vem inteira; o cursor é a versão dela. Versão nova substitui a tela.
    const terminal = next.terminal
    if (!lapsed && terminal && this.terminalOpen && terminal.cursor !== this.cursor) {
      this.cursor = terminal.cursor
      this.show(terminal.output)
    }
    // Preparar inteiro é a etapa Preparando.
    const step = this.step === 'install' ? 0 : stepOf(next.phase)
    if (step !== null) this.reached = Math.max(this.reached, step)
    this.operation = lapsed ? { ...next, challenge: null, terminal: null } : next
    this.options.onRef?.(isFinal(next.phase) || !this.step ? null : { operationId: next.operationId, generation: next.generation, step: this.step })
    if (isFinal(next.phase)) {
      this.finish(next)
      return
    }
    if (lapsed) {
      this.expireLocally()
      return
    }
    this.scheduleExpiry(next)
    this.schedule()
    this.changed()
  }

  private finish(op: SetupOperation) {
    this.advanceCycle()
    this.cancelling = false
    this.submitting = false
    const step = this.step ?? 'login'
    // Desafio e apresentação somem; ficam só a fase e o motivo.
    this.operation = { ...op, challenge: null, terminal: null }
    this.purge()
    this.changed()
    this.options.onFinished?.(op, step)
    if (step === 'install' && op.phase === 'connected' && !this.disposed) void this.startStep('login', true)
  }

  private schedule(delay?: number) {
    this.clearTimer()
    if (this.disposed || !this.operation || isFinal(this.operation.phase)) return
    const wait = delay ?? (this.terminalOpen ? READ_TERMINAL_MS : READ_MS)
    this.timer = setTimeout(() => void this.read(), wait)
  }

  private async read() {
    const op = this.operation
    if (this.disposed || !op || !this.active || this.reading !== null) return
    const cycle = this.cycle
    this.reading = cycle
    try {
      const client = await this.client()
      if (!this.currentCycle(cycle, op)) return
      // `afterCursor` igual ao do Hub dispensa reenviar a mesma apresentação.
      const afterCursor = this.terminalOpen && this.cursor >= 0 ? this.cursor : undefined
      const next = await this.api.action(client, { operationId: op.operationId, generation: op.generation, action: 'read', ...(afterCursor === undefined ? {} : { afterCursor }) })
      if (!this.currentCycle(cycle, op)) return
      this.reading = null
      this.failures = 0
      this.stalled = false
      this.apply(next)
      if (this.currentCycle(cycle, op) && this.active) this.schedule()
    } catch (error) {
      if (!this.currentCycle(cycle, op)) return
      this.reading = null
      const code = errorCode(error)
      if (code && GONE.has(code)) {
        this.finish({ ...op, phase: 'failed', errorCode: code })
        return
      }
      this.failures++
      this.stalled = this.failures >= 2
      if (this.failures >= MAX_FAILURES) {
        this.clearTimer()
        this.setError(error)
        this.changed()
        return
      }
      this.changed()
      this.schedule(BACKOFF_MS[Math.min(this.failures - 1, BACKOFF_MS.length - 1)])
    }
  }

  /** Volta a acompanhar depois de muitas falhas seguidas. */
  retryReading() {
    if (!this.active) return
    this.failures = 0
    this.setError(null)
    this.changed()
    void this.read()
  }

  async cancel() {
    const op = this.operation
    if (this.disposed || !op || isFinal(op.phase) || this.cancelling) return
    this.advanceCycle()
    this.scheduleExpiry(op)
    this.submitting = false
    this.cancelling = true
    this.setError(null)
    this.changed()
    // Só a fase do Hub diz que cancelou; até lá, segue acompanhando.
    await this.send(op, { action: 'cancel' }, () => { this.cancelling = false })
    if (!this.disposed && this.active && this.operation) { this.scheduleExpiry(this.operation); this.schedule() }
  }

  /** Código colado no painel (login do Claude). O valor não fica guardado aqui. */
  async submitCode(code: string) {
    const op = this.operation
    const value = code.trim()
    if (this.disposed || !op || op.phase !== 'awaiting_authorization' || this.overdue || this.submitting || !value) return
    this.submitting = true
    const cycle = this.cycle
    this.setError(null)
    this.changed()
    await this.send(op, { action: 'submit_code', code: value }, () => {})
    if (!this.currentCycle(cycle, op)) return
    this.submitting = false
    this.changed()
  }

  async openTerminal() {
    const op = this.operation
    if (this.disposed || !op || !this.canOpenTerminal) return
    this.setError(null)
    this.terminalOpen = true
    this.cursor = -1
    this.changed()
    await this.send(op, { action: 'open_terminal' }, () => this.closeTerminal())
  }

  closeTerminal() {
    if (!this.terminalOpen) return
    this.terminalOpen = false
    this.cursor = 0
    this.show('')
    this.changed()
    this.schedule()
  }

  /** Uma linha digitada no terminal: o Hub só aceita o código de autorização inteiro. */
  async submitTerminalLine(line: string) {
    const op = this.operation
    const value = line.trim()
    if (this.disposed || !op || !this.terminalWritable || !value) return
    this.submitting = true
    const cycle = this.cycle
    this.setError(null)
    this.changed()
    await this.send(op, { action: 'terminal_input', data: utf8ToBase64(value) }, () => {})
    if (!this.currentCycle(cycle, op)) return
    this.submitting = false
    this.changed()
  }

  resize(cols: number, rows: number) {
    const op = this.operation
    if (this.disposed || !op || !this.terminalOpen || !this.active || !op.terminal) return
    // O Hub aceita de 20 a 240 colunas e de 5 a 100 linhas.
    const size = { cols: Math.min(240, Math.max(20, cols)), rows: Math.min(100, Math.max(5, rows)) }
    if (size.cols === this.lastSize.cols && size.rows === this.lastSize.rows) return
    this.lastSize = size
    const cycle = this.cycle
    void this.client()
      .then((client) => this.currentCycle(cycle, op) ? this.api.action(client, { operationId: op.operationId, generation: op.generation, action: 'terminal_resize', ...size }) : null)
      .then((next) => { if (next && this.currentCycle(cycle, op)) this.apply(next) }, () => {})
  }

  /** O terminal visível recebe a apresentação já filtrada, sempre inteira (substitui a anterior). */
  onTerminal(listener: (text: string) => void) {
    this.terminalListeners.add(listener)
    listener(this.shown)
    return () => void this.terminalListeners.delete(listener)
  }

  /** Fecha o painel: para de acompanhar e apaga desafio e apresentação. A operação segue no Hub. */
  dispose() {
    if (this.disposed) return
    this.disposed = true
    this.clearTimer()
    if (this.expiry) clearTimeout(this.expiry)
    this.purge()
    if (this.operation) this.operation = { ...this.operation, challenge: null, terminal: null }
    this.terminalListeners.clear()
  }

  private async send(op: SetupOperation, request: OperationRequest, failed: () => void) {
    const cycle = this.cycle
    try {
      const client = await this.client()
      if (!this.currentCycle(cycle, op)) return
      const next = await this.api.action(client, { operationId: op.operationId, generation: op.generation, ...request } as SetupAction)
      if (this.currentCycle(cycle, op)) this.apply(next)
    } catch (error) {
      if (!this.currentCycle(cycle, op)) return
      failed()
      const code = errorCode(error)
      if (code && GONE.has(code)) {
        this.finish({ ...op, phase: 'failed', errorCode: code })
        return
      }
      this.setError(error)
      this.changed()
    }
  }

  private setError(error: unknown) {
    this.error = error
    this.errorKind = error === null ? null : errorCode(error)
  }

  private show(text: string) {
    this.filter.reset()
    this.shown = text ? this.filter.push(text) : ''
    for (const listener of this.terminalListeners) listener(this.shown)
  }

  private purge() {
    this.terminalOpen = false
    this.cursor = 0
    this.show('')
  }

  private lapsed(op: SetupOperation) {
    return op.expiresAt > 0 && this.now() >= expiresAtMs(op.expiresAt)
  }

  /** Venceu pelo relógio: apaga desafio e apresentação já, fecha o terminal e pede ao Hub que confirme. */
  private expireLocally() {
    const op = this.operation
    if (this.disposed || !op || isFinal(op.phase)) return
    this.operation = { ...op, challenge: null, terminal: null }
    this.purge()
    this.changed()
    this.schedule(READ_MS)
  }

  private scheduleExpiry(op: SetupOperation) {
    if (this.expiry) clearTimeout(this.expiry)
    this.expiry = null
    if (!op.expiresAt) return
    const wait = expiresAtMs(op.expiresAt) - this.now()
    if (wait <= 0) { this.expireLocally(); return }
    if (wait > 0 && wait < 2 ** 31) this.expiry = setTimeout(() => { if (this.overdue) this.expireLocally() }, wait + 50)
  }

  private clearTimer() {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
  }

  private advanceCycle() {
    this.clearTimer()
    if (this.expiry) clearTimeout(this.expiry)
    this.expiry = null
    this.reading = null
    this.failures = 0
    return ++this.cycle
  }

  private currentCycle(cycle: number, op?: SetupOperation) {
    return !this.disposed && cycle === this.cycle && (!op ||
      (op.operationId === this.operation?.operationId && op.generation === this.operation.generation))
  }

  private now() {
    return this.options.now?.() ?? Date.now()
  }
}
