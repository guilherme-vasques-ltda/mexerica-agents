import type { Engine } from '../../../../shared/fork-engine-accounts.ts'
import { accountTitle, engineName } from '../contas-ia/regras.ts'
export interface Binding { accountId: string; modelId: string; modelOptions: Record<string, string> }
export type Selection = ({ mode: 'custom' } & Binding) | { mode: 'automatic'; accountId: string | null; binding: Binding | null; alternativeAccountId?: string | null } | { mode: 'managedInVoice' }
export interface FunctionRow {
  definition: { id: string; name: string; managedInVoice: boolean; recipe: string }
  selection: Selection | null
  resolution: { status: 'available'; resolved: { effective: Binding } } | { status: 'unavailable'; unavailable: { reason: string } }
  /** Entrega 4 (MX-7): o Hub já manda; `background_worker` = a função roda hoje. Hub antigo: ausente. */
  consumer?: string
}
export interface Account { id: string; label: string; origin?: string; backend: string; state: string; billing: string; models: { id: string; options: { id: string; values: string[]; default: string | null }[] }[] }
export interface FunctionDocument { schemaVersion: 1; revision: number; functions: Record<string, Selection> }
export interface FunctionSnapshot { document: FunctionDocument | null; accounts: Account[]; accountsIssue: string | null; functions: FunctionRow[]; execution: string }
export interface SpaceSelection {
  spaceId: string; spaceName: string; spaceRevision: number; functionId: 'memory_consolidation'; revision: number
  state: 'not_authorized' | 'configured' | 'stale'; audience: 'owner_private'; execution: string
  current: { functionRevision: number; accountId: string; accountRevision: number; accountLabel: string; modelId: string } | null
}
export interface SpaceSnapshot { selections: SpaceSelection[]; execution: string }
export interface Draft { functionId: string; baseRevision: number; mode: 'automatic' | 'custom'; accountId: string; modelId: string; modelOptions: Record<string, string>; alternativeAccountId: string }
export type HistoryStatus = 'response_received' | 'applied' | 'unchanged' | 'proposed' | 'deferred' | 'failed'
export type HistoryStage = 'selection' | 'account' | 'usage' | 'execution' | 'revalidation'
export interface HistoryUsage { inputTokens: number; outputTokens: number; cachedInputTokens: number; cachedWriteTokens: number }
/** Projeção visual do histórico Local; não transporta conta, preço ou erro livre. */
export interface HistoryEntry { id: string; botId: string; function: 'memory' | 'titles' | 'daily_summary'; status: HistoryStatus; at: number; effectiveModels: string[]; usage: HistoryUsage | null; failureStage: HistoryStage | null }
/** DON-917 (D507): erro e aviso são códigos; a tela escolhe a frase (`fundo.erro.*`, `fundo.aviso.*`, `fundo.historico.erro.*`). */
export type BackgroundError = 'leitura' | 'indisponivel' | 'semRecurso'
export type BackgroundMessage = 'rascunho' | 'salvo' | 'naoConfirmado' | 'conferenciaFalhou' | 'espacoConcluido' | 'espacoNaoConfirmado'
export type HistoryError = 'indisponivel' | 'invalido'
export interface BackgroundState { data: FunctionSnapshot | null; spaces: SpaceSnapshot | null; error: BackgroundError | null; spacesError: 'indisponivel' | null; loading: boolean; busy: boolean; selectedId: string; draft: Draft | null; dirty: boolean; conflict: boolean; aviso: BackgroundMessage | null; check: FunctionRow['resolution'] | null; history: HistoryEntry[] | null; historyLoading: boolean; historyError: HistoryError | null }
export interface Transport { call<T>(method: string, body?: unknown): Promise<T> }
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const revision = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const fail = () => { throw new Error('background_invalid_answer') }
const validSpaceCurrent = (v: unknown): v is NonNullable<SpaceSelection['current']> => object(v) && revision(v.functionRevision) && revision(v.accountRevision) && typeof v.accountId === 'string' && !!v.accountId.trim() && typeof v.accountLabel === 'string' && !!v.accountLabel.trim() && typeof v.modelId === 'string' && !!v.modelId.trim()
const validBinding = (v: unknown) => object(v) && typeof v.accountId === 'string' && typeof v.modelId === 'string' && object(v.modelOptions) && Object.values(v.modelOptions).every((v) => typeof v === 'string')
const validSelection = (v: unknown) => object(v) && (v.mode === 'managedInVoice' || (v.mode === 'custom' && validBinding(v)) || (v.mode === 'automatic' && (v.accountId === null || typeof v.accountId === 'string') && (v.binding === null || validBinding(v.binding))))
/** Os códigos conhecidos; o rótulo é da tela (`fundo.historico.status.*`, `fundo.historico.etapa.*`). */
const HISTORY_STATUSES: readonly string[] = ['response_received', 'applied', 'unchanged', 'proposed', 'deferred', 'failed'] satisfies HistoryStatus[]
const HISTORY_STAGES: readonly string[] = ['selection', 'account', 'usage', 'execution', 'revalidation'] satisfies HistoryStage[]
const HISTORY_UNAVAILABLE: HistoryError = 'indisponivel'

