// Rota-03b (D354, opção A): textos e regras do recibo do turno. Puro: sem I/O nem React.
import type { Entry } from '../../../../shared/models.ts'
import type { RoutingReceipt } from '../../../../shared/fork-roteamento.ts'
import type { ReceiptLoad, ReceiptRequest } from '../../../store/fork-recibo.ts'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'

/** A resposta que abre um turno do bot. `request` nulo: turno sem pedido humano (rotina, grupo), sem recibo. */
export interface ReceiptAnchor {
  request: ReceiptRequest | null; previous: ReceiptRequest | null
  /** D371: a maior versão das regras de delegação em vigor quando este bot delegou neste turno (aviso do remetente). */
  rulesVersion?: number
}

export interface AccountName { id: string; label: string }

export type ReceiptView =
  | { kind: 'loading'; label: string }
  | { kind: 'routed'; label: string; model: string; account: string; accountGone: boolean; engine: string; effort: string | null; tag: { tone: 'fixo' | 'conversa' | 'sessao'; text: string }; why: string; motorLink: string | null }
  | { kind: 'plain'; label: string; title: string; body: string; note: string | null; warn: boolean; retry: boolean }

// Textos em conversa.recibo.* (lib/fork-i18n/<idioma>/conversa/recibo.ts); a nota e o título do popover são
// `conversa.recibo.nota` e `conversa.recibo.titulo`.
const EFFORT = { low: 'conversa.recibo.esforco.low', medium: 'conversa.recibo.esforco.medium', high: 'conversa.recibo.esforco.high' } as const
const EFFORT_KEYS = ['reasoning_effort', 'effort']
// Nomes próprios dos motores (produtos): iguais em todo idioma.
const ENGINE: Record<string, string> = {
  codex: 'Codex',
  claude: 'Claude Code', // textos-ok: nome de produto, igual em todo idioma
}

/**
 * A primeira resposta final de cada autor em cada turno. O Hub dá à resposta o `turn` da última
 * entrada humana do lote (`agent/bot/turn.rs`), e o recibo é achado por qualquer entrada do lote.
 * Entradas locais (`local-…`, turno 0) ainda não existem no Hub.
 */
export function receiptAnchors(entries: readonly Entry[]): Map<string, ReceiptAnchor> {
  const users = new Map<string, Entry>()
  for (const e of entries) if (e.kind === 'user' && !e.id.startsWith('local-') && e.turn > 0) users.set(`${e.turn}|${e.sessionId ?? e.botId}`, e)
  // D371: o aviso de ask_bot/message_bot do remetente traz `delegationRulesVersion` (`host/src/fork/delegacao`).
  const rules = new Map<string, number>()
  for (const e of entries) {
    const v = e.kind === 'notice' ? e.data.delegationRulesVersion : undefined
    if (typeof v !== 'number' || !e.data.sourceBotId) continue
    const k = `${e.turn}|${e.sessionId ?? e.botId}|${e.data.sourceBotId}`
    rules.set(k, Math.max(v, rules.get(k) ?? 0))
  }
  const seen = new Set<string>()
  const out = new Map<string, ReceiptAnchor>()
  const last = new Map<string, ReceiptRequest>()
  for (const e of entries) {
    if (e.kind !== 'agent' || e.data.final !== true) continue
    const session = e.sessionId ?? e.botId
    const author = e.data.author ?? e.botId
    const turnKey = `${e.turn}|${session}|${author}`
    if (seen.has(turnKey)) continue
    seen.add(turnKey)
    const user = users.get(`${e.turn}|${session}`)
    // `Entry.botId` é o chat da entrada (a conversa aberta; num Space, a conversa, não o bot que responde).
    const request = user ? { botId: author, conversationId: user.botId, sessionId: user.sessionId ?? user.botId, entryId: user.id } : null
    const lane = `${session}|${author}`
    const rulesVersion = rules.get(turnKey)
    out.set(e.id, { request, previous: last.get(lane) ?? null, ...(rulesVersion ? { rulesVersion } : {}) })
    if (request) last.set(lane, request)
  }
  return out
}

/** Reaproveita as âncoras que não mudaram, para o `memo` das linhas continuar valendo. */
export function stableAnchors(previous: ReadonlyMap<string, ReceiptAnchor>, next: Map<string, ReceiptAnchor>): Map<string, ReceiptAnchor> {
  // Não muta `next`: o mapa pode vir do cache do chatView (compartilhado entre instâncias).
  const out = new Map<string, ReceiptAnchor>()
  for (const [id, anchor] of next) {
    const old = previous.get(id)
    out.set(id, old && JSON.stringify(old) === JSON.stringify(anchor) ? old : anchor)
  }
  return out
}

/** O nome do modelo fica ao lado do ícone só quando muda em relação ao turno anterior com recibo. */
export function changedModel(t: Tradutor, current: ReceiptLoad | undefined, previous: ReceiptLoad | undefined): string | null {
  const now = routed(current), before = routed(previous)
  if (!now || !before) return null
  const a = now.admitted.selection.model ?? null, b = before.admitted.selection.model ?? null
  return a !== b ? a ?? modelOf(t, now) : null
}

/** O ícone aparece com recibo, na fila ou com erro de leitura; nunca sem recibo nem antes de ler. */
export function showsIcon(load: ReceiptLoad | undefined): boolean {
  if (!load || load.status === 'loading') return false
  return load.status === 'error' || load.response.state !== 'notRouted'
}

