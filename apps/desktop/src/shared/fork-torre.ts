import { validMexeriquinha, type MexeriquinhaConfig } from './fork-mexeriquinha-config.ts'
// VG-02 (docs/features/mission-board.md): o que o app lê da projeção `donnaControlTower` do Hub e como a tela
// a apresenta. Só leitura e rótulos: nenhuma regra de audiência, contagem privada, fila ou uso é calculada aqui.
// Indisponível nunca vira zero, Livre ou lista vazia. O snapshot é efêmero: não entra no mirror nem em disco.
import { parseActivityCode, type ActivityCode } from './fork-activity-code.ts'
import { parseInterBotRequests, type InterBotRequests } from './fork-torre-pedidos.ts'
import { parseWhatHappened, type WhatHappened } from './fork-torre-resumo.ts'
import { parseRoutineCreations, type RoutineCreations } from './fork-torre-rotinas.ts'
import { parseTrocasDeConta, type TrocasDeConta } from './fork-torre-trocas.ts'
import { parseDelegatedPermissions, type DelegatedPermissions } from './fork-torre-permissoes.ts'
import { parseAttentionObservation, type AttentionQueue } from './fork-torre-esperando.ts'
export type { InterBotRequests, WhatHappened, RoutineCreations, DelegatedPermissions }

export type TowerStatus = 'idle' | 'working' | 'needsInput' | 'waiting' | 'error'

/** Uma medida do Hub: com fonte e cobertura, ou indisponível (que não revela se existe algo por trás). */
export type Observation<T> =
  | { availability: 'available'; source: string; coverage: string; value: T }
  | { availability: 'unavailable' }

export interface ConversationRef { conversationId: string; revision: number }
export interface SessionRef { id: string; chatId: string }

export interface TowerWork {
  status: TowerStatus
  activity: string | null
  /** MX-147: código estável; `null` quando ausente, desconhecido ou malformado (vale `activity`). */
  activityCode: ActivityCode | null
  startedAt: number | null
  resumeAt: number | null
  reason: string | null
  conversation: ConversationRef | null
}

export interface TowerOpen { label: string; botId: string; conversation: ConversationRef; sessionRef: SessionRef | null }

export interface TowerPending { entryId: string; sessionRef: SessionRef; kind: string; status: string; conversation: ConversationRef }

/** MB-1b: pergunta `ask_owner` pendente, já filtrada pela audiência do Space no Hub. */
export interface TowerQuestion { entryId: string; conversation: ConversationRef; sessionId: string | null; text: string; askedAt: number }

export interface TowerBot {
  botId: string
  name: string
  title: string | null
  avatarColor: string
  avatarShape: string
  avatarConfig: unknown
  mexeriquinha?: MexeriquinhaConfig
  runtime: Observation<TowerWork>
  open: TowerOpen | null
  pending: Observation<TowerPending[]>
  /** MB-1b; Hub anterior sem o campo: indisponível. */
  questions: Observation<TowerQuestion[]>
  queue: Observation<number>
  usage: { dailyBotTokens: Observation<number>; subscriptionLimit: Observation<number>; contextTokens: Observation<number> }
}

export interface TowerSnapshot { version: 1; hubId: string; snapshotId: string; collectedAt: number; spaceId: string; bots: TowerBot[]; interBotRequests: Observation<InterBotRequests>; whatHappened: Observation<WhatHappened>; routineCreations: Observation<RoutineCreations>; attention: Observation<AttentionQueue>
  /** MEX-1391 (o).2: um cartão por troca de conta do fallback; Hub anterior sem o campo: indisponível. */
  accountSwitches: Observation<TrocasDeConta>
  /** MX-478: permissões pedidas por turno delegado, só Abrir; Hub anterior sem o campo: indisponível. */
  delegatedPermissions: Observation<DelegatedPermissions> }

const STATUSES = new Set<TowerStatus>(['idle', 'working', 'needsInput', 'waiting', 'error'])
const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' ? v : null)
const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : null)

function bad(what: string): never {
  throw new Error(`torre_invalid_answer: ${what}`)
}

function conversation(v: unknown): ConversationRef | null {
  if (!isObj(v)) return null
  const id = str(v.conversationId)
  const revision = num(v.revision)
  return id && revision !== null ? { conversationId: id, revision } : null
}

function sessionRef(v: unknown): SessionRef | null {
  if (!isObj(v)) return null
  const id = str(v.id)
  const chatId = str(v.chatId)
  return id && chatId ? { id, chatId } : null
}