/** Textos de cada tela (o Bot de fundo em português, a Memória por `useT`); a regra é a mesma. */
export interface AccountWords { legacy: string; subscription: string; refused: string; state: (state: string) => string }

/** Espelho explícito do Hub, `host/src/fork/background_selection.rs`: `resolve` só atende memória e títulos (linhas
 * 28-31) e recusa toda conta Codex nelas (linhas 51-52, `Problem::Codex`). `functionModels` não manda isso por conta.
 * As outras funções não passam por ali e continuam aceitando Codex (algumas usam Codex por padrão). */
export function codexRefused(functionId: string, account: Pick<Account, 'backend'>): boolean {
  return FUNCOES_DE_FUNDO.includes(functionId) && account.backend === 'codex'
}
/** Funções que aceitam a escolha explícita da API Anthropic. */
export const FUNCOES_DE_FUNDO: readonly string[] = ['memory_consolidation', 'bot_naming']
/** Por que uma conta não serve à função agora. */
export type Motivo = 'desconectada' | 'indisponivel' | 'naoConfirmada' | 'motorRecusado' | 'apiEmBreve' | 'semAssinatura' | 'semModelo'

export function motivoDaConta(account: Pick<Account, 'backend' | 'state' | 'billing' | 'models' | 'origin'>, functionId: string): Motivo | null {
  if (account.state === 'disconnected') return 'desconectada'
  if (account.state === 'unverified') return 'naoConfirmada'
  if (account.state !== 'connected') return 'indisponivel'
  if (codexRefused(functionId, account)) return 'motorRecusado'
  if (account.billing === 'paid_api' || account.billing === 'api') {
    const api = account.billing === 'paid_api' && account.backend === 'claude' && account.origin === 'managed' && FUNCOES_DE_FUNDO.includes(functionId)
    return api ? (account.models.length ? null : 'semModelo') : 'apiEmBreve'
  }
  if (account.billing !== 'subscription') return 'semAssinatura'
  return account.models.length ? null : 'semModelo'
}

/** O Automático (modelo vazio) tem receita que o Hub resolve nesta conta? Espelho de `config.rs::recipe`: o padrão Codex ainda
 * não é anunciado (`DefaultModelUnknown`, "AC ainda não anuncia isDefault"), e o do Claude é o `haiku` anunciado pela conta.
 * O motor também precisa ser o do Automático da função (`automaticBackend`). Sem prova, não resolve: nunca presume um modelo. */
export function automaticoResolvivel(account: Pick<Account, 'backend' | 'models' | 'billing'>, functionId: string): boolean {
  return account.billing === 'subscription' && account.backend === automaticBackend(functionId) && account.backend === 'claude' && account.models.some((m) => m.id === 'haiku')
}

/** Minutos parado antes da rodada da memória: `IDLE_MS` (30 min) em `host/src/fork/memory_auto/sleep.rs`; uma rodada por bot por dia de calendário. */
export const MEMORIA_OCIOSA_MIN = 30

/** 0: conectada com assinatura confirmada; 1: outra conectada; 2: conectada recusada pelo Hub nesta função; 3: não conectada
 * (inclusive Codex: a desconexão vem antes da recusa). */
const standing = (a: Account, functionId: string) => a.state !== 'connected' ? 3 : codexRefused(functionId, a) ? 2 : a.billing === 'subscription' ? 0 : 1

/** Contas dos seletores do Bot de fundo e da Memória: nome de Contas de IA (`accountTitle`), motor e estado.
 * Ordem: conectadas com assinatura confirmada, outras conectadas, recusadas pelo Hub, não conectadas (estável em cada grupo).
 * Recusada e não conectada ficam marcadas e desabilitadas, salvo se for a escolha salva. Dois textos nunca são iguais. */
