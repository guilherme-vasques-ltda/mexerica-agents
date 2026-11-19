import { ROUTING_PERSONAL, type RoutingPolicy, type RoutingReceiptRequest, type RoutingReceiptResponse, type RoutingSave, type RoutingView } from '../../shared/fork-roteamento.ts'

export interface RoutingOperation {
  read(check: boolean): Promise<RoutingView>
  save(request: RoutingSave): Promise<RoutingView>
  current(): boolean
}
export interface RoutingTransport extends RoutingOperation {
  capture?(): RoutingOperation
}
export interface RoutingState {
  view: RoutingView | null
  policy: RoutingPolicy | null
  generalGuide: string
  dirty: boolean
  busy: boolean
  error: string | null
  message: string | null
  fixedReady: boolean
  fixedCurrent: boolean
}
interface FixedBinding {
  snapshot(): { key: string; ready: boolean }
  subscribe(listener: () => void): () => void
}
const copy = <T>(value: T): T => structuredClone(value)
const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
export function routingError(error: unknown) {
  const raw = typeof error === 'object' && error !== null && 'message' in error ? String(error.message) : String(error)
  const code = raw.replace(/^Error: /, '').split(':')[0]
  if (code === 'settings_conflict') return 'As preferências mudaram em outra janela. Seu rascunho continua aqui. Recarregue para comparar antes de salvar.'
  if (code === 'remote_setup_unavailable') return 'Este aparelho não tem autorização vigente para gerenciar contas de IA. Seu rascunho continua aqui.'
  if (code === 'accounts_unavailable') return 'Não foi possível ler o cadastro de contas. As preferências e o rascunho foram preservados.'
  if (code === 'account_conflict') return 'A conta mudou. Recarregue o cadastro antes de escolher a reserva novamente.'
  if (code === 'routing_scope_unavailable') return 'As contas deste Space ainda não têm vínculo comprovado. Nenhuma reserva nova foi salva.'
  if (code === 'invalid_field') return 'Revise os guias (até 2.000 caracteres) e as reservas. Nada foi salvo.'
  return 'Não foi possível concluir. Confira a conexão, a conta, o modelo e as opções. Seu rascunho continua aqui.'
}
/** Rascunho somente de interface. A validação e o CAS pertencem ao Hub. */
export class RoutingStore {
  private state: RoutingState = { view: null, policy: null, generalGuide: '', dirty: false, busy: false, error: null, message: null, fixedReady: true, fixedCurrent: false }
  private listeners = new Set<() => void>()
  private generation = 0
  private transport: RoutingTransport
  private fixed?: FixedBinding
  private fixedKey = ''
  private fixedGeneration = 0
  private stopFixed?: () => void
  constructor(transport: RoutingTransport, fixed?: FixedBinding) { this.transport = transport; this.fixed = fixed; this.syncFixed() }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    if (this.listeners.size === 1 && this.fixed) { this.stopFixed = this.fixed.subscribe(() => this.syncFixed()); this.syncFixed() }
    return () => { this.listeners.delete(listener); if (!this.listeners.size) { this.stopFixed?.(); this.stopFixed = undefined } }
  }
  getSnapshot = () => this.state
  fixedChanged() { this.syncFixed() }
  private syncFixed() {
    const next = this.fixed?.snapshot()
    if (!next || next.key === this.fixedKey && next.ready === this.state.fixedReady) return
    const changed = next.key !== this.fixedKey
    this.fixedKey = next.key
    this.fixedGeneration++
    this.update({ fixedReady: next.ready, ...(changed ? { fixedCurrent: false } : {}), message: this.state.message?.startsWith('Conferência') ? null : this.state.message })
  }
  private update(patch: Partial<RoutingState>) {
    this.state = { ...this.state, ...patch }
    this.state.dirty = !!this.state.view && (!equal(this.state.policy, this.state.view.policy.value) || this.state.generalGuide !== this.state.view.generalGuide.value)
    for (const listener of this.listeners) listener()
  }
  edit(policy: RoutingPolicy, generalGuide = this.state.generalGuide) { this.update({ policy, generalGuide, error: null, message: null }) }
  discard() { if (this.state.view) this.update({ policy: copy(this.state.view.policy.value), generalGuide: this.state.view.generalGuide.value, error: null, message: null }) }
  async settled() {
    if (!this.state.busy) return
    await new Promise<void>((resolve) => {
      const stop = this.subscribe(() => { if (!this.state.busy) { stop(); resolve() } })
    })
  }
  close() { this.generation++; this.update({ busy: false }) }
  async read(check = false, replace = false) {
    this.syncFixed()
    if (this.state.busy || !this.transport.current() || check && !this.state.fixedReady) return
    const fixedGeneration = this.fixedGeneration
    const fixedReady = this.state.fixedReady
    const transport = this.transport.capture?.() ?? this.transport
    const current = () => transport.current()
    const generation = ++this.generation
    this.update({ busy: true, error: null, message: null })
    try {
      const view = await transport.read(check)
      if (generation !== this.generation || !current()) return
      this.syncFixed()
      const fixedCurrent = fixedReady && this.state.fixedReady && fixedGeneration === this.fixedGeneration
      const keep = this.state.dirty && !replace
      this.update({ view: keep ? { ...this.state.view!, fixedCheck: view.fixedCheck, accounts: view.accounts } : view, ...(keep ? {} : { policy: copy(view.policy.value), generalGuide: view.generalGuide.value }), fixedCurrent, message: check && fixedCurrent ? 'Conferência estática atualizada. Nenhuma chamada ao modelo foi feita.' : null })
    } catch (error) { if (generation === this.generation && current()) this.update({ error: routingError(error) }) }
    finally { if (generation === this.generation) this.update({ busy: false }) }
  }
  async save() {
    this.syncFixed()
    const { view, policy, generalGuide, busy } = this.state
    if (!view || !policy || busy || !this.transport.current()) return
    const transport = this.transport.capture?.() ?? this.transport
    const current = () => transport.current()
    const generation = ++this.generation
    const submitted = copy(policy)
    const fixedGeneration = this.fixedGeneration
    const fixedReady = this.state.fixedReady
    this.update({ busy: true, error: null, message: null })
    try {
      const saved = await transport.save({ botId: view.botId, expectedRevision: view.policy.revision, expectedGeneralRevision: view.generalGuide.revision, policy: submitted, generalGuide })
      if (generation !== this.generation || !current()) return
      this.syncFixed()
      this.update({ view: saved, ...(equal(this.state.policy, submitted) ? { policy: copy(saved.policy.value) } : {}), ...(this.state.generalGuide === generalGuide ? { generalGuide: saved.generalGuide.value } : {}), fixedCurrent: fixedReady && this.state.fixedReady && fixedGeneration === this.fixedGeneration, message: 'Preferência guardada. O bot continua no Fixo; Auto e reservas permanecem indisponíveis.' })
    } catch (error) { if (generation === this.generation && current()) this.update({ error: routingError(error) }) }
    finally { if (generation === this.generation) this.update({ busy: false }) }
  }
}