/** Uma observação que o app não entende é tratada como indisponível, nunca como um valor. */
function observation<T>(v: unknown, value: (raw: unknown) => T | null): Observation<T> {
  if (!isObj(v) || v.availability !== 'available') return { availability: 'unavailable' }
  const source = str(v.source)
  const coverage = str(v.coverage)
  const parsed = value(v.value)
  return source && coverage && parsed !== null ? { availability: 'available', source, coverage, value: parsed } : { availability: 'unavailable' }
}

function work(v: unknown): TowerWork | null {
  if (!isObj(v) || !STATUSES.has(v.status as TowerStatus)) return null
  return {
    status: v.status as TowerStatus,
    activity: str(v.activity),
    activityCode: parseActivityCode(v.activityCode),
    startedAt: num(v.startedAt),
    resumeAt: num(v.resumeAt),
    reason: str(v.reason),
    conversation: v.conversation == null ? null : conversation(v.conversation),
  }
}

function pendingList(v: unknown): TowerPending[] | null {
  if (!Array.isArray(v)) return null
  const out: TowerPending[] = []
  for (const p of v) {
    if (!isObj(p)) return null
    const entryId = str(p.entryId)
    const ref = sessionRef(p.sessionRef)
    const conv = conversation(p.conversation)
    if (!entryId || !ref || !conv) return null
    out.push({ entryId, sessionRef: ref, kind: str(p.kind) ?? '', status: str(p.status) ?? '', conversation: conv })
  }
  return out
}

function questionList(v: unknown): TowerQuestion[] | null {
  if (!Array.isArray(v)) return null
  const out: TowerQuestion[] = []
  for (const q of v) {
    if (!isObj(q)) return null
    const entryId = str(q.entryId)
    const conv = conversation(q.conversation)
    const text = str(q.text)
    const askedAt = num(q.askedAt)
    if (!entryId || !conv || !text || askedAt === null) return null
    out.push({ entryId, conversation: conv, sessionId: str(q.sessionId), text, askedAt })
  }
  return out
}

function open(v: unknown, botId: string): TowerOpen | null {
  if (!isObj(v)) return null
  const conv = conversation(v.conversation)
  // O destino é sempre do próprio bot do cartão; outro bot não é navegação deste cartão.
  if (!conv || v.botId !== botId) return null
  // O Hub manda um rótulo fixo em pt-BR; a tela usa o do catálogo (DON-917), e o campo fica só como dado.
  return { label: str(v.label) ?? '', botId, conversation: conv, sessionRef: sessionRef(v.sessionRef) }
}

/**
 * Valida a resposta do Hub. `hubId`/`spaceId` diferentes do pedido são recusados: a tela nunca pinta uma fotografia
 * de outro Hub ou space.
 */
export function parseTowerSnapshot(answer: unknown, expected: { hubId: string; spaceId: string }): TowerSnapshot {
  if (!isObj(answer) || answer.version !== 1) bad('version')
  if (answer.hubId !== expected.hubId) bad('hub')
  if (answer.spaceId !== expected.spaceId) bad('space')
  const collectedAt = num(answer.collectedAt)
  const snapshotId = str(answer.snapshotId)
  if (collectedAt === null || !snapshotId || !Array.isArray(answer.bots)) bad('fields')
  const bots: TowerBot[] = answer.bots.map((b) => {
    if (!isObj(b)) bad('bot')
    const botId = str(b.botId)
    const name = str(b.name)
    if (!botId || !name) bad('bot')
    const usage = isObj(b.usage) ? b.usage : {}
    return {
      botId,
      name,
      title: str(b.title),
      avatarColor: str(b.avatarColor) ?? '',
      avatarShape: str(b.avatarShape) ?? '',
      avatarConfig: b.avatarConfig ?? null,
      ...(validMexeriquinha(b.mexeriquinha) ? { mexeriquinha: b.mexeriquinha } : {}),
      runtime: observation(b.runtime, work),
      open: b.open == null ? null : open(b.open, botId),
      pending: observation(b.pending, pendingList),
      questions: observation(b.questions, questionList),
      queue: observation(b.queue, num),
      usage: {
        dailyBotTokens: observation(usage.dailyBotTokens, num),
        subscriptionLimit: observation(usage.subscriptionLimit, num),
        contextTokens: observation(usage.contextTokens, num),
      },
    }
  })
  return { version: 1, hubId: answer.hubId as string, snapshotId, collectedAt, spaceId: answer.spaceId as string, bots, interBotRequests: observation(answer.interBotRequests, parseInterBotRequests), whatHappened: observation(answer.whatHappened, parseWhatHappened), routineCreations: observation(answer.routineCreations, parseRoutineCreations), accountSwitches: observation(answer.accountSwitches, parseTrocasDeConta), delegatedPermissions: observation(answer.delegatedPermissions, parseDelegatedPermissions), attention: parseAttentionObservation(answer.attention, expected) }
}