export function accountOptions(accounts: Account[], selected: string, words: AccountWords, functionId: string): { id: string; label: string; disabled: boolean }[] {
  // Mesmo nome (mesmo motor): o identificador curto da conta vai junto do nome, mesmo que o estado difira.
  const names = new Map<string, number>()
  for (const a of accounts) { const k = accountTitle(a, words.legacy) + '|' + a.backend; names.set(k, (names.get(k) ?? 0) + 1) }
  const rows = [...accounts].sort((a, b) => standing(a, functionId) - standing(b, functionId)).map((a) => {
    const group = standing(a, functionId)
    const title = accountTitle(a, words.legacy)
    return {
      id: a.id,
      label: [names.get(title + '|' + a.backend)! > 1 ? `${title} ${shortId(a.id)}` : title, a.origin === 'legacy' ? null : engineName(a.backend as Engine), words.state(a.state), group === 0 ? words.subscription : group === 2 ? words.refused : null].filter(Boolean).join(' · '),
      disabled: group >= 2 && a.id !== selected,
    }
  })
  // Ainda repetido (mesmo id curto): o número.
  const total = new Map<string, number>(), seen = new Map<string, number>()
  for (const row of rows) total.set(row.label, (total.get(row.label) ?? 0) + 1)
  return rows.map((row) => {
    if (total.get(row.label)! < 2) return row
    const n = (seen.get(row.label) ?? 0) + 1
    seen.set(row.label, n)
    return { ...row, label: `${row.label} (${n})` }
  })
}
/** Duplicatas de nome ganham um identificador curto (fim do id da conta), para o leigo distinguir. */
const shortId = (id: string) => `#${id.replace(/[^a-zA-Z0-9]/g, '').slice(-4)}`

/** A primeira conta que o Hub aceita para a função, na mesma ordem dos seletores: a que `motivoDaConta` não recusa (conectada,
 * assinatura confirmada, sem API nem cobrança desconhecida, Codex fora da memória/nomes, com modelos informados). No modo
 * Automático (`automatico`) também precisa de receita que o Hub resolve (`automaticoResolvivel`). Sem conta elegível, null. */
export function suggestedAccount(accounts: Account[], functionId: string, automatico = false): Account | null {
  return [...accounts].sort((a, b) => standing(a, functionId) - standing(b, functionId)).find((a) => motivoDaConta(a, functionId) === null && (!automatico || automaticoResolvivel(a, functionId))) ?? null
}
/** A conta a que a escolha da função se refere (a salva ou a do rascunho). O Hub resolve `disconnected` e `unavailable` (cadastro)
 * como `account_disconnected`; a tela avisa nos dois. */
export const isDisconnected = (accounts: Account[], id: string) => accounts.some((a) => a.id === id && (a.state === 'disconnected' || a.state === 'unavailable'))
/** Conta desconectada: pelo estado cadastral ou pela resolução do Hub (`resolution.unavailable.reason`), que vale para a conta
 * a que a escolha salva se vincula (`savedAccountId`) — nunca para outra conta do rascunho. */
export function accountDisconnected(accounts: Account[], id: string, row: Pick<FunctionRow, 'selection' | 'resolution'>): boolean {
  if (!id) return false
  if (isDisconnected(accounts, id)) return true
  return row.resolution.status === 'unavailable' && row.resolution.unavailable.reason === 'account_disconnected' && savedAccountId(row.selection) === id
}
/** Motivos do Hub que, com a conta desconectada, só repetem o problema real: a conta precisa ser conectada. O Hub ainda
 * pode dizer "modelo padrão não informado" para uma conta desconectada; a tela diz "desconectada". */
const SUPERSEDED_BY_DISCONNECTED: readonly string[] = ['account_disconnected', 'default_model_unknown', 'capabilities_unconfirmed', 'model_missing', 'billing_unknown', 'subscription_unverified', 'quota_unknown']
/** O motivo mostrado: `account_disconnected` quando a conta vinculada (`selection.binding.accountId` ou a escolhida) está
 * desconectada na lista de contas, mesmo que o Hub ainda devolva `default_model_unknown` (MEX-1355). O motivo do Hub é a
 * STRING `resolution.unavailable.reason` de `functionModels`/`checkFunctionModel` (não há `reason.code` nem `accountId` ali). */
