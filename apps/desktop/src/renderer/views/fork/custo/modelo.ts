// DON-908 (C8, docs/reference/design/don-908-c8/README.md): o custo estimado do Hub (`donnaCostReport`, só leitura e só do
// dono), sem DOM nem React. O Hub calcula tudo: totais de mãe, bot e mês já sem dupla contagem; a UI nunca soma linhas
// nem converte moedas. `costs: []` é sem dinheiro (nunca 0 inventado); token `null` é sem dado (nunca 0). Nada vai
// para o mirror, o hello ou o cache: a página e o balão releem ao abrir.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { EngineAccount } from '../../../../shared/fork-engine-accounts.ts'
import { formatarData, formatarMoeda, formatarNumero } from '../../../lib/fork-i18n/index.ts'

export const RPC_CUSTO = 'donnaCostReport'

export interface Custo { amount: number; currency: string; source: string }
export interface Totais {
  turns: number
  /** Turnos sem preço do motor; com `costs` não vazio, a soma em dinheiro é parcial. */
  unpricedTurns: number
  inputTokens: number | null
  cachedReadTokens: number | null
  cachedWriteTokens: number | null
  outputTokens: number | null
  /** Uma entrada por moeda, sem conversão. */
  costs: Custo[]
  /** Estimativas por moeda, separadas dos valores do motor; ausente em Hubs antigos. */
  estimatedCosts?: Custo[]
}
export interface LinhaBot { botId: string; name: string; totals: Totais }
export interface LinhaConversa { botId: string; chatId: string; sessionId: string; title: string; parentSessionId: string | null; totals: Totais }
/** MX-145 E2: o grupo por conta do Hub; `accountId` null = turnos sem conta capturada (nunca inferida). */
export interface LinhaConta { accountId: string | null; totals: Totais }
/** MX-145 E2: o grupo por modelo do Hub; `model` null = o motor não informou o modelo. */
export interface LinhaModelo { model: string | null; totals: Totais }
export interface RelatorioCusto {
  hubId: string
  measuredAt: number
  month: string
  total: Totais
  bots: LinhaBot[]
  /** `null`: Hub antigo, que não manda `accounts`/`models` (a view degrada para o que existe). Ordem é a do Hub. */
  contas: LinhaConta[] | null
  modelos: LinhaModelo[] | null
  conversations: LinhaConversa[]
  helpers: LinhaConversa[]
  conversation: LinhaConversa | null
}

/** O pedido: mês da página (`month` ausente = mês corrente UTC no Hub) ou a conversa do balão. */
export type Pedido = { month?: string } | { botId: string; sessionId: string | null }

/** O corpo do RPC: a principal (`sessionId` null) vai sem `sessionId`; uma extra vai exata, sem inferência. */
export function corpoDo(pedido: Pedido): Record<string, string> {
  if ('botId' in pedido) return pedido.sessionId !== null ? { botId: pedido.botId, sessionId: pedido.sessionId } : { botId: pedido.botId }
  return pedido.month ? { month: pedido.month } : {}
}

export const INVALIDA = 'cost_invalid_answer'
const invalida = (): never => { throw new Error(INVALIDA) }
const obj = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : invalida())
const inteiro = (v: unknown): number => (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : invalida())
const talvez = (v: unknown): number | null => (v === null ? null : inteiro(v))
const texto = (v: unknown): string => (typeof v === 'string' ? v : invalida())
const id = (v: unknown): string => (typeof v === 'string' && v ? v : invalida())
const lista = <T>(v: unknown, f: (x: unknown) => T): T[] => (Array.isArray(v) ? v.map(f) : invalida())
const MES = /^\d{4}-(0[1-9]|1[0-2])$/

function custo(v: unknown): Custo {
  const o = obj(v)
  const amount = typeof o.amount === 'number' && Number.isFinite(o.amount) && o.amount >= 0 ? o.amount : invalida()
  const currency = typeof o.currency === 'string' && /^[A-Z]{3}$/.test(o.currency) ? o.currency : invalida()
  return { amount, currency, source: id(o.source) }
}

