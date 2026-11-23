import { PROJECT_TASK_STATES, type ProjectTaskState } from './fork-projetos.ts'

const PERSPECTIVES = ['overview', 'director', 'bot'] as const
const ORIGINS = ['owner_direct', 'delegated', 'routine', 'unknown'] as const
const BUCKETS = ['needs_you', 'observed_working', 'waiting_or_blocked', 'backlog', 'done'] as const
const REQUESTERS = ['owner', 'bot', 'routine', 'unknown'] as const
const BLOCKERS = ['permission_pause', 'owner_decision', 'dependency', 'manual', 'none'] as const
const EXECUTION_STATUSES = ['idle', 'working', 'needsInput', 'waiting', 'error', 'unavailable'] as const

export type TowerPerspective = (typeof PERSPECTIVES)[number]
export type WorkOriginKind = (typeof ORIGINS)[number]
export type WorkBucket = (typeof BUCKETS)[number]

export interface WorkOrigin {
  kind: WorkOriginKind
  rootBotId: string | null
  delegatedByBotId: string | null
  routineId: string | null
  admittedAt: number | null
}

export interface WorkCard {
  taskId: string
  projectId: string
  projectName: string
  spaceId: string
  parentTaskId: string | null
  title: string
  state: ProjectTaskState
  bucket: WorkBucket
  origin: WorkOrigin
  requesterLabel: (typeof REQUESTERS)[number]
  requesterBotId: string | null
  approverRole: 'owner'
  directorBotId: string | null
  assigneeBotId: string | null
  nextAction: string | null
  blocker: { kind: (typeof BLOCKERS)[number]; reason: string | null; attentionItemId: string | null }
  delivery: { hasEvidence: boolean; evidenceSummary: string | null; acceptedByOwner: boolean }
  declaredUpdatedAt: number
  observedExecution: {
    active: boolean
    status: (typeof EXECUTION_STATUSES)[number]
    observedAt: number | null
    machineLabel: string | null
  }
  capabilities: { canDirectorInterrupt: boolean; canOwnerAccept: boolean }
}

export interface TowerViewSnapshot {
  version: 1
  hubId: string
  spaceId: string
  perspective: TowerPerspective
  directorBotId: string | null
  botId: string | null
  collectedAt: number
  cards: WorkCard[]
  counts: { needsYou: number; observedWorking: number; waitingOrBlocked: number; totalVisible: number }
}

type ExpectedTowerView = { hubId: string; spaceId: string } & (
  | { perspective: 'overview'; directorBotId?: null; botId?: null }
  | { perspective: 'director'; directorBotId: string; botId?: null }
  | { perspective: 'bot'; directorBotId?: null; botId: string }
)

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const nonempty = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0
const nullableId = (v: unknown): v is string | null => v === null || nonempty(v)
const nullableText = (v: unknown): v is string | null => v === null || typeof v === 'string'
const timestamp = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= 8_640_000_000_000_000
const oneOf = <T extends string>(v: unknown, values: readonly T[]): v is T => values.some((value) => value === v)

function bucketFor(state: ProjectTaskState, blocker: WorkCard['blocker']['kind']): WorkBucket | null {
  if (state === 'cancelled') return null
  if (state === 'done') return 'done'
  if (state === 'review' || blocker === 'owner_decision') return 'needs_you'
  if (state === 'blocked' || blocker !== 'none') return 'waiting_or_blocked'
  return state === 'in_progress' ? 'observed_working' : 'backlog'
}

