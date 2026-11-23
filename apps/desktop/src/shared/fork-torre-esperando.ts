import type { ConversationRef, Observation } from './fork-torre.ts'

const actionByKind = {
  permission: 'respond_permission',
  delegated_permission: 'respond_permission',
  owner_question: 'answer_question',
  task_proposal: 'decide_proposal',
  task_review: 'review_task',
  routine_review: 'review_routine',
  doctor_alert: 'open_doctor',
} as const

export type AttentionKind = keyof typeof actionByKind
export type AttentionSeverity = 'blocking' | 'action_required' | 'warning'
export type AttentionActionKind = typeof actionByKind[AttentionKind] | 'open_conversation'
export type AttentionResolutionKind = 'approved' | 'rejected' | 'answered' | 'accepted' | 'reopened' | 'kept' | 'cancelled' | 'recovered'

export interface AttentionTarget {
  botId?: string
  requesterBotId?: string
  projectId?: string
  taskId?: string
  proposalId?: string
  routineId?: string
  entryId?: string
  conversation?: ConversationRef
  sessionId?: string | null
  doctorCheckId?: string
}

interface AttentionSubject {
  id: string
  kind: AttentionKind
  titleKey: string
  summary: string | null
  target: AttentionTarget
}

export interface AttentionItem extends AttentionSubject {
  dedupeKey: string
  severity: AttentionSeverity
  spaceId: string
  actionKind: AttentionActionKind
  approverRole: 'owner'
  createdAt: number
}

export interface ResolvedAttentionItem extends AttentionSubject {
  resolvedAt: number
  resolutionKind: AttentionResolutionKind
}

export interface AttentionQueue {
  version: 1
  hubId: string
  spaceId: string
  collectedAt: number
  items: AttentionItem[]
  resolvedToday: ResolvedAttentionItem[]
  coverageGaps: { source: string; code: 'source_unavailable' | 'hub_outdated' }[]
}

const obj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0
const num = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const severity = { blocking: 0, action_required: 1, warning: 2 } as const
const resolutions = new Set<AttentionResolutionKind>(['approved', 'rejected', 'answered', 'accepted', 'reopened', 'kept', 'cancelled', 'recovered'])

function parseSubject(raw: unknown): AttentionSubject | null {
  if (!obj(raw) || !str(raw.id) || !str(raw.kind) || !Object.hasOwn(actionByKind, raw.kind)
    || !str(raw.titleKey) || !(raw.summary === null || typeof raw.summary === 'string') || !obj(raw.target)) return null
  const kind = raw.kind as AttentionKind
  const target: AttentionTarget = {}
  for (const key of ['botId', 'requesterBotId', 'projectId', 'taskId', 'proposalId', 'routineId', 'entryId', 'doctorCheckId'] as const) {
    const value = raw.target[key]
    if (value === undefined) continue
    if (!str(value)) return null
    target[key] = value
  }
  if (raw.target.sessionId !== undefined) {
    if (raw.target.sessionId !== null && !str(raw.target.sessionId)) return null
    target.sessionId = raw.target.sessionId
  }
  const conversation = raw.target.conversation
  if (conversation !== undefined) {
    if (!obj(conversation) || !str(conversation.conversationId) || !num(conversation.revision)) return null
    target.conversation = { conversationId: conversation.conversationId, revision: conversation.revision }
  }
  if ((kind === 'permission' || kind === 'delegated_permission' || kind === 'owner_question')
    && (!target.entryId || !target.botId || !target.conversation)) return null
  if (kind === 'delegated_permission' && (!target.requesterBotId || target.requesterBotId === target.botId)) return null
  if (kind === 'task_proposal' && (!target.proposalId || !target.projectId)) return null
  if (kind === 'task_review' && (!target.projectId || !target.taskId)) return null
  if (kind === 'routine_review' && (!target.routineId || !target.entryId || !target.botId)) return null
  if (kind === 'doctor_alert' && !target.doctorCheckId) return null
  return { id: raw.id, kind, titleKey: raw.titleKey, summary: raw.summary, target }
}