function totais(v: unknown): Totais {
  const o = obj(v)
  const costs = lista(o.costs, custo)
  const estimatedCosts = o.estimatedCosts === undefined ? [] : lista(o.estimatedCosts, custo)
  // Moedas separadas: duas entradas na mesma moeda seriam uma soma que a UI teria de fazer.
  if ([costs, estimatedCosts].some((cs) => new Set(cs.map((c) => c.currency)).size !== cs.length)) invalida()
  if (estimatedCosts.some((c) => c.source !== 'estimado')) invalida()
  const t = { turns: inteiro(o.turns), unpricedTurns: inteiro(o.unpricedTurns), inputTokens: talvez(o.inputTokens), cachedReadTokens: talvez(o.cachedReadTokens), cachedWriteTokens: talvez(o.cachedWriteTokens), outputTokens: talvez(o.outputTokens), costs, estimatedCosts }
  if (t.unpricedTurns > t.turns) invalida()
  return t
}

const linhaBot = (v: unknown): LinhaBot => { const o = obj(v); return { botId: id(o.botId), name: texto(o.name), totals: totais(o.totals) } }
const linhaConta = (v: unknown): LinhaConta => { const o = obj(v); return { accountId: o.accountId === null ? null : id(o.accountId), totals: totais(o.totals) } }
const linhaModelo = (v: unknown): LinhaModelo => { const o = obj(v); return { model: o.model === null ? null : id(o.model), totals: totais(o.totals) } }
function linhaConversa(v: unknown): LinhaConversa {
  const o = obj(v)
  return { botId: id(o.botId), chatId: id(o.chatId), sessionId: id(o.sessionId), title: texto(o.title), parentSessionId: o.parentSessionId === null ? null : id(o.parentSessionId), totals: totais(o.totals) }
}

/**
 * Valida a resposta inteira contra o pedido; qualquer campo fora do contrato recusa tudo (`cost_invalid_answer`).
 * Outro Hub, outro mês ou outra conversa/sessão também recusam: nunca números de outro lugar.
 */
export function lerRelatorio(raw: unknown, pedido: Pedido, hubId: string): RelatorioCusto {
  const o = obj(raw)
  // Só `accounts` ou só `models` não é nenhum Hub que existe: recusa. Os dois ausentes é um Hub antigo.
  if ((o.accounts === undefined) !== (o.models === undefined)) invalida()
  const r: RelatorioCusto = {
    hubId: id(o.hubId),
    measuredAt: inteiro(o.measuredAt),
    month: typeof o.month === 'string' && MES.test(o.month) ? o.month : invalida(),
    total: totais(o.total),
    bots: lista(o.bots, linhaBot),
    contas: o.accounts === undefined ? null : lista(o.accounts, linhaConta),
    modelos: o.models === undefined ? null : lista(o.models, linhaModelo),
    conversations: lista(o.conversations, linhaConversa),
    helpers: lista(o.helpers, linhaConversa),
    conversation: o.conversation === null ? null : linhaConversa(o.conversation),
  }
  if (r.hubId !== hubId) invalida()
  if (r.helpers.some((h) => h.parentSessionId === null)) invalida()
  // Cada grupo aparece uma vez (dois iguais seriam uma soma que a UI teria de fazer) e os grupos cobrem o mês: conta e
  // modelo nulos têm grupo próprio, então cada turno cai em exatamente um grupo (a soma dos turnos fecha com o total).
  // Só se confere a contagem de turnos; dinheiro e tokens nunca são recalculados aqui.
  for (const [chaves, grupos] of [[r.contas?.map((c) => c.accountId), r.contas], [r.modelos?.map((m) => m.model), r.modelos]] as const) {
    if (!chaves || !grupos) continue
    if (new Set(chaves).size !== chaves.length) invalida()
    if (grupos.reduce((n, g) => n + g.totals.turns, 0) !== r.total.turns) invalida()
  }
  if ('botId' in pedido) {
    const c = r.conversation
    // O balão pede só a conversa pessoal (sem `conversationId`): `chatId` = o bot. A principal volta com `sessionId` =
    // `chatId`; uma extra volta exatamente com o id pedido. Outro chat (Space do mesmo bot) é recusado.
    if (c && (c.botId !== pedido.botId || c.chatId !== pedido.botId || (pedido.sessionId === null ? c.sessionId !== c.chatId : c.sessionId !== pedido.sessionId))) invalida()
  } else if (pedido.month && r.month !== pedido.month) invalida()
  return r
}