export function shownReason(reason: string | undefined, row: Pick<FunctionRow, 'selection'>, accounts: Account[]): string | undefined {
  const id = savedAccountId(row.selection)
  return reason && SUPERSEDED_BY_DISCONNECTED.includes(reason) && isDisconnected(accounts, id) ? 'account_disconnected' : reason
}
/** Motivo (string do Hub) → texto da Atividade, com os mesmos termos desta tela (MEX-1355). Só `account_disconnected` por ora. */
export const REASON_CODE_TEXT = { account_disconnected: 'fundo.atividade.motivo.account_disconnected' } as const
export const reasonCodeText = (code: unknown): (typeof REASON_CODE_TEXT)[keyof typeof REASON_CODE_TEXT] | null =>
  typeof code === 'string' && Object.hasOwn(REASON_CODE_TEXT, code) ? REASON_CODE_TEXT[code as keyof typeof REASON_CODE_TEXT] : null
/** Chave da tarefa em frase (`fundo.tarefa.*`): "a organização da memória", "dar nome ao bot"... */
export const taskKey = (id: string): `fundo.tarefa.${typeof KNOWN_FUNCTIONS[number] | 'generica'}` =>
  (KNOWN_FUNCTIONS as readonly string[]).includes(id) ? `fundo.tarefa.${id as typeof KNOWN_FUNCTIONS[number]}` : 'fundo.tarefa.generica'

export function readHistory(value: unknown): HistoryEntry[] {
  const invalid = () => { throw new Error('background_history_invalid') }
  if (!Array.isArray(value) || value.length > 50) return invalid()
  return value.map((row) => {
    if (!object(row) || typeof row.id !== 'string' || typeof row.botId !== 'string' || !['memory', 'titles', 'daily_summary'].includes(String(row.function)) || typeof row.status !== 'string' || !HISTORY_STATUSES.includes(row.status)) return invalid()
    if (!revision(row.at) || row.at > 8.64e15 || !Array.isArray(row.effectiveModels) || !row.effectiveModels.every((v) => typeof v === 'string' && !!v.trim())) return invalid()
    if (row.failureStage != null && (typeof row.failureStage !== 'string' || !HISTORY_STAGES.includes(row.failureStage))) return invalid()
    let usage: HistoryUsage | null = null
    if (row.usage != null) {
      if (!object(row.usage) || !revision(row.usage.inputTokens) || !revision(row.usage.outputTokens) || !revision(row.usage.cachedInputTokens) || !revision(row.usage.cachedWriteTokens)) return invalid()
      usage = { inputTokens: row.usage.inputTokens, outputTokens: row.usage.outputTokens, cachedInputTokens: row.usage.cachedInputTokens, cachedWriteTokens: row.usage.cachedWriteTokens }
    }
    return { id: row.id, botId: row.botId, function: row.function as HistoryEntry['function'], status: row.status as HistoryStatus, at: row.at,
      effectiveModels: [...row.effectiveModels] as string[], usage, failureStage: (row.failureStage ?? null) as HistoryStage | null }
  })
}
export function validResolution(v: unknown): v is FunctionRow['resolution'] {
  return object(v) && ((v.status === 'available' && object(v.resolved) && validBinding(v.resolved.effective)) || (v.status === 'unavailable' && object(v.unavailable) && typeof v.unavailable.reason === 'string'))
}

export function readFunctions(value: unknown): FunctionSnapshot {
  if (!object(value) || !Array.isArray(value.functions) || !Array.isArray(value.accounts) || typeof value.execution !== 'string') return fail()
  if (value.document !== null && (!object(value.document) || value.document.schemaVersion !== 1 || !revision(value.document.revision) || !object(value.document.functions) || !Object.values(value.document.functions).every(validSelection))) return fail()
  for (const account of value.accounts) {
    if (!object(account) || typeof account.id !== 'string' || typeof account.label !== 'string' || typeof account.backend !== 'string' || !Array.isArray(account.models)) return fail()
    for (const model of account.models) if (!object(model) || typeof model.id !== 'string' || !Array.isArray(model.options) || !model.options.every((option) => object(option) && typeof option.id === 'string' && Array.isArray(option.values) && option.values.every((v) => typeof v === 'string'))) return fail()
  }
  for (const row of value.functions) if (!object(row) || !object(row.definition) || typeof row.definition.id !== 'string' || typeof row.definition.name !== 'string' || typeof row.definition.recipe !== 'string' || typeof row.definition.managedInVoice !== 'boolean' || !validResolution(row.resolution) || (row.selection !== null && !validSelection(row.selection))) return fail()
  return value as unknown as FunctionSnapshot
}
/** Entrega 4 (MX-7): onde a função aparece. Em uso = o Hub diz que roda (`background_worker`); Em preparo = só guarda a
 * escolha (`awaiting_implementation`); Voz = gerenciada lá. Hub sem `consumer` ou com valor desconhecido: o grupo neutro
 * Funções, que não afirma se roda (revisão R1). */
