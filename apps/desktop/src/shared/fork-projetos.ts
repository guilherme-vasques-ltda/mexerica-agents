// D541 fatia 1 (docs/reference/redesenho/components/ProjetosKanban/README.md): Projetos e tarefas MANUAIS do dono.
// Só regras puras: forma dos DTOs `donnaProjects`/`donnaProject`/`donnaProjectCreate`/`donnaProjectSave`, conferência
// estrita da resposta (outra origem, campo faltando ou tipo errado: nada é mostrado), árvore de pais e o corpo da
// gravação. Nada aqui inicia bot, envia mensagem ou executa tarefa; os estados são declarados pelo dono.

export const PROJECT_TASK_STATES = ['backlog', 'in_progress', 'review', 'done', 'blocked', 'cancelled'] as const
export type ProjectTaskState = (typeof PROJECT_TASK_STATES)[number]
export type ProjectState = 'active' | 'archived'

/** Limites de proteção da UI (contrato D541): o Hub confere de novo e é quem decide. */
export const PROJECT_LIMITS = {
  projects: 50,
  tasks: 100,
  depth: 3,
  name: 120,
  objective: 2000,
  title: 200,
  description: 4000,
  acceptance: 2000,
  evidence: 4000,
} as const

export const PROJECTS_CAPABILITY = 'manualProjects'

export interface ProjectBot { id: string; name: string }

export interface ProjectSummary {
  id: string
  name: string
  objective: string
  coordinatorBotId: string | null
  state: ProjectState
  revision: number
  createdAt: number
  updatedAt: number
}

export interface ProjectTask {
  id: string
  parentTaskId: string | null
  assigneeBotId: string | null
  title: string
  description: string
  acceptance: string
  evidence: string
  state: ProjectTaskState
}

/** Autor da mudança: o dono, um bot (`bot:<id>`, quando o Hub registrar escrita de bot) ou o sistema. */
export type HistoryAuthor = 'owner' | 'system' | `bot:${string}`
export interface ProjectHistoryItem { id: string; at: number; author: HistoryAuthor; action: string; taskId: string | null; summary: string }

/**
 * Concluída declarada (decisão 2, TEC-722-17): fato gravado pelo Hub quando um bot leva a tarefa a Concluída (com
 * prova). Nunca é deduzido do histórico; sem o campo (Hub anterior), nada é declarado.
 */
export interface TaskDeclaration { botId: string; at: number }

export function parseDeclarations(v: unknown): Record<string, TaskDeclaration> {
  if (v === undefined || v === null) return {}
  const o = need(isObj(v), v as Obj, 'declarations')
  const out: Record<string, TaskDeclaration> = {}
  for (const [id, d] of Object.entries(o)) {
    const x = need(isObj(d), d as Obj, 'declaration')
    out[id] = { botId: str(x, 'botId'), at: num(x, 'at') }
  }
  return out
}

export interface ProjectsList {
  hubId: string
  spaceId: string
  projects: ProjectSummary[]
  bots: ProjectBot[]
  capable: boolean
  /** O que mais o Hub anuncia (Tarefas, MX-142); sem o anúncio, o controle não aparece. */
  capabilities: string[]
  /** Projeto padrão (“Geral”), só quando o Hub o anuncia (`defaultProject`). */
  defaultProjectId: string | null
}

export interface ProjectSnapshot {
  hubId: string
  spaceId: string
  project: ProjectSummary
  tasks: ProjectTask[]
  history: ProjectHistoryItem[]
  historyOmitted: number
  bots: ProjectBot[]
  /** Tarefas Concluídas declaradas por bot (fato do Hub); vazio sem o campo. */
  declarations: Record<string, TaskDeclaration>
}

export interface ProjectReceipt { clientRequestId: string; revision: number }