export interface RoutingSource {
  readonly connectedClient: { call<T>(method: string, body: Record<string, unknown>): Promise<T> } | null
  readonly hello: { hostId?: string } | null
  readonly principalEpoch: number
  readonly spaceId?: string
  readonly bots: { get(id: string): FixedFields | undefined }
  subscribe(listener: () => void): () => void
}
interface FixedFields { backend?: string; model?: string | null; modelOptions?: Record<string, string>; forkEditor?: { engineAccountId: string | null } }
export interface RoutingFixedEditor {
  getSnapshot(): { botId: string; editor: { profile: FixedFields } | null; draft: FixedFields | null; saving: boolean; loading: boolean; error: string | null }
  subscribe(listener: () => void): () => void
}
export interface RoutingContext { botId: string; hubId: string | undefined; spaceId: string; fixedPending?: () => boolean }
const fixedKey = (value: FixedFields | null | undefined) => value ? JSON.stringify([value.backend ?? '', value.model ?? null, Object.entries(value.modelOptions ?? {}).sort(([a], [b]) => a.localeCompare(b)), value.forkEditor?.engineAccountId ?? null]) : null

const routingAllowed = (source: RoutingSource, context: RoutingContext) => !!source.connectedClient && !!context.hubId && source.hello?.hostId === context.hubId && (source.spaceId ?? ROUTING_PERSONAL) === context.spaceId

/** Recibo de um turno (só leitura); a chamada sai na hora, no cliente e no contexto atuais. */
export function readRoutingReceipt(source: RoutingSource, context: RoutingContext, request: RoutingReceiptRequest): Promise<RoutingReceiptResponse> {
  const client = source.connectedClient
  if (!client || !routingAllowed(source, context)) return Promise.reject(new Error('routing_context_changed: reabra a conversa.'))
  return client.call<RoutingReceiptResponse>('donnaRoutingReceipt', { conversationId: request.conversationId, sessionId: request.sessionId, entryId: request.entryId, botId: context.botId, hubId: context.hubId, spaceId: context.spaceId })
}

/** Sem fila/ready: cliente e contexto são fixados por operação antes de qualquer await. */
export function createRoutingStore(source: RoutingSource, context: RoutingContext, editor: RoutingFixedEditor) {
  const fixed: FixedBinding = {
    snapshot: () => {
      const state = editor.getSnapshot()
      const saved = fixedKey(state.editor?.profile), draft = fixedKey(state.draft)
      const mirror = fixedKey(source.bots.get(context.botId))
      return { key: JSON.stringify([saved, draft, mirror]), ready: state.botId === context.botId && saved !== null && saved === draft && saved === mirror && !state.saving && !state.loading && !state.error && !context.fixedPending?.() }
    },
    subscribe: (listener) => { const offEditor = editor.subscribe(listener), offSource = source.subscribe(listener); return () => { offEditor(); offSource() } },
  }
  const allowed = () => routingAllowed(source, context)
  const capture = (): RoutingOperation => {
    const client = source.connectedClient
    const epoch = source.principalEpoch
    const current = () => allowed() && source.connectedClient === client && source.principalEpoch === epoch
    const send = (method: string, body: Record<string, unknown>) => {
      if (!client || !current()) return Promise.reject(new Error('routing_context_changed: reabra as configurações.'))
      // A chamada acontece aqui, sincronicamente, no cliente capturado; nunca no cliente de uma reconexão.
      return client.call<RoutingView>(method, { ...body, botId: context.botId, hubId: context.hubId, spaceId: context.spaceId })
    }
    return { current, read: (check) => check && !fixed.snapshot().ready ? Promise.reject(new Error('routing_fixed_pending: aguarde salvar o Fixo.')) : send(check ? 'donnaRoutingCheck' : 'donnaRouting', {}), save: (request) => send('donnaRoutingSave', { ...request }) }
  }
  return new RoutingStore({ current: allowed, capture, read: (check) => capture().read(check), save: (request) => capture().save(request) }, fixed)
}
