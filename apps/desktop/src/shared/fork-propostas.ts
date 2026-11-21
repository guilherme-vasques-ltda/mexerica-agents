import {
  PROJECT_TASK_STATES, ProjectsAnswerError, isCanonicalUuid, newUuid,
  type ProjectTaskState,
} from './fork-projetos.ts'
import type { SpacesSnapshot } from './fork-spaces.ts'

export { newUuid }

export const PROPOSAL_STATES = ['pending', 'approved', 'rejected'] as const
export type ProposalState = (typeof PROPOSAL_STATES)[number]

export const PROPOSAL_DECISIONS = ['approve', 'reject'] as const
export type ProposalDecision = (typeof PROPOSAL_DECISIONS)[number]

export const PROPOSAL_START_STATES = ['scheduled', 'starting', 'started', 'failed', 'refused', 'uncertain'] as const
export type ProposalStartState = (typeof PROPOSAL_START_STATES)[number]

export const PROPOSALS_EVENT = 'donnaProjectsChanged'

export interface ProposalLimits {
  perBot: number
  total: number
}

export interface ProposalItem {
  id: string
  title: string
  description: string
  acceptance: string
  assigneeBotId: string | null
  included: boolean
  taskId: string | null
  taskState: ProjectTaskState | null
  startState: ProposalStartState | null
}

export interface TaskProposal {
  id: string
  version: number
  state: ProposalState
  botId: string
  projectId: string
  projectName: string
  title: string
  createdAt: number
  decidedAt: number | null
  items: ProposalItem[]
}

export interface TaskProposalsSnapshot {
  version: 1
  hubId: string
  spaceId: string
  limits: ProposalLimits
  proposals: TaskProposal[]
}

export interface ProposalDecideBody {
  hubId: string
  spaceId: string
  proposalId: string
  expectedVersion: number
  clientRequestId: string
  decision: ProposalDecision
  omitItemIds?: string[]
}

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
function need<T>(ok: boolean, value: T, what: string): T {
  if (!ok) throw new ProjectsAnswerError(what)
  return value
}
const str = (o: Obj, k: string) => need(typeof o[k] === 'string', o[k] as string, k)
const nonEmptyStr = (o: Obj, k: string) => need(typeof o[k] === 'string' && (o[k] as string).length > 0, o[k] as string, k)
const optStr = (o: Obj, k: string) => need(o[k] === null || o[k] === undefined || typeof o[k] === 'string', (o[k] ?? null) as string | null, k)
const bool = (o: Obj, k: string) => need(typeof o[k] === 'boolean', o[k] as boolean, k)
const num = (o: Obj, k: string) => need(typeof o[k] === 'number' && Number.isFinite(o[k]), o[k] as number, k)
const int = (o: Obj, k: string, min = 0) => need(typeof o[k] === 'number' && Number.isInteger(o[k]) && (o[k] as number) >= min, o[k] as number, k)
const optNum = (o: Obj, k: string) => need(o[k] === null || o[k] === undefined || (typeof o[k] === 'number' && Number.isFinite(o[k])), (o[k] ?? null) as number | null, k)
const arr = (o: Obj, k: string) => need(Array.isArray(o[k]), o[k] as unknown[], k)

/** Valida um item de proposta segundo o contrato fechado do Hub. */
export function parseProposalItem(v: unknown): ProposalItem {
  const o = need(isObj(v), v as Obj, 'proposal.item')
  const rawTaskState = o.taskState ?? null
  const rawStartState = o.startState ?? null
  need(
    rawTaskState === null || (typeof rawTaskState === 'string' && (PROJECT_TASK_STATES as readonly string[]).includes(rawTaskState)),
    null,
    'proposal.item.taskState',
  )
  need(
    rawStartState === null || (typeof rawStartState === 'string' && (PROPOSAL_START_STATES as readonly string[]).includes(rawStartState)),
    null,
    'proposal.item.startState',
  )
  return {
    id: nonEmptyStr(o, 'id'),
    title: str(o, 'title'),
    description: str(o, 'description'),
    acceptance: str(o, 'acceptance'),
    assigneeBotId: optStr(o, 'assigneeBotId'),
    included: bool(o, 'included'),
    taskId: optStr(o, 'taskId'),
    taskState: rawTaskState as ProjectTaskState | null,
    startState: rawStartState as ProposalStartState | null,
  }
}

/** Valida uma proposta individual do contrato do Hub. */
export function parseProposal(v: unknown): TaskProposal {
  const o = need(isObj(v), v as Obj, 'proposal')
  const state = str(o, 'state')
  need((PROPOSAL_STATES as readonly string[]).includes(state), null, 'proposal.state')
  return {
    id: nonEmptyStr(o, 'id'),
    version: int(o, 'version', 0),
    state: state as ProposalState,
    botId: nonEmptyStr(o, 'botId'),
    projectId: nonEmptyStr(o, 'projectId'),
    projectName: str(o, 'projectName'),
    title: str(o, 'title'),
    createdAt: num(o, 'createdAt'),
    decidedAt: optNum(o, 'decidedAt'),
    items: arr(o, 'items').map(parseProposalItem),
  }
}