export type Grupo = 'emUso' | 'emPreparo' | 'funcoes' | 'voz'
export const grupoDe = (row: Pick<FunctionRow, 'definition' | 'consumer'>): Grupo =>
  row.definition.managedInVoice ? 'voz' : row.consumer === 'background_worker' ? 'emUso' : row.consumer === 'awaiting_implementation' ? 'emPreparo' : 'funcoes'
export function readSpaces(value: unknown): SpaceSnapshot {
  if (!object(value) || !Array.isArray(value.selections) || typeof value.execution !== 'string') return fail()
  for (const row of value.selections) {
    if (!object(row) || typeof row.spaceId !== 'string' || typeof row.spaceName !== 'string' || !revision(row.revision) || !revision(row.spaceRevision) || row.functionId !== 'memory_consolidation' || row.audience !== 'owner_private' || !['not_authorized', 'configured', 'stale'].includes(String(row.state))) return fail()
    if (row.current !== null && !validSpaceCurrent(row.current)) return fail()
  }
  return value as unknown as SpaceSnapshot
}
export const automaticBackend = (id: string) => ['vision', 'context_compression', 'bot_draft'].includes(id) ? 'codex' : 'claude'
/** Por que o rascunho não vale na conta: Automático sem receita que o Hub resolve, modelo que a conta não tem, ou opção que o
 * modelo não tem mais. `null` = cabe. Não olha o estado da conta (`motivoDaConta`). */
export function falhaDoRascunho(account: Pick<Account, 'backend' | 'billing' | 'models'> | undefined, draft: Draft): 'conta' | 'automatico' | 'modelo' | 'opcao' | null {
  if (!account) return 'conta'
  if (draft.mode === 'automatic') return automaticoResolvivel(account, draft.functionId) ? null : 'automatico'
  const model = account.models.find((m) => m.id === draft.modelId)
  if (!model) return 'modelo'
  return Object.entries(draft.modelOptions).every(([key, value]) => model.options.some((o) => o.id === key && o.values.includes(value))) ? null : 'opcao'
}
/** O rascunho cabe no catálogo da conta (usado também pelo cartão Memória, D389). */
export function validDraft(data: FunctionSnapshot, draft: Draft) {
  if (data.accountsIssue) return false
  const account = data.accounts.find((a) => a.id === draft.accountId)
  if (draft.mode === 'automatic') return (!draft.accountId || falhaDoRascunho(account, draft) === null) && (!draft.alternativeAccountId || data.accounts.some((a) => a.id === draft.alternativeAccountId && a.backend === 'claude'))
  return falhaDoRascunho(account, draft) === null
}
/** A conta da escolha salva (a que continua escolhível mesmo sem conexão); `alternative`: a da alternativa Haiku. */
export function savedAccountId(selection?: Selection | null, alternative = false): string {
  if (selection?.mode === 'custom') return alternative ? '' : selection.accountId
  if (selection?.mode !== 'automatic') return ''
  return (alternative ? selection.alternativeAccountId : selection.accountId ?? selection.binding?.accountId) ?? ''
}
export function bindingOf(selection?: Selection | null): Binding | null { return selection?.mode === 'custom' ? selection : selection?.mode === 'automatic' ? selection.binding : null }
/** As funções que o app conhece têm nome no catálogo (`fundo.funcao.<id>`); outra fica com o nome que o Hub mandou. */
export const KNOWN_FUNCTIONS = ['vision', 'context_compression', 'message_triage', 'skills_hub', 'skill_selection', 'approval_judge', 'mcp_routing', 'memory_consolidation', 'bot_naming', 'bot_draft', 'voice_live', 'speech_to_text', 'text_to_speech'] as const
export function functionName(row: FunctionRow): { key: `fundo.funcao.${typeof KNOWN_FUNCTIONS[number]}` } | { label: string } {
  const id = row.definition.id as typeof KNOWN_FUNCTIONS[number]
  return KNOWN_FUNCTIONS.includes(id) ? { key: `fundo.funcao.${id}` } : { label: row.definition.name }
}
const REASONS = ['account_missing', 'account_choice_required', 'account_disconnected', 'billing_unknown', 'api_not_enabled', 'subscription_unverified', 'quota_unknown', 'quota_exhausted', 'model_missing', 'option_unsupported', 'capabilities_unconfirmed', 'model_incompatible', 'default_model_unknown', 'context_denied', 'configuration_unavailable', 'accounts_unavailable', 'revision_changed'] as const
/** O motivo de indisponibilidade como chave do catálogo (`fundo.motivo.*`); a tela traduz. */
export type ReasonKey = `fundo.motivo.${typeof REASONS[number] | 'semConta' | 'semContaMemoria' | 'desconhecido'}`
export function reasonText(reason?: string, row?: Pick<FunctionRow, 'definition' | 'selection'>): ReasonKey {
  if (reason === 'account_missing' && row?.selection?.mode === 'automatic' && row.selection.accountId === null && row.selection.binding === null) {
    return ['memory_consolidation', 'bot_naming'].includes(row.definition.id) ? 'fundo.motivo.semContaMemoria' : 'fundo.motivo.semConta'
  }
  const known = REASONS.find((r) => r === reason)
  return known ? `fundo.motivo.${known}` : 'fundo.motivo.desconhecido'
}
export function draftOf(row: FunctionRow, baseRevision: number): Draft | null {
  const s = row.selection
  if (!s || s.mode === 'managedInVoice' || row.definition.managedInVoice) return null
  return { functionId: row.definition.id, baseRevision, mode: s.mode, accountId: s.accountId ?? '', modelId: s.mode === 'custom' ? s.modelId : '', modelOptions: s.mode === 'custom' ? { ...s.modelOptions } : {}, alternativeAccountId: s.mode === 'automatic' ? s.alternativeAccountId ?? '' : '' }
}
export function selectionOf(draft: Draft) {
  return draft.mode === 'custom' ? { mode: 'custom', accountId: draft.accountId, modelId: draft.modelId, modelOptions: draft.modelOptions }
    : { mode: 'automatic', ...(draft.accountId ? { accountId: draft.accountId } : {}), ...(draft.functionId === 'bot_draft' && draft.alternativeAccountId ? { alternativeAccountId: draft.alternativeAccountId } : {}) }
}
/** Hub antigo: o método ainda não existe lá. Não é falha de leitura; é falta de recurso. */
export const metodoDesconhecido = (e: unknown) => e instanceof Error && /^(unknown[ _]method|method_not_found)\b/i.test(e.message.trim())
const initial = (): BackgroundState => ({ data: null, spaces: null, error: null, spacesError: null, loading: false, busy: false, selectedId: 'memory_consolidation', draft: null, dirty: false, conflict: false, aviso: null, check: null, history: null, historyLoading: false, historyError: null })