export function parseAttentionQueue(raw: unknown, expected: { hubId: string; spaceId: string }): AttentionQueue | null {
  if (!obj(raw) || raw.version !== 1 || raw.hubId !== expected.hubId || raw.spaceId !== expected.spaceId || !num(raw.collectedAt)
    || !Array.isArray(raw.items) || !Array.isArray(raw.resolvedToday) || !Array.isArray(raw.coverageGaps)) return null
  const byKey = new Map<string, AttentionItem>()
  for (const value of raw.items) {
    const subject = parseSubject(value)
    if (!subject || !obj(value) || !str(value.dedupeKey) || value.spaceId !== expected.spaceId || value.approverRole !== 'owner'
      || !str(value.severity) || !Object.hasOwn(severity, value.severity) || !num(value.createdAt)) return null
    const action = actionByKind[subject.kind]
    if (value.actionKind !== action && !(subject.target.conversation && value.actionKind === 'open_conversation')) return null
    const permission = subject.kind === 'permission' || subject.kind === 'delegated_permission'
    if (permission && value.dedupeKey !== `permission:${subject.target.entryId}`) return null
    const item: AttentionItem = { ...subject, dedupeKey: value.dedupeKey, severity: value.severity as AttentionSeverity,
      spaceId: expected.spaceId, actionKind: value.actionKind === 'open_conversation' ? 'open_conversation' : action,
      approverRole: 'owner', createdAt: value.createdAt }
    const prior = byKey.get(item.dedupeKey)
    if (prior && (!permission || prior.target.entryId !== item.target.entryId || prior.target.botId !== item.target.botId
      || prior.target.conversation?.conversationId !== item.target.conversation?.conversationId
      || (prior.kind !== 'permission' && prior.kind !== 'delegated_permission')
      || (prior.kind === 'delegated_permission' && item.kind === 'delegated_permission'
        && prior.target.requesterBotId !== item.target.requesterBotId))) return null
    if (!prior || (item.kind === 'delegated_permission' && prior.kind === 'permission')) byKey.set(item.dedupeKey, item)
  }
  const items = [...byKey.values()].sort((a, b) => severity[a.severity] - severity[b.severity] || b.createdAt - a.createdAt)
  const seen = new Set(items.map((item) => item.id))
  if (seen.size !== items.length) return null
  const resolvedToday: ResolvedAttentionItem[] = []
  for (const value of raw.resolvedToday) {
    const subject = parseSubject(value)
    if (!subject || !obj(value) || !num(value.resolvedAt) || !resolutions.has(value.resolutionKind as AttentionResolutionKind)
      || seen.has(subject.id)) return null
    seen.add(subject.id)
    resolvedToday.push({ ...subject, resolvedAt: value.resolvedAt, resolutionKind: value.resolutionKind as AttentionResolutionKind })
  }
  const coverageGaps: AttentionQueue['coverageGaps'] = []
  for (const gap of raw.coverageGaps) {
    if (!obj(gap) || !str(gap.source) || (gap.code !== 'source_unavailable' && gap.code !== 'hub_outdated')) return null
    coverageGaps.push({ source: gap.source, code: gap.code })
  }
  return { version: 1, ...expected, collectedAt: raw.collectedAt, items, resolvedToday, coverageGaps }
}

export function parseAttentionObservation(raw: unknown, expected: { hubId: string; spaceId: string }): Observation<AttentionQueue> {
  if (!obj(raw) || raw.availability !== 'available' || !str(raw.source) || !str(raw.coverage)) return { availability: 'unavailable' }
  const value = parseAttentionQueue(raw.value, expected)
  return value ? { availability: 'available', source: raw.source, coverage: raw.coverage, value } : { availability: 'unavailable' }
}