/** Valida a resposta de `donnaTaskProposals` contra a origem esperada (Hub e Space). */
export function parseTaskProposals(v: unknown, origin: { hubId: string; spaceId: string }): TaskProposalsSnapshot {
  const o = need(isObj(v), v as Obj, 'answer')
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
  const rawLimits = need(isObj(o.limits), o.limits as Obj, 'limits')
  const limits: ProposalLimits = {
    perBot: int(rawLimits, 'perBot', 0),
    total: int(rawLimits, 'total', 0),
  }
  const proposals = arr(o, 'proposals').map(parseProposal)
  return { version: 1, hubId: origin.hubId, spaceId: origin.spaceId, limits, proposals }
}

/** Valida a resposta de `donnaTaskProposalDecide`. */
export function parseProposalDecideAnswer(
  v: unknown,
  origin: { hubId: string; spaceId: string },
  expectedProposalId?: string,
): { proposal: TaskProposal } {
  const o = need(isObj(v), v as Obj, 'answer')
  if (o.hubId !== undefined) need(o.hubId === origin.hubId, null, 'hubId')
  if (o.spaceId !== undefined) need(o.spaceId === origin.spaceId, null, 'spaceId')
  const proposal = parseProposal(o.proposal)
  if (expectedProposalId !== undefined) need(proposal.id === expectedProposalId, null, 'proposal.id')
  return { proposal }
}

/** Monta o corpo exato de `donnaTaskProposalDecide`. */
export function decideBody(
  origin: { hubId: string; spaceId: string },
  proposal: Pick<TaskProposal, 'id' | 'version'>,
  decision: ProposalDecision,
  clientRequestId: string,
  omitItemIds?: readonly string[],
): ProposalDecideBody {
  need(isCanonicalUuid(clientRequestId), null, 'clientRequestId')
  const omitted = decision === 'approve' && omitItemIds?.length ? [...new Set(omitItemIds)] : undefined
  return {
    hubId: origin.hubId,
    spaceId: origin.spaceId,
    proposalId: proposal.id,
    expectedVersion: proposal.version,
    clientRequestId,
    decision,
    ...(omitted?.length ? { omitItemIds: omitted } : {}),
  }
}

export type ProposalBadgeTone = 'neutral' | 'accent' | 'attention' | 'success' | 'danger' | 'muted'

export type ProposalTaskStateKey =
  | 'propostas.estadoTarefa.backlog'
  | 'propostas.estadoTarefa.in_progress'
  | 'propostas.estadoTarefa.review'
  | 'propostas.estadoTarefa.done'
  | 'propostas.estadoTarefa.blocked'
  | 'propostas.estadoTarefa.cancelled'

export type ProposalStartStateKey =
  | 'propostas.inicio.scheduled'
  | 'propostas.inicio.aguardandoVaga'
  | 'propostas.inicio.starting'
  | 'propostas.inicio.started'
  | 'propostas.inicio.failed'
  | 'propostas.inicio.refused'
  | 'propostas.inicio.uncertain'

const TASK_STATE_BADGES: Record<ProjectTaskState, { key: ProposalTaskStateKey; tone: ProposalBadgeTone; awaitingOwner: boolean }> = {
  backlog: { key: 'propostas.estadoTarefa.backlog', tone: 'neutral', awaitingOwner: false },
  in_progress: { key: 'propostas.estadoTarefa.in_progress', tone: 'accent', awaitingOwner: false },
  review: { key: 'propostas.estadoTarefa.review', tone: 'attention', awaitingOwner: true },
  done: { key: 'propostas.estadoTarefa.done', tone: 'success', awaitingOwner: false },
  blocked: { key: 'propostas.estadoTarefa.blocked', tone: 'danger', awaitingOwner: false },
  cancelled: { key: 'propostas.estadoTarefa.cancelled', tone: 'muted', awaitingOwner: false },
}

/** Rótulo e tom de exibição para o estado da tarefa (`review` é sempre aguardando o dono, nunca aprovado). */
export function taskStateBadge(state: ProjectTaskState): { key: ProposalTaskStateKey; tone: ProposalBadgeTone; awaitingOwner: boolean } {
  return TASK_STATE_BADGES[state]
}

/** Tarefa aprovada que ocupa vaga de execução no Space/bot. */
export function isActiveProposalTask(item: Pick<ProposalItem, 'included' | 'taskId' | 'taskState' | 'startState'>): boolean {
  if (!item.included) return false
  if (item.taskId === null && item.startState === null) return false
  if (item.taskState === 'done' || item.taskState === 'cancelled' || item.taskState === 'blocked' || item.taskState === 'review') return false
  return item.taskState === 'in_progress' || item.startState === 'starting' || item.startState === 'started'
}

export interface ActiveProposalCounts {
  total: number
  byBot: Map<string, number>
}