/** O que o app grava: o Projeto inteiro, como o dono o deixou. */
export interface ProjectDraft {
  name: string
  objective: string
  coordinatorBotId: string | null
  state: ProjectState
  tasks: ProjectTask[]
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

/** UUID canônico (minúsculo, com hífens), como o Hub exige para IDs de tarefa e `clientRequestId`. */
export function isCanonicalUuid(v: unknown): v is string {
  return typeof v === 'string' && UUID.test(v)
}

export function newUuid(): string {
  return crypto.randomUUID().toLowerCase()
}

export class ProjectsAnswerError extends Error {
  constructor(what: string) {
    super(`projects_invalid_answer: ${what}`)
  }
}

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
function need<T>(ok: boolean, value: T, what: string): T {
  if (!ok) throw new ProjectsAnswerError(what)
  return value
}
const str = (o: Obj, k: string) => need(typeof o[k] === 'string', o[k] as string, k)
const num = (o: Obj, k: string) => need(typeof o[k] === 'number' && Number.isFinite(o[k]), o[k] as number, k)
const optStr = (o: Obj, k: string) => need(o[k] === null || o[k] === undefined || typeof o[k] === 'string', (o[k] ?? null) as string | null, k)
const arr = (o: Obj, k: string) => need(Array.isArray(o[k]), o[k] as unknown[], k)

function parseBots(o: Obj): ProjectBot[] {
  return arr(o, 'bots').map((b) => {
    const x = need(isObj(b), b as Obj, 'bot')
    return { id: str(x, 'id'), name: str(x, 'name') }
  })
}

export function parseProject(v: unknown): ProjectSummary {
  const o = need(isObj(v), v as Obj, 'project')
  const state = str(o, 'state')
  return {
    id: str(o, 'id'),
    name: str(o, 'name'),
    objective: str(o, 'objective'),
    coordinatorBotId: optStr(o, 'coordinatorBotId'),
    state: need(state === 'active' || state === 'archived', state as ProjectState, 'project.state'),
    revision: num(o, 'revision'),
    createdAt: num(o, 'createdAt'),
    updatedAt: num(o, 'updatedAt'),
  }
}

export function parseTask(v: unknown): ProjectTask {
  const o = need(isObj(v), v as Obj, 'task')
  const state = str(o, 'state')
  return {
    id: str(o, 'id'),
    parentTaskId: optStr(o, 'parentTaskId'),
    assigneeBotId: optStr(o, 'assigneeBotId'),
    title: str(o, 'title'),
    description: str(o, 'description'),
    acceptance: str(o, 'acceptance'),
    evidence: str(o, 'evidence'),
    state: need((PROJECT_TASK_STATES as readonly string[]).includes(state), state as ProjectTaskState, 'task.state'),
  }
}

function parseHistory(v: unknown): ProjectHistoryItem {
  const o = need(isObj(v), v as Obj, 'history')
  return {
    id: String(need(typeof o.id === 'string' || typeof o.id === 'number', o.id, 'history.id')),
    at: num(o, 'at'),
    author: need(o.author === 'owner' || o.author === 'system' || (typeof o.author === 'string' && /^bot:.+/.test(o.author)), o.author as HistoryAuthor, 'history.author'),
    action: str(o, 'action'),
    taskId: optStr(o, 'taskId'),
    summary: str(o, 'summary'),
  }
}

/** A resposta tem de ser da mesma origem pedida (Hub e space); outra origem nunca é pintada. */
function sameOrigin(o: Obj, origin: { hubId: string; spaceId: string }) {
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
}

export function parseProjectsList(v: unknown, origin: { hubId: string; spaceId: string }): ProjectsList {
  const o = need(isObj(v), v as Obj, 'answer')
  sameOrigin(o, origin)
  const capabilities = arr(o, 'capabilities').filter((c): c is string => typeof c === 'string')
  const projects = arr(o, 'projects').map(parseProject)
  const defaultProjectId = capabilities.includes('defaultProject') ? optStr(o, 'defaultProjectId') : null
  return {
    hubId: origin.hubId,
    spaceId: origin.spaceId,
    projects,
    bots: parseBots(o),
    capable: capabilities.includes(PROJECTS_CAPABILITY),
    capabilities,
    // O padrão só vale se for um Projeto da própria lista.
    defaultProjectId: defaultProjectId && projects.some((p) => p.id === defaultProjectId) ? defaultProjectId : null,
  }
}

export function parseProjectSnapshot(v: unknown, origin: { hubId: string; spaceId: string; projectId?: string }): ProjectSnapshot {
  const o = need(isObj(v), v as Obj, 'answer')
  sameOrigin(o, origin)
  const project = parseProject(o.project)
  if (origin.projectId) need(project.id === origin.projectId, null, 'project.id')
  return {
    hubId: origin.hubId,
    spaceId: origin.spaceId,
    project,
    tasks: arr(o, 'tasks').map(parseTask),
    history: arr(o, 'history').map(parseHistory),
    historyOmitted: typeof o.historyOmitted === 'number' ? o.historyOmitted : 0,
    bots: parseBots(o),
    declarations: parseDeclarations(o.declarations),
  }
}

/** Resposta de create/save: o snapshot novo e o recibo do MESMO pedido. */
export function parseWriteAnswer(v: unknown, origin: { hubId: string; spaceId: string; projectId?: string }, clientRequestId: string) {
  const o = need(isObj(v), v as Obj, 'answer')
  const receipt = need(isObj(o.receipt), o.receipt as Obj, 'receipt')
  need(receipt.clientRequestId === clientRequestId, null, 'receipt.clientRequestId')
  return { snapshot: parseProjectSnapshot(o.snapshot, origin), receipt: { clientRequestId, revision: num(receipt, 'revision') } }
}

// MARK: árvore de pais

/** Nível da tarefa (1 = sem pai). Pai ausente ou ciclo contam como até onde foi possível subir. */
export function depthOf(tasks: readonly ProjectTask[], id: string): number {
  const byId = new Map(tasks.map((t) => [t.id, t]))
  let depth = 0
  const seen = new Set<string>()
  let cur = byId.get(id)
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id)
    depth++
    cur = cur.parentTaskId ? byId.get(cur.parentTaskId) : undefined
  }
  return depth
}