export function receiptView(t: Tradutor, load: ReceiptLoad | undefined, accounts: readonly AccountName[] | null | 'failed', botName: string, hasRequest = true): ReceiptView {
  if (!hasRequest) return notRouted(t, botName)
  if (!load || load.status === 'loading') return { kind: 'loading', label: t('conversa.recibo.lendo') }
  if (load.status === 'error') {
    return { kind: 'plain', warn: true, retry: true, label: t('conversa.recibo.erro.rotulo'), title: t('conversa.recibo.erro.titulo'), body: t('conversa.recibo.erro.corpo'), note: t('conversa.recibo.erro.nota', { erro: hubError(t, load.code) }) }
  }
  const { response } = load
  if (response.state === 'pending') {
    return { kind: 'plain', warn: false, retry: false, label: t('conversa.recibo.pendente.rotulo'), title: t('conversa.recibo.pendente.titulo'), body: t('conversa.recibo.pendente.corpo'), note: null }
  }
  const r = routed(load)
  if (!r) return notRouted(t, botName)
  const selection = r.admitted.selection
  const model = modelOf(t, r)
  // `null`: a lista ainda carrega; `failed`: não foi lida, então não dá para dizer que a conta saiu.
  const list = Array.isArray(accounts) ? accounts as readonly AccountName[] : null
  const found = list?.find((a) => a.id === r.admitted.accountId)
  const account = accounts === null ? '…' : accounts === 'failed' ? t('conversa.recibo.conta.indisponivel') : found ? found.label : t('conversa.recibo.conta.removida')
  const effort = effortOf(t, selection.modelOptions)
  const engine = ENGINE[selection.backend] ?? selection.backend
  // Sessão extra (id ≠ chat): o 1º turno chega como Fixo (o PR4 só fixa a escolha depois), mas a sessão
  // guardou essa combinação. Sempre "Fixada nesta sessão", sem o atalho para o Motor.
  const why0 = r.destination.session.id !== r.destination.session.chatId ? 'frozenSessionSelection' : r.reason
  const [tone, tag, why] = reason(t, why0, botName)
  // Frase acessível inteira por combinação (com/sem conta, com/sem esforço), sem montar pedaços.
  const conta = list ? (found ? found.label : t('conversa.recibo.conta.removidaCurta')) : null
  const esforco = effort?.toLocaleLowerCase()
  const label = conta && esforco ? t('conversa.recibo.aria.contaEsforco', { modelo: model, conta, esforco, etiqueta: tag })
    : conta ? t('conversa.recibo.aria.conta', { modelo: model, conta, etiqueta: tag })
      : esforco ? t('conversa.recibo.aria.esforco', { modelo: model, esforco, etiqueta: tag })
        : t('conversa.recibo.aria.modelo', { modelo: model, etiqueta: tag })
  return { kind: 'routed', label, model, account, accountGone: !!list && !found, engine, effort, tag: { tone, text: tag }, why, motorLink: why0 === 'fixedSelection' ? t('conversa.recibo.abrirMotor', { bot: botName }) : null }
}

const notRouted = (t: Tradutor, bot: string): ReceiptView => ({ kind: 'plain', warn: false, retry: false, label: t('conversa.recibo.semRecibo.titulo'), title: t('conversa.recibo.semRecibo.titulo'), body: t('conversa.recibo.semRecibo.corpo', { bot }), note: t('conversa.recibo.semRecibo.nota') })

/** Só os três motivos entregues; Auto, Reserva e Dono ganham etiqueta quando existirem. */
function reason(t: Tradutor, kind: RoutingReceipt['reason'], bot: string): ['fixo' | 'conversa' | 'sessao', string, string] {
  switch (kind) {
    case 'explicitSelection': return ['conversa', t('conversa.recibo.motivo.conversa'), t('conversa.recibo.motivo.conversaPorque')]
    case 'frozenSessionSelection': return ['sessao', t('conversa.recibo.motivo.sessao'), t('conversa.recibo.motivo.sessaoPorque', { bot })]
    default: return ['fixo', t('conversa.recibo.motivo.fixo'), t('conversa.recibo.motivo.fixoPorque', { bot })]
  }
}

function routed(load: ReceiptLoad | undefined): RoutingReceipt | null {
  return load?.status === 'loaded' && load.response.state === 'routed' ? load.response.receipt : null
}
const modelOf = (t: Tradutor, r: RoutingReceipt) => r.admitted.selection.model ?? t('conversa.recibo.modeloPadrao')

function effortOf(t: Tradutor, options: Record<string, string>): string | null {
  const key = EFFORT_KEYS.find((k) => options[k])
  if (!key) return null
  const value = options[key]!
  return Object.hasOwn(EFFORT, value) ? t(EFFORT[value as keyof typeof EFFORT]) : value
}

/** Erro do Hub em frase curta; o código técnico não aparece. */
export function hubError(t: Tradutor, code: string): string {
  if (code === 'routing_context_changed') return t('conversa.recibo.hub.contexto')
  if (code === 'remote_setup_unavailable') return t('conversa.recibo.hub.semAutorizacao')
  if (code === 'unknown method') return t('conversa.recibo.hub.semRecibo')
  return t('conversa.recibo.hub.falhou')
}