/** Dados administrativos efêmeros; cada abertura pertence a um computador/conexão. */
export class BackgroundStore {
  private value = initial()
  private listeners = new Set<() => void>()
  private generation = 0
  private reading = 0
  private api: () => Promise<Transport>
  private scope: () => string
  private owner: string
  private link?: () => Transport | null
  private ownerLink?: Transport | null
  constructor(api: () => Promise<Transport>, scope = () => '', link?: () => Transport | null) {
    this.api = api; this.scope = scope; this.owner = scope(); this.link = link; this.ownerLink = link?.()
  }
  private current(generation: number) {
    return generation === this.generation && this.scope() === this.owner && (!this.link || (!!this.ownerLink && this.link() === this.ownerLink))
  }
  private async client(generation: number) {
    if (!this.current(generation)) throw new Error('scope_changed')
    const client = await this.api()
    if (!this.current(generation)) throw new Error('scope_changed')
    return client
  }
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getSnapshot = () => this.value
  private update(patch: Partial<BackgroundState>) { this.value = { ...this.value, ...patch }; for (const listener of this.listeners) listener() }
  close() { this.generation++; this.reading++; this.update(initial()) }
  async load(resetDraft = false) {
    const generation = this.generation
    if (!this.current(generation)) { this.update({ history: null, historyLoading: false, historyError: HISTORY_UNAVAILABLE }); return }
    const reading = ++this.reading
    this.update({ loading: true, check: null, history: null, historyLoading: true, historyError: null })
    try {
      const client = await this.client(generation)
      // Consulta independente, com erros tratados e cercas próprias; não prolonga a mutação.
      void this.loadHistory(client, generation, reading)
      const [functions, spaces] = await Promise.allSettled([client.call('functionModels', {}), client.call('backgroundSpaceSelections', {})])
      if (!this.current(generation) || reading !== this.reading) return
      let data: FunctionSnapshot | null = null, spaceData: SpaceSnapshot | null = null, error: BackgroundError | null = null, spacesError: 'indisponivel' | null = null
      try { if (functions.status === 'rejected') throw functions.reason; data = readFunctions(functions.value) } catch (e) { error = metodoDesconhecido(e) ? 'semRecurso' : 'leitura' }
      try { if (spaces.status === 'rejected') throw spaces.reason; spaceData = readSpaces(spaces.value) } catch { spacesError = 'indisponivel' }
      const dirty = !resetDraft && this.value.dirty
      const row = data?.functions.find((f) => f.definition.id === this.value.selectedId) ?? data?.functions[0]
      this.update({ data, spaces: spaceData, error, spacesError, loading: false, selectedId: row?.definition.id ?? this.value.selectedId,
        draft: dirty ? this.value.draft : row && data?.document ? draftOf(row, data.document.revision) : null,
        dirty, conflict: dirty && (!data?.document || this.value.draft?.baseRevision !== data.document.revision), ...(resetDraft ? { aviso: null, check: null } : {}) })
    } catch {
      if (this.current(generation) && reading === this.reading) this.update({ data: null, spaces: null, loading: false, error: 'indisponivel', spacesError: 'indisponivel', history: null, historyLoading: false, historyError: HISTORY_UNAVAILABLE })
    }
  }
  private async loadHistory(client: Transport, generation: number, reading: number) {
    try {
      const raw = await client.call('backgroundHistory', {})
      if (!this.current(generation) || reading !== this.reading) return
      let history: HistoryEntry[]
      try { history = readHistory(raw) } catch {
        this.update({ history: null, historyLoading: false, historyError: 'invalido' })
        return
      }
      this.update({ history, historyLoading: false, historyError: null })
    } catch {
      if (this.current(generation) && reading === this.reading) this.update({ history: null, historyLoading: false, historyError: HISTORY_UNAVAILABLE })
    }
  }
  select(id: string) {
    if (this.value.busy || id === this.value.selectedId) return
    if (this.value.dirty && id !== this.value.selectedId) { this.update({ aviso: 'rascunho' }); return }
    const row = this.value.data?.functions.find((f) => f.definition.id === id)
    if (row) this.update({ selectedId: id, draft: this.value.data?.document ? draftOf(row, this.value.data.document.revision) : null, dirty: false, conflict: false, check: null, aviso: null })
  }
  /** A escolha do seletor (conta e modelo juntos): troca os quatro campos de uma vez, sem a limpeza em cadeia de `change`. */
  choose(choice: Pick<Draft, 'mode' | 'accountId' | 'modelId' | 'modelOptions'>) {
    if (!this.value.draft || this.value.busy) return
    this.update({ draft: { ...this.value.draft, ...choice }, dirty: true, check: null, aviso: null })
  }
  change(patch: Partial<Pick<Draft, 'mode' | 'accountId' | 'modelId' | 'modelOptions' | 'alternativeAccountId'>>) {
    if (!this.value.draft || this.value.busy) return
    const draft = { ...this.value.draft, ...patch }
    if (patch.accountId !== undefined || patch.mode !== undefined) { draft.modelId = ''; draft.modelOptions = {} }
    if (patch.modelId !== undefined) draft.modelOptions = {}
    this.update({ draft, dirty: true, check: null, aviso: null })
  }
  async save() {
    const { draft, data, busy, conflict } = this.value
    if (!draft || !data?.document || busy || conflict || draft.baseRevision !== data.document.revision) return
    const generation = this.generation
    if (!this.current(generation)) return
    this.update({ busy: true, aviso: null })
    try {
      const result = await (await this.client(generation)).call<{ document?: FunctionDocument }>('saveFunctionModel', { expectedRevision: draft.baseRevision, functionId: draft.functionId, selection: selectionOf(draft) })
      if (!object(result.document) || !revision(result.document.revision) || result.document.revision <= draft.baseRevision || !object(result.document.functions) || !validSelection(result.document.functions[draft.functionId])) throw new Error('unconfirmed')
      if (!this.current(generation)) return
      // Relendo: até a releitura terminar, `data` ainda traz a resolução da escolha anterior (R1-01).
      this.update({ dirty: false, conflict: false, loading: true, aviso: 'salvo' })
      await this.load()
    } catch { if (this.current(generation)) this.update({ conflict: true, aviso: 'naoConfirmado' }) }
    finally { if (this.current(generation)) this.update({ busy: false }) }
  }
  async check() {
    if (!this.value.data || this.value.busy || this.value.dirty || this.value.loading) return
    const generation = this.generation, reading = this.reading, id = this.value.selectedId
    if (!this.current(generation)) return
    this.update({ busy: true, aviso: null })
    try {
      const result = await (await this.client(generation)).call<FunctionRow['resolution']>('checkFunctionModel', { functionId: id })
      if (!validResolution(result)) throw new Error('invalid')
      if (this.current(generation) && reading === this.reading && id === this.value.selectedId) this.update({ check: result })
    } catch { if (this.current(generation) && reading === this.reading) this.update({ aviso: 'conferenciaFalhou' }) }
    finally { if (this.current(generation)) this.update({ busy: false }) }
  }
  async saveSpace(row: SpaceSelection, enabled: boolean) {
    if (this.value.busy || (enabled && !canBind(this.value.data, row))) return
    const generation = this.generation
    if (!this.current(generation)) return
    this.update({ busy: true, aviso: null })
    try {
      const result = await (await this.client(generation)).call('saveBackgroundSpaceSelection', { spaceId: row.spaceId, functionId: row.functionId, expectedRevision: row.revision, expectedSpaceRevision: row.spaceRevision, enabled, ...(enabled && row.current ? { functionRevision: row.current.functionRevision, accountId: row.current.accountId, accountRevision: row.current.accountRevision } : {}) })
      readSpaces(result)
      if (this.current(generation)) this.update({ aviso: 'espacoConcluido' })
    } catch { if (this.current(generation)) this.update({ aviso: 'espacoNaoConfirmado' }) }
    finally {
      // O evento pode chegar antes da resposta. O snapshot da mutação nunca substitui uma leitura mais nova.
      if (this.current(generation)) await this.load()
      if (this.current(generation)) this.update({ busy: false })
    }
  }
}
/** Regra do dono (15/11): cada Space tem as suas configurações. Só a seleção do Space ativo aparece; as dos outros ficam intactas no Hub. */
export const selecaoDoSpace = (selections: readonly SpaceSelection[], spaceId: string | null): SpaceSelection | null => (spaceId ? selections.find((row) => row.spaceId === spaceId) ?? null : null)
/** O Space do cartão "Neste Space": o ativo no topo, no computador e no Hub dele; sem ativo, o Pessoal aberto.
 * Ativo que não bate com este computador/Hub: `null` (aguarda; nunca cai no Pessoal de outro). */