// MARK: apresentação

/** `unknown`: sem observação do Hub (“Não informado”), nem Livre nem zero. O rótulo é da tela (`torre.estado.*`, DON-917). */
export type TowerTone = TowerStatus | 'unknown'

/** O estado de um cartão: o do Hub, ou `unknown` quando o Hub não o pode mostrar. */
export function statusOf(bot: TowerBot): { tone: TowerTone } {
  if (bot.runtime.availability !== 'available') return { tone: 'unknown' }
  return { tone: bot.runtime.value.status }
}

/** As pendências originais (cartões de permissão) que o Hub autorizou; null se indisponível. */
export function pendingOf(bot: TowerBot): TowerPending[] | null {
  return bot.pending.availability === 'available' ? bot.pending.value : null
}

/** MB-1b: as perguntas do bot ao dono que o Hub autorizou; null se indisponível. */
export function questionsOf(bot: TowerBot): TowerQuestion[] | null {
  return bot.questions.availability === 'available' ? bot.questions.value : null
}

/** D341 (DON-896): o Board só lista o que precisa do dono ou está acontecendo; Livre e sem estado ficam de fora. */
export type TowerGroup = 'waitingYou' | 'working' | 'waitingOrError'

/**
 * Em que seção o cartão aparece, ou null se fica fora do Board (Livre ou sem estado informado). Uma permissão ou
 * pergunta pendente coloca o bot em “Esperam você”, qualquer que seja o estado.
 */
export function groupOf(bot: TowerBot): TowerGroup | null {
  if ((pendingOf(bot)?.length ?? 0) > 0) return 'waitingYou'
  if ((questionsOf(bot)?.length ?? 0) > 0) return 'waitingYou'
  const { tone } = statusOf(bot)
  if (tone === 'needsInput') return 'waitingYou'
  if (tone === 'working') return 'working'
  if (tone === 'waiting' || tone === 'error') return 'waitingOrError'
  return null
}

const GROUP_ORDER: TowerGroup[] = ['waitingYou', 'working', 'waitingOrError']

/** Os cartões listados no Board, na ordem do Hub. */
export function listedBots(bots: readonly TowerBot[]): TowerBot[] {
  return bots.filter((b) => groupOf(b) !== null)
}

/** Quantos bots ficaram fora só por falta de estado informado (para o aviso honesto do topo). */
export function uninformedCount(bots: readonly TowerBot[]): number {
  return bots.filter((b) => groupOf(b) === null && statusOf(b).tone === 'unknown').length
}

/** Os cartões listados, por seção, na ordem do Hub dentro de cada seção. Seções vazias não aparecem. */
export function groupBots(bots: readonly TowerBot[]): { group: TowerGroup; bots: TowerBot[] }[] {
  return GROUP_ORDER.map((group) => ({ group, bots: bots.filter((b) => groupOf(b) === group) })).filter((g) => g.bots.length)
}

/** Quantos cartões desta fotografia estão em cada estado (os que o Hub enviou; nada é somado de outra fonte). */
export function countByTone(bots: readonly TowerBot[]): Record<TowerTone, number> {
  const out: Record<TowerTone, number> = { working: 0, needsInput: 0, waiting: 0, error: 0, idle: 0, unknown: 0 }
  for (const b of bots) out[statusOf(b).tone]++
  return out
}

/** A fila de um bot: só com medição; sem ela, null (“Fila não disponível”), nunca 0. */
export function queueOf(bot: TowerBot): number | null {
  return bot.queue.availability === 'available' ? bot.queue.value : null
}

/** Uma medida de uso: o número informado pelo Hub, ou null (“Não informado”). A tela formata no idioma. */
export function usageOf(o: Observation<number>): number | null {
  return o.availability === 'available' ? o.value : null
}

/**
 * Para onde Abrir leva, sem inventar destino: no Pessoal, a conversa própria do bot (ID igual ao do bot); num space,
 * exatamente a conversa que o Hub indicou. Outra conversa no Pessoal não tem caminho hoje: fica indisponível, sem cair
 * na principal.
 */
export type OpenTarget =
  | { kind: 'personal'; botId: string }
  | { kind: 'space'; spaceId: string; conversationId: string }

export function openTarget(conversationId: string, botId: string, spaceId: string, personalSpaceId: string | null): OpenTarget | null {
  if (spaceId === personalSpaceId) return conversationId === botId ? { kind: 'personal', botId } : null
  return { kind: 'space', spaceId, conversationId }
}

/** Abrir para parar só aparece com trabalho em andamento informado e destino do Hub. */
export function canOpenToStop(bot: TowerBot): boolean {
  return bot.open !== null && bot.runtime.availability === 'available' && bot.runtime.value.status === 'working'
}