/**
 * O valor guardado só vale para a mesma leitura: mesma chave (Hub, principal, corpo do pedido) e o mesmo client
 * (reconexão = outro client). Outra identidade: null, e a view mostra "lendo" já no render.
 */
export function daMesmaLeitura<T>(guardado: { chave: string; client: unknown; valor: T } | null, chave: string, client: unknown): T | null {
  return guardado && client !== null && guardado.chave === chave && guardado.client === client ? guardado.valor : null
}

/** Por que não há relatório. Código estável; a view traduz. */
export type Falha = 'antigo' | 'soDono' | 'invalida' | 'falha'
export function codigoFalha(e: unknown): Falha {
  const status = typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
  const message = typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
  if (status === 404 || /unknown method|Not available in the web demo/i.test(message)) return 'antigo'
  if (status === 403 || status === 401) return 'soDono'
  if (message.includes(INVALIDA)) return 'invalida'
  return 'falha'
}

/** "US$ 1,23 + R$ 0,40": cada moeda no formato do idioma, nunca convertidas nem somadas. Vazio: sem dinheiro. */
export function dinheiro(idioma: AppLanguage, costs: readonly Custo[]): string | null {
  if (costs.length === 0) return null
  return [...costs].sort((a, b) => a.currency.localeCompare(b.currency)).map((c) => formatarMoeda(idioma, c.amount, c.currency, { maximumFractionDigits: c.amount > 0 && c.amount < 0.01 ? 4 : 2 })).join(' + ')
}

/** A soma em dinheiro deixou turnos de fora (motor sem preço, ACP sem base). */
export const parcial = (t: Totais) => (t.costs.length > 0 || (t.estimatedCosts?.length ?? 0) > 0) && t.unpricedTurns > 0

/** Tokens curtos ("84 mil", "1,2 mi"). */
export const tokens = (idioma: AppLanguage, n: number) => formatarNumero(idioma, n, { notation: 'compact', maximumFractionDigits: 1 })

export type ParteTokens = 'entrada' | 'cacheLido' | 'cacheGravado' | 'saida'
/** As partes de tokens que o Hub tem (nunca um total inventado); `null` fica de fora. */
export function partes(t: Totais): { parte: ParteTokens; n: number }[] {
  const todas: [ParteTokens, number | null][] = [['entrada', t.inputTokens], ['cacheLido', t.cachedReadTokens], ['cacheGravado', t.cachedWriteTokens], ['saida', t.outputTokens]]
  return todas.flatMap(([parte, n]) => (n === null ? [] : [{ parte, n }]))
}

/**
 * Maior primeiro, sem converter moedas: com dinheiro antes de sem dinheiro; com dinheiro, pela moeda (código) e,
 * na mesma moeda, pelo valor; sem dinheiro, pelos tokens conhecidos. Empate pelo nome.
 */
export function ordenar<T extends { totals: Totais }>(linhas: readonly T[], nome: (l: T) => string): T[] {
  const moeda = (t: Totais) => (t.costs.length ? [...t.costs].sort((a, b) => a.currency.localeCompare(b.currency))[0]! : null)
  const soma = (t: Totais) => partes(t).reduce((s, p) => s + p.n, 0)
  return [...linhas].sort((a, b) => {
    const ma = moeda(a.totals), mb = moeda(b.totals)
    if (!!ma !== !!mb) return ma ? -1 : 1
    if (ma && mb) {
      if (ma.currency !== mb.currency) return ma.currency.localeCompare(mb.currency)
      if (ma.amount !== mb.amount) return mb.amount - ma.amount
    } else if (soma(a.totals) !== soma(b.totals)) return soma(b.totals) - soma(a.totals)
    return nome(a).localeCompare(nome(b))
  })
}