/** Quantos níveis a tarefa ocupa com seus descendentes (1 = sem filhos). */
function heightOf(tasks: readonly ProjectTask[], id: string, seen = new Set<string>()): number {
  if (seen.has(id)) return 0
  seen.add(id)
  const kids = tasks.filter((t) => t.parentTaskId === id)
  return 1 + Math.max(0, ...kids.map((k) => heightOf(tasks, k.id, seen)))
}

function isDescendant(tasks: readonly ProjectTask[], ancestor: string, id: string): boolean {
  const byId = new Map(tasks.map((t) => [t.id, t]))
  const seen = new Set<string>()
  let cur = byId.get(id)
  while (cur?.parentTaskId && !seen.has(cur.id)) {
    seen.add(cur.id)
    if (cur.parentTaskId === ancestor) return true
    cur = byId.get(cur.parentTaskId)
  }
  return false
}

/** Pais possíveis para `taskId` (null: tarefa nova): mesmo Projeto, sem ciclo e sem passar de 3 níveis. */
export function parentOptions(tasks: readonly ProjectTask[], taskId: string | null): ProjectTask[] {
  const height = taskId ? heightOf(tasks, taskId) : 1
  return tasks.filter((t) => t.id !== taskId && !(taskId && isDescendant(tasks, taskId, t.id)) && depthOf(tasks, t.id) + height <= PROJECT_LIMITS.depth)
}

// MARK: gravação

export type DraftProblem = 'nameRequired' | 'titleRequired' | 'tooLong' | 'tooManyTasks' | 'badParent' | 'removedTask'

/** O que impede enviar; o Hub confere tudo de novo. `base`: as tarefas já gravadas, que não podem sumir. */
export function draftProblem(draft: ProjectDraft, base: readonly ProjectTask[] = []): DraftProblem | null {
  if (!draft.name.trim()) return 'nameRequired'
  if (draft.name.length > PROJECT_LIMITS.name || draft.objective.length > PROJECT_LIMITS.objective) return 'tooLong'
  if (draft.tasks.length > PROJECT_LIMITS.tasks) return 'tooManyTasks'
  const ids = new Set(draft.tasks.map((t) => t.id))
  if (base.some((t) => !ids.has(t.id))) return 'removedTask'
  for (const t of draft.tasks) {
    if (!t.title.trim()) return 'titleRequired'
    if (t.title.length > PROJECT_LIMITS.title || t.description.length > PROJECT_LIMITS.description
      || t.acceptance.length > PROJECT_LIMITS.acceptance || t.evidence.length > PROJECT_LIMITS.evidence) return 'tooLong'
    if (t.parentTaskId && (!ids.has(t.parentTaskId) || t.parentTaskId === t.id)) return 'badParent'
    if (t.parentTaskId && depthOf(draft.tasks, t.id) > PROJECT_LIMITS.depth) return 'badParent'
    if (t.parentTaskId && isDescendant(draft.tasks, t.id, t.id)) return 'badParent'
  }
  // Ciclo: subir a partir de cada tarefa tem de terminar numa sem pai.
  const byId = new Map(draft.tasks.map((t) => [t.id, t]))
  for (const t of draft.tasks) {
    const seen = new Set<string>()
    let cur: ProjectTask | undefined = t
    while (cur?.parentTaskId) {
      if (seen.has(cur.id)) return 'badParent'
      seen.add(cur.id)
      cur = byId.get(cur.parentTaskId)
    }
  }
  return null
}

/** O rascunho a partir do que o Hub tem agora (ponto de partida de toda mudança). */
export function draftOf(s: ProjectSnapshot): ProjectDraft {
  return {
    name: s.project.name,
    objective: s.project.objective,
    coordinatorBotId: s.project.coordinatorBotId,
    state: s.project.state,
    tasks: s.tasks.map((t) => ({ ...t })),
  }
}

export function withTask(draft: ProjectDraft, task: ProjectTask): ProjectDraft {
  const exists = draft.tasks.some((t) => t.id === task.id)
  return { ...draft, tasks: exists ? draft.tasks.map((t) => (t.id === task.id ? { ...task } : t)) : [...draft.tasks, { ...task }] }
}

