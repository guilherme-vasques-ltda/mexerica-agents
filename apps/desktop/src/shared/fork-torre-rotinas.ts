// D544: projeção efêmera do Board, já filtrada pelo Space no Hub. Não entra no mirror.
export interface RoutineCreationReview {
  entryId: string
  entryRev: number
  routineId: string
  botId: string
  botName: string
  name: string
  createdAt: number
  when: string
  nextRunAt: number | null
  status: 'active' | 'paused'
  externalReleasePending: boolean
  routineRevision: string | null
}

export interface RoutineQuotaBlock {
  botId: string
  botName: string
  limit: number
  used: number
  retryAt: number
  reason: string
  howToIncrease: string
  settingsAvailable: boolean
}

export interface RoutineCreations { items: RoutineCreationReview[]; blocked: RoutineQuotaBlock[] }
export type RoutineReviewChoice = 'keep' | 'cancel' | 'enable'
export interface RoutineCreationReviewRequest {
  hubId: string
  spaceId: string
  entryId: string
  entryRev: number
  choice: RoutineReviewChoice
  routineRevision?: string
}
export interface RoutineCreationReviewResult {
  entryId: string
  routineId: string
  decision: 'kept' | 'cancelled' | 'enabled'
  rev: number
}
export type ReviewRoutine = (body: RoutineCreationReviewRequest) => Promise<unknown>

const obj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const text = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0
const count = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const timestamp = (v: unknown): v is number => count(v) && v <= 8_640_000_000_000_000

function review(v: unknown): RoutineCreationReview | null {
  if (!obj(v) || !text(v.entryId) || !count(v.entryRev) || !text(v.routineId)
    || !text(v.botId) || !text(v.botName) || !text(v.name) || !text(v.when)
    || !timestamp(v.createdAt) || (v.nextRunAt !== null && !timestamp(v.nextRunAt))
    || (v.status !== 'active' && v.status !== 'paused')
    || (v.externalReleasePending !== undefined && typeof v.externalReleasePending !== 'boolean')) return null
  return { entryId: v.entryId, entryRev: v.entryRev, routineId: v.routineId, botId: v.botId,
    botName: v.botName, name: v.name, createdAt: v.createdAt, when: v.when, nextRunAt: v.nextRunAt, status: v.status,
    externalReleasePending: v.externalReleasePending === true && v.status === 'paused' && typeof v.routineRevision === 'string' && /^[a-f0-9]{64}$/.test(v.routineRevision),
    routineRevision: typeof v.routineRevision === 'string' && /^[a-f0-9]{64}$/.test(v.routineRevision) ? v.routineRevision : null }
}

function block(v: unknown): RoutineQuotaBlock | null {
  if (!obj(v) || !text(v.botId) || !text(v.botName) || !count(v.limit) || !count(v.used)
    || !timestamp(v.retryAt) || !text(v.reason) || !text(v.howToIncrease)
    || typeof v.settingsAvailable !== 'boolean') return null
  return { botId: v.botId, botName: v.botName, limit: v.limit, used: v.used, retryAt: v.retryAt,
    reason: v.reason, howToIncrease: v.howToIncrease, settingsAvailable: v.settingsAvailable }
}

/** Falha fechada: lista parcial/malformada não pode fingir que nada precisa de revisão. */
export function parseRoutineCreations(v: unknown): RoutineCreations | null {
  if (!obj(v) || !Array.isArray(v.items) || !Array.isArray(v.blocked)) return null
  const items: RoutineCreationReview[] = []
  const blocked: RoutineQuotaBlock[] = []
  for (const raw of v.items) {
    const item = review(raw)
    if (!item || items.some((other) => other.entryId === item.entryId)) return null
    items.push(item)
  }
  for (const raw of v.blocked) {
    const item = block(raw)
    if (!item || blocked.some((other) => other.botId === item.botId)) return null
    blocked.push(item)
  }
  return { items, blocked }
}

/** HTTP/RPC resolvida não basta: confirmar o alvo, a escolha e a revisão registrada. */
export function parseRoutineReviewResult(answer: unknown, item: Pick<RoutineCreationReview, 'entryId' | 'routineId' | 'entryRev'>, choice: RoutineReviewChoice): RoutineCreationReviewResult {
  const decision = choice === 'keep' ? 'kept' : choice === 'enable' ? 'enabled' : 'cancelled'
  if (!obj(answer) || answer.entryId !== item.entryId || answer.routineId !== item.routineId
    || answer.decision !== decision || !count(answer.rev) || answer.rev <= item.entryRev) {
    throw new Error('routine_review_invalid_answer')
  }
  return { entryId: item.entryId, routineId: item.routineId, decision, rev: answer.rev }
}