export function parseWorkCard(v: unknown, spaceId?: string): WorkCard | null {
  if (!isObject(v) || !nonempty(v.taskId) || !nonempty(v.projectId) || !nonempty(v.projectName)
    || !nonempty(v.spaceId) || (spaceId !== undefined && v.spaceId !== spaceId) || !nullableId(v.parentTaskId) || !nonempty(v.title)
    || !oneOf(v.state, PROJECT_TASK_STATES) || !oneOf(v.bucket, BUCKETS)
    || !oneOf(v.requesterLabel, REQUESTERS) || !nullableId(v.requesterBotId) || v.approverRole !== 'owner'
    || !nullableId(v.directorBotId) || !nullableId(v.assigneeBotId) || !nullableText(v.nextAction)
    || (v.nextAction !== null && [...v.nextAction].length > 240) || !timestamp(v.declaredUpdatedAt)) return null

  const { origin, blocker, delivery, observedExecution, capabilities } = v
  if (!isObject(origin) || !oneOf(origin.kind, ORIGINS) || !nullableId(origin.rootBotId)
    || !nullableId(origin.delegatedByBotId) || !nullableId(origin.routineId)
    || (origin.admittedAt !== null && !timestamp(origin.admittedAt))
    || (origin.kind === 'owner_direct' && (origin.rootBotId !== null || origin.delegatedByBotId !== null))) return null
  if (!isObject(blocker) || !oneOf(blocker.kind, BLOCKERS) || !nullableText(blocker.reason)
    || !nullableId(blocker.attentionItemId) || v.bucket !== bucketFor(v.state, blocker.kind)) return null
  if (!isObject(delivery) || typeof delivery.hasEvidence !== 'boolean' || !nullableText(delivery.evidenceSummary)
    || typeof delivery.acceptedByOwner !== 'boolean') return null
  if (!isObject(observedExecution) || typeof observedExecution.active !== 'boolean'
    || !oneOf(observedExecution.status, EXECUTION_STATUSES)
    || (observedExecution.observedAt !== null && !timestamp(observedExecution.observedAt))
    || !nullableId(observedExecution.machineLabel)
    || (observedExecution.machineLabel !== null && !observedExecution.active)) return null
  if (!isObject(capabilities) || typeof capabilities.canDirectorInterrupt !== 'boolean'
    || typeof capabilities.canOwnerAccept !== 'boolean'
    || (capabilities.canOwnerAccept && v.state !== 'review')
    || (capabilities.canDirectorInterrupt && origin.kind !== 'delegated' && origin.kind !== 'routine')) return null

  return {
    taskId: v.taskId, projectId: v.projectId, projectName: v.projectName, spaceId: v.spaceId,
    parentTaskId: v.parentTaskId, title: v.title, state: v.state, bucket: v.bucket,
    origin: { kind: origin.kind, rootBotId: origin.rootBotId, delegatedByBotId: origin.delegatedByBotId,
      routineId: origin.routineId, admittedAt: origin.admittedAt },
    requesterLabel: v.requesterLabel, requesterBotId: v.requesterBotId, approverRole: 'owner',
    directorBotId: v.directorBotId, assigneeBotId: v.assigneeBotId, nextAction: v.nextAction,
    blocker: { kind: blocker.kind, reason: blocker.reason, attentionItemId: blocker.attentionItemId },
    delivery: { hasEvidence: delivery.hasEvidence, evidenceSummary: delivery.evidenceSummary, acceptedByOwner: delivery.acceptedByOwner },
    declaredUpdatedAt: v.declaredUpdatedAt,
    observedExecution: { active: observedExecution.active, status: observedExecution.status,
      observedAt: observedExecution.observedAt, machineLabel: observedExecution.machineLabel },
    capabilities: { canDirectorInterrupt: capabilities.canDirectorInterrupt, canOwnerAccept: capabilities.canOwnerAccept },
  }
}

export function parseTowerViewSnapshot(v: unknown, expected?: ExpectedTowerView): TowerViewSnapshot | null {
  if (!isObject(v) || v.version !== 1 || !nonempty(v.hubId) || !nonempty(v.spaceId)
    || !oneOf(v.perspective, PERSPECTIVES)
    || (expected && (v.hubId !== expected.hubId || v.spaceId !== expected.spaceId || v.perspective !== expected.perspective))
    || !timestamp(v.collectedAt) || !Array.isArray(v.cards) || !isObject(v.counts)) return null

  const { directorBotId, botId, perspective, spaceId } = v
  if (!nullableId(directorBotId) || (expected && directorBotId !== (expected.directorBotId ?? null))
    || (directorBotId !== null) !== (perspective === 'director')
    || !nullableId(botId) || (expected && botId !== (expected.botId ?? null))
    || (botId !== null) !== (perspective === 'bot')) return null

  const cards: WorkCard[] = []
  const seen = new Set<string>()
  const counts = { needsYou: 0, observedWorking: 0, waitingOrBlocked: 0, totalVisible: v.cards.length }
  for (const raw of v.cards) {
    const card = parseWorkCard(raw, spaceId)
    if (!card || (perspective === 'bot' && card.assigneeBotId !== botId)) return null
    if ((card.capabilities.canOwnerAccept && perspective !== 'overview')
      || (card.capabilities.canDirectorInterrupt && perspective !== 'director')) return null
    const key = JSON.stringify([card.projectId, card.taskId])
    if (seen.has(key)) return null
    seen.add(key)
    if (card.bucket === 'needs_you') counts.needsYou++
    if (card.bucket === 'observed_working') counts.observedWorking++
    if (card.bucket === 'waiting_or_blocked') counts.waitingOrBlocked++
    cards.push(card)
  }
  if (v.counts.needsYou !== counts.needsYou || v.counts.observedWorking !== counts.observedWorking
    || v.counts.waitingOrBlocked !== counts.waitingOrBlocked || v.counts.totalVisible !== counts.totalVisible) return null

  return { version: 1, hubId: v.hubId, spaceId, perspective, directorBotId, botId, collectedAt: v.collectedAt, cards, counts }
}