export function blankTask(state: ProjectTaskState = 'backlog'): ProjectTask {
  return { id: newUuid(), parentTaskId: null, assigneeBotId: null, title: '', description: '', acceptance: '', evidence: '', state }
}

export function saveBody(s: Pick<ProjectSnapshot, 'hubId' | 'spaceId' | 'project'>, draft: ProjectDraft, clientRequestId: string) {
  return {
    hubId: s.hubId,
    spaceId: s.spaceId,
    projectId: s.project.id,
    clientRequestId,
    expectedRevision: s.project.revision,
    name: draft.name,
    objective: draft.objective,
    coordinatorBotId: draft.coordinatorBotId,
    state: draft.state,
    tasks: draft.tasks.map((t) => ({
      id: t.id, parentTaskId: t.parentTaskId, assigneeBotId: t.assigneeBotId, title: t.title,
      description: t.description, acceptance: t.acceptance, evidence: t.evidence, state: t.state,
    })),
  }
}

export function createBody(origin: { hubId: string; spaceId: string }, fields: { name: string; objective: string; coordinatorBotId: string | null }, clientRequestId: string) {
  return { hubId: origin.hubId, spaceId: origin.spaceId, clientRequestId, name: fields.name, objective: fields.objective, coordinatorBotId: fields.coordinatorBotId }
}

/** Mesmo conteúdo de tarefa (todos os campos que o dono edita): base da folha × fotografia atual. */
export function sameTaskContent(a: ProjectTask, b: ProjectTask): boolean {
  return a.id === b.id && a.parentTaskId === b.parentTaskId && a.assigneeBotId === b.assigneeBotId && a.title === b.title
    && a.description === b.description && a.acceptance === b.acceptance && a.evidence === b.evidence && a.state === b.state
}

export type ProjectFields = Pick<ProjectSummary, 'name' | 'objective' | 'coordinatorBotId' | 'state'>
export function projectFields(p: ProjectFields): ProjectFields {
  return { name: p.name, objective: p.objective, coordinatorBotId: p.coordinatorBotId, state: p.state }
}
/** Mesmos campos do Projeto (sem as tarefas, que a folha do Projeto não edita). */
export function sameProjectFields(a: ProjectFields, b: ProjectFields): boolean {
  return a.name === b.name && a.objective === b.objective && a.coordinatorBotId === b.coordinatorBotId && a.state === b.state
}

/**
 * A folha abriu sobre `base`; `mine` é o rascunho e `now` o que o Hub tem agora. `follow`: o dono não mexeu, então a
 * folha acompanha a versão atual (nada se perde). `conflict`: mudou em outro lugar E o rascunho difere do atual; nada é
 * salvo até o dono escolher. Igual ao atual (ex.: o próprio pedido incerto que chegou) não é conflito.
 */
export function editState<T>(base: T, mine: T, now: T | undefined, same: (a: T, b: T) => boolean): 'clean' | 'follow' | 'conflict' | 'gone' {
  if (now === undefined) return 'gone'
  if (same(base, now) || same(mine, now)) return 'clean'
  return same(base, mine) ? 'follow' : 'conflict'
}

/**
 * “Manter a minha” num conflito: fica o que o dono mudou nesta folha (campo diferente da base) e o resto vem da versão
 * atual, para não desfazer de volta o que mudou em outro aparelho num campo que o dono nem tocou.
 */
export function keepMine<T extends object>(base: T, mine: T, now: T): T {
  const out = { ...now }
  for (const k of Object.keys(mine) as (keyof T)[]) if (mine[k] !== base[k]) out[k] = mine[k]
  return out
}

/** A coluna vizinha (teclado Alt+←/→), sem dar a volta. */
export function neighborState(state: ProjectTaskState, dir: -1 | 1): ProjectTaskState | null {
  const i = PROJECT_TASK_STATES.indexOf(state) + dir
  return PROJECT_TASK_STATES[i] ?? null
}

/** Última mudança registrada de cada tarefa (pelo histórico do Hub). */
export function lastChangeByTask(history: readonly ProjectHistoryItem[]): Map<string, number> {
  const out = new Map<string, number>()
  for (const h of history) if (h.taskId && (out.get(h.taskId) ?? -Infinity) < h.at) out.set(h.taskId, h.at)
  return out
}

/** Responsável/coordenador: o nome do bot visível, ou indisponível sem expor nada (removido ou oculto). */
export function botLabel(bots: readonly ProjectBot[], id: string | null): { kind: 'none' } | { kind: 'bot'; name: string } | { kind: 'unavailable' } {
  if (!id) return { kind: 'none' }
  const bot = bots.find((b) => b.id === id)
  return bot ? { kind: 'bot', name: bot.name } : { kind: 'unavailable' }
}