/** Conta tarefas ativas ligadas a propostas aprovadas (total e por bot). */
export function countActiveProposalTasks(proposals: readonly TaskProposal[]): ActiveProposalCounts {
  let total = 0
  const byBot = new Map<string, number>()
  for (const p of proposals) {
    if (p.state !== 'approved') continue
    for (const item of p.items) {
      if (!isActiveProposalTask(item)) continue
      total++
      if (item.assigneeBotId) byBot.set(item.assigneeBotId, (byBot.get(item.assigneeBotId) ?? 0) + 1)
    }
  }
  return { total, byBot }
}

/** Informa se o limite global ou por bot já está ocupado pelas tarefas ativas atuais. */
export function isSlotBlocked(limits: ProposalLimits, active: ActiveProposalCounts, assigneeBotId: string | null): boolean {
  if (active.total >= limits.total) return true
  if (assigneeBotId !== null && (active.byBot.get(assigneeBotId) ?? 0) >= limits.perBot) return true
  return false
}

/** Rótulo e tom de exibição do início da tarefa (`scheduled` vira Aguardando vaga quando o limite estiver cheio). */
export function startStateBadge(startState: ProposalStartState, waitingForSlot: boolean): { key: ProposalStartStateKey; tone: ProposalBadgeTone } {
  switch (startState) {
    case 'scheduled':
      return waitingForSlot
        ? { key: 'propostas.inicio.aguardandoVaga', tone: 'attention' }
        : { key: 'propostas.inicio.scheduled', tone: 'neutral' }
    case 'starting':
      return { key: 'propostas.inicio.starting', tone: 'accent' }
    case 'started':
      return { key: 'propostas.inicio.started', tone: 'success' }
    case 'failed':
      return { key: 'propostas.inicio.failed', tone: 'danger' }
    case 'refused':
      return { key: 'propostas.inicio.refused', tone: 'danger' }
    case 'uncertain':
      return { key: 'propostas.inicio.uncertain', tone: 'attention' }
  }
}

/** Resolve o selo de início considerando os limites e tarefas ativas do snapshot. */
export function resolveItemStartBadge(
  item: Pick<ProposalItem, 'assigneeBotId' | 'startState'>,
  limits: ProposalLimits,
  active: ActiveProposalCounts,
): { key: ProposalStartStateKey; tone: ProposalBadgeTone } | null {
  if (!item.startState) return null
  const waitingForSlot = item.startState === 'scheduled' && isSlotBlocked(limits, active, item.assigneeBotId)
  return startStateBadge(item.startState, waitingForSlot)
}

/** Item incluído de proposta aprovada cuja tarefa ainda não foi concluída nem cancelada. */
export function isProposalItemOpen(item: Pick<ProposalItem, 'included' | 'taskState'>): boolean {
  return item.included && item.taskState !== 'done' && item.taskState !== 'cancelled'
}

/** Proposta deve aparecer no cartão: pendente, ou aprovada com pelo menos uma tarefa aberta. */
export function isProposalVisible(proposal: TaskProposal): boolean {
  if (proposal.state === 'pending') return true
  if (proposal.state === 'approved') return proposal.items.some(isProposalItemOpen)
  return false
}

/** Filtra e ordena propostas visíveis (pendentes primeiro, depois aprovadas com tarefas abertas). */
export function visibleProposals(snapshot: TaskProposalsSnapshot | null): TaskProposal[] {
  if (!snapshot) return []
  const pending: TaskProposal[] = []
  const approved: TaskProposal[] = []
  for (const p of snapshot.proposals) {
    if (!isProposalVisible(p)) continue
    if (p.state === 'pending') pending.push(p)
    else approved.push(p)
  }
  return [...pending, ...approved]
}

/** Itens que permanecerão incluídos ao aprovar após aplicar a lista de omissões do dono. */
export function includedPendingItemIds(proposal: TaskProposal, omittedIds: ReadonlySet<string>): string[] {
  return proposal.items.filter((it) => it.included && !omittedIds.has(it.id)).map((it) => it.id)
}

export interface PrincipalDiscoverySource {
  spaceId?: string
  spaces?: SpacesSnapshot | null
  hello?: { spaceId?: string; spaces?: SpacesSnapshot } | null
  principal?: { id: string | null } | null
}

/**
 * Descobre o Space ativo e o `principalBotId` declarado para ele no store/hello/spaces.
 * Nunca presume o principal do Pessoal em outro Space.
 */
export function activeSpaceAndPrincipal(source: PrincipalDiscoverySource): { spaceId: string | null; principalBotId: string | null } {
  const spaces = source.spaces ?? source.hello?.spaces ?? null
  const spaceId = source.spaceId ?? source.hello?.spaceId ?? spaces?.personalSpaceId ?? null
  if (!spaceId) return { spaceId: null, principalBotId: null }
  const space = spaces?.spaces.find((s) => s.id === spaceId) ?? null
  if (space?.principalBotId) return { spaceId, principalBotId: space.principalBotId }
  const isPersonal = !!spaces?.personalSpaceId && spaceId === spaces.personalSpaceId
  if (isPersonal && source.principal?.id && (!space || space.kind === 'personal')) {
    return { spaceId, principalBotId: source.principal.id }
  }
  return { spaceId, principalBotId: null }
}