/** O título vazio vira texto traduzido pela view: principal (`sessionId` = `chatId`) ou sem título. */
export const tituloVazio = (c: LinhaConversa): 'principal' | 'semTitulo' => (c.sessionId === c.chatId ? 'principal' : 'semTitulo')

/** `YYYY-MM` em UTC (o mesmo mês padrão do Hub). */
export const mesDe = (ms: number) => new Date(ms).toISOString().slice(0, 7)
/** O mês vizinho: -1 anterior, +1 seguinte. */
export function mesVizinho(mes: string, passo: number): string {
  const [a, m] = mes.split('-').map(Number) as [number, number]
  return mesDe(Date.UTC(a, m - 1 + passo, 1))
}
/** "novembro de 2026" no idioma, sem deslocar pelo fuso local. */
export const nomeDoMes = (idioma: AppLanguage, mes: string) => formatarData(idioma, Date.parse(`${mes}-01T00:00:00Z`), { month: 'long', year: 'numeric', timeZone: 'UTC' })

// ---- MX-145 E2: o painel de uso (Ajustes → Uso, abas Por conta, Por modelo e Por bot) ----

/** As abas do painel: o id que `abasDe.usage` e `TelaAjustes` guardam. */
export const ABAS_USO = ['conta', 'modelo', 'bot'] as const
export type AbaUso = (typeof ABAS_USO)[number]
/** Aba desconhecida ou ausente cai na primeira. */
export const abaDoUso = (aba: string | null): AbaUso => (ABAS_USO as readonly string[]).includes(aba ?? '') ? (aba as AbaUso) : 'conta'

/** Só os campos do cadastro do Hub (`donnaEngineAccounts`) que o painel usa para dar nome e tipo à conta. */
export type ContaCadastro = Pick<EngineAccount, 'id' | 'label' | 'backend' | 'billing'>

/** Resposta do cadastro: qualquer item fora do formato recusa tudo (a view mostra só o ID curto, os números ficam). */
export function lerCadastro(raw: unknown): ContaCadastro[] {
  const o = obj(raw)
  return lista(o.accounts, (v) => {
    const c = obj(v)
    const backend = c.backend === 'claude' || c.backend === 'codex' ? c.backend : invalida()
    const billing = c.billing === 'subscription' || c.billing === 'paid_api' || c.billing === 'unverified' ? c.billing : invalida()
    return { id: id(c.id), label: texto(c.label), backend, billing }
  })
}

/** A identidade de uma leitura do cadastro: Hub e época da principal (reconexão, resync), como a do custo. */
export const chaveDoCadastro = (hubId: string | null, epoca: number) => JSON.stringify([hubId, epoca])

/** Os primeiros 8 caracteres do ID: o que a view mostra quando o cadastro não nomeia a conta. */
export const idCurto = (accountId: string) => (accountId.length > 8 ? accountId.slice(0, 8) : accountId)

/** O tipo mostrado: assinatura ou chave; `unverified` não afirma nada (`null`). */
export type TipoConta = 'assinatura' | 'chave'
export type RotuloConta =
  | { kind: 'naoIdentificada' }
  | { kind: 'nome'; label: string; tipo: TipoConta | null; backend: ContaCadastro['backend'] }
  | { kind: 'curto'; id: string }

/**
 * O rótulo de um grupo por conta. `cadastro` null: ainda lendo ou falhou (só o ID curto). Conta que o cadastro não
 * conhece, ou sem nome, também fica com o ID curto; nunca se adivinha pelo modelo ou pelo bot.
 */
export function rotuloDaConta(accountId: string | null, cadastro: readonly ContaCadastro[] | null): RotuloConta {
  if (accountId === null) return { kind: 'naoIdentificada' }
  const c = cadastro?.find((x) => x.id === accountId)
  if (!c || !c.label.trim()) return { kind: 'curto', id: idCurto(accountId) }
  return { kind: 'nome', label: c.label, tipo: c.billing === 'subscription' ? 'assinatura' : c.billing === 'paid_api' ? 'chave' : null, backend: c.backend }
}