export interface SpaceAtivo { computerId: string; hostId: string | null; spaceId: string }
export function spaceDaPagina(ativo: SpaceAtivo | null, computerId: string, hostId: string | null | undefined, pessoal: string | null | undefined): string | null {
  if (!ativo) return pessoal ?? null
  return ativo.computerId === computerId && ativo.hostId === (hostId ?? null) ? ativo.spaceId : null
}
/** O computador da página: o do Space ativo; sem ativo (Pessoal aberto), o escolhido. */
export const computadorDaPagina = (ativo: SpaceAtivo | null, escolhido: string) => ativo ? ativo.computerId : escolhido
export function canBind(data: FunctionSnapshot | null, row: SpaceSelection) {
  return !!row.current && !spaceChoiceIssue(data, row)
}
/** Por que o vínculo não pode ser feito agora, como chave do catálogo; null se pode. */
export function spaceChoiceIssue(data: FunctionSnapshot | null, row: SpaceSelection): 'fundo.espaco.escolhaIndisponivel' | 'fundo.espaco.divergem' | null {
  if (!validSpaceCurrent(row.current)) return 'fundo.espaco.escolhaIndisponivel'
  if (!data) return null // A projeção administrativa do Space é suficiente; functionModels continua Local-only.
  const binding = bindingOf(data?.document?.functions.memory_consolidation)
  return !binding || data.document?.revision !== row.current.functionRevision || binding.accountId !== row.current.accountId || binding.modelId !== row.current.modelId
    ? 'fundo.espaco.divergem' : null
}
/** A escolha do espaço: a conta e o modelo informados (dado), ou a chave do estado sem escolha. */
export function spaceChoiceText(data: FunctionSnapshot | null, row: SpaceSelection): { label: string } | { key: 'fundo.espaco.escolhaNaoInformada' | 'fundo.espaco.aguardandoReleitura' } {
  if (!validSpaceCurrent(row.current)) return { key: 'fundo.espaco.escolhaNaoInformada' }
  if (spaceChoiceIssue(data, row)) return { key: 'fundo.espaco.aguardandoReleitura' }
  return { label: `${row.current.accountLabel} · ${row.current.modelId}` }
}
