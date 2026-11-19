// MX-142 etapa 1 (docs/reference/redesenho/components/Tarefas/README.md): a área **Tarefas** sobre os Projetos do #620.
// Só regras puras: capacidades anunciadas pelo Hub (fail-closed), metadados da tarefa (prioridade, prazo, fases),
// comentários do dono com o status de aviso que o Hub devolve, e o agrupamento/filtro da Lista e do Quadro. Nada aqui
// inicia bot, envia mensagem ou deduz que alguém foi avisado: o Hub decide e a tela só mostra o que ele respondeu.
import {
  PROJECT_TASK_STATES, ProjectsAnswerError, depthOf, parseDeclarations, parseProjectSnapshot, type TaskDeclaration,
  type ProjectHistoryItem, type ProjectSnapshot, type ProjectSummary, type ProjectTask, type ProjectTaskState, type ProjectBot,
} from './fork-projetos.ts'
import { isolado } from './bidi.ts'

/**
 * Capacidades que o Hub anuncia em `donnaProjects.capabilities`. Sem o anúncio, o controle não aparece (fail-closed):
 * nada indisponível é mostrado como se funcionasse. Anexos inline ainda não têm capacidade aqui: a tela não os
 * oferece até o Hub ter a função de verdade.
 */
export const TASK_CAPABILITIES = {
  list: 'taskList',
  meta: 'taskMeta',
  comments: 'taskComments',
  defaultProject: 'defaultProject',
  /** Hub B (tarefas_runtime): iniciar agora, agendar o início, ler a agenda e as sessões vinculadas. */
  start: 'taskStart',
  schedule: 'taskSchedule',
  agenda: 'taskAgenda',
  sessions: 'taskSessions',
} as const
export type TaskCapability = (typeof TASK_CAPABILITIES)[keyof typeof TASK_CAPABILITIES]

export const TASK_PRIORITIES = ['low', 'normal', 'high', 'urgent'] as const
export type TaskPriority = (typeof TASK_PRIORITIES)[number]

export const TASK_LIMITS = { phases: 20, phase: 120, comment: 4000, pages: 50 } as const

/** Metadados da tarefa (contrato v1 do Hub: sem `startAt` até existir o início agendado de verdade). */
export interface TaskMeta {
  priority: TaskPriority | null
  dueAt: number | null
  phases: string[]
  currentPhase: number | null
}

export const EMPTY_META: TaskMeta = Object.freeze({ priority: null, dueAt: null, phases: [], currentPhase: null }) as TaskMeta

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
function need<T>(ok: boolean, value: T, what: string): T {
  if (!ok) throw new ProjectsAnswerError(what)
  return value
}
const str = (o: Obj, k: string) => need(typeof o[k] === 'string', o[k] as string, k)
const num = (o: Obj, k: string) => need(typeof o[k] === 'number' && Number.isFinite(o[k]), o[k] as number, k)
const arr = (o: Obj, k: string) => need(Array.isArray(o[k]), o[k] as unknown[], k)
const optNum = (o: Obj, k: string) => need(o[k] === null || o[k] === undefined || (typeof o[k] === 'number' && Number.isFinite(o[k])), (o[k] ?? null) as number | null, k)

export function parseTaskMeta(v: unknown): TaskMeta {
  const o = need(isObj(v), v as Obj, 'meta')
  const priority = o.priority ?? null
  need(priority === null || (TASK_PRIORITIES as readonly unknown[]).includes(priority), null, 'meta.priority')
  const phases = (o.phases ?? []) as unknown[]
  need(Array.isArray(phases) && phases.every((p) => typeof p === 'string'), null, 'meta.phases')
  const currentPhase = optNum(o, 'currentPhase')
  need(currentPhase === null || (Number.isInteger(currentPhase) && currentPhase >= 0 && currentPhase < phases.length), null, 'meta.currentPhase')
  return { priority: priority as TaskPriority | null, dueAt: optNum(o, 'dueAt'), phases: phases as string[], currentPhase }
}

export function parseMetaMap(v: unknown): Record<string, TaskMeta> {
  if (v === undefined || v === null) return {}
  const o = need(isObj(v), v as Obj, 'meta')
  const out: Record<string, TaskMeta> = {}
  for (const [id, m] of Object.entries(o)) out[id] = parseTaskMeta(m)
  return out
}

export const metaOf = (meta: Readonly<Record<string, TaskMeta>>, id: string): TaskMeta => meta[id] ?? EMPTY_META

export function sameMeta(a: TaskMeta, b: TaskMeta): boolean {
  return a.priority === b.priority && a.dueAt === b.dueAt && a.currentPhase === b.currentPhase
    && a.phases.length === b.phases.length && a.phases.every((p, i) => p === b.phases[i])
}

/**
 * Edição parcial de metadados sobre a versão relida (`fresh`, do MESMO snapshot da revisão do CAS): o que o dono
 * mudou (`mine` ≠ `base`) vale; o resto vem de `fresh`, para não desfazer o que outro cliente gravou. Fases e fase
 * atual andam juntas. Conflito só quando o MESMO campo mudou aqui e lá para valores diferentes.
 */
export function mergeMeta(base: TaskMeta, mine: TaskMeta, fresh: TaskMeta): { meta: TaskMeta; conflict: boolean } {
  const samePhases = (a: TaskMeta, b: TaskMeta) => a.currentPhase === b.currentPhase && a.phases.length === b.phases.length && a.phases.every((p, i) => p === b.phases[i])
  let conflict = false
  const pick = <T>(changed: boolean, theirs: boolean, same: boolean, m: T, f: T): T => {
    if (!changed) return f
    if (theirs && !same) conflict = true
    return m
  }
  const priority = pick(mine.priority !== base.priority, fresh.priority !== base.priority, mine.priority === fresh.priority, mine.priority, fresh.priority)
  const dueAt = pick(mine.dueAt !== base.dueAt, fresh.dueAt !== base.dueAt, mine.dueAt === fresh.dueAt, mine.dueAt, fresh.dueAt)
  const phases = pick(!samePhases(mine, base), !samePhases(fresh, base), samePhases(mine, fresh), mine, fresh)
  return { meta: { priority, dueAt, phases: [...phases.phases], currentPhase: phases.currentPhase }, conflict }
}

export type MetaProblem = 'tooManyPhases' | 'phaseTooLong' | 'phaseEmpty' | 'phasesOnlyRoot' | 'badDue'

/** O que impede enviar os metadados (o Hub confere tudo de novo). Fases só na tarefa principal (sem pai). */
export function metaProblem(meta: TaskMeta, task: Pick<ProjectTask, 'parentTaskId'>): MetaProblem | null {
  if (meta.dueAt !== null && (!Number.isFinite(meta.dueAt) || meta.dueAt < 0)) return 'badDue'
  if (meta.phases.length && task.parentTaskId) return 'phasesOnlyRoot'
  if (meta.phases.length > TASK_LIMITS.phases) return 'tooManyPhases'
  if (meta.phases.some((p) => !p.trim())) return 'phaseEmpty'
  if (meta.phases.some((p) => p.length > TASK_LIMITS.phase)) return 'phaseTooLong'
  return null
}

/** Fases escritas uma por linha (linhas vazias saem); a fase atual cai se sumir. */
export function phasesFromText(text: string, current: number | null): Pick<TaskMeta, 'phases' | 'currentPhase'> {
  const phases = text.split('\n').map((l) => l.trim()).filter(Boolean)
  return { phases, currentPhase: current !== null && current < phases.length ? current : null }
}

/**
 * A fase atual acompanha a LINHA dela quando o dono edita o texto: apagar ou inserir uma linha antes dela não passa a
 * “fase atual” para outra fase sem aviso. Regra, nesta ordem:
 * 1. linhas iguais no começo (prefixo) e no fim (sufixo) não mudaram: a atual nelas fica na mesma linha (no sufixo,
 *    contada a partir do fim);
 * 2. a atual está no trecho editado: o mesmo nome dentro do trecho novo; nome repetido → a ocorrência mais perto da
 *    posição antiga; empate → a mais cedo;
 * 3. sem o nome: renomeada no lugar (mesmo número de fases) fica na mesma posição; senão sem fase atual (o campo mostra
 *    isso; nada é escolhido sozinho).
 */
export function followCurrentPhase(prev: readonly string[], current: number | null, next: readonly string[]): number | null {
  if (current === null || current >= prev.length) return null
  let pre = 0
  while (pre < prev.length && pre < next.length && prev[pre] === next[pre]) pre++
  let suf = 0
  while (suf < prev.length - pre && suf < next.length - pre && prev[prev.length - 1 - suf] === next[next.length - 1 - suf]) suf++
  if (current < pre) return current
  if (current >= prev.length - suf) return next.length - (prev.length - current)
  const name = prev[current]
  let best: number | null = null
  for (let i = pre; i < next.length - suf; i++) {
    if (next[i] === name && (best === null || Math.abs(i - current) < Math.abs(best - current))) best = i
  }
  if (best !== null) return best
  return prev.length === next.length ? current : null
}

/** Os metadados da folha como o dono os vê agora: as fases vêm do texto digitado, não do último envio. */
export function sheetMeta(meta: TaskMeta, phasesText: string): TaskMeta {
  return { ...meta, ...phasesFromText(phasesText, meta.currentPhase) }
}

export type SheetMetaSync =
  | { kind: 'clean' }
  | { kind: 'rebase' | 'conflict'; meta: TaskMeta; phasesText: string }

/**
 * Folha de Editar tarefa aberta e uma releitura trouxe outro meta (`now`) para a tarefa. O que o dono mexeu na folha
 * (inclusive fases só digitadas, ainda fora de `meta`) fica; o resto segue a versão nova, campo a campo (`mergeMeta`).
 * `conflict`: o MESMO campo mudou aqui e lá; `meta`/`phasesText` são o “Manter a minha” (o do dono vale nesse campo).
 * O texto das fases só é trocado quando as fases que valem são outras: linhas em branco do meio da digitação ficam.
 */
export function syncSheetMeta(base: TaskMeta, meta: TaskMeta, phasesText: string, now: TaskMeta): SheetMetaSync {
  if (sameMeta(base, now)) return { kind: 'clean' }
  const merged = mergeMeta(base, sheetMeta(meta, phasesText), now)
  const typed = phasesFromText(phasesText, null).phases
  const keepText = typed.length === merged.meta.phases.length && typed.every((p, i) => p === merged.meta.phases[i])
  return { kind: merged.conflict ? 'conflict' : 'rebase', meta: merged.meta, phasesText: keepText ? phasesText : merged.meta.phases.join('\n') }
}

export function metaBody(s: Pick<ProjectSnapshot, 'hubId' | 'spaceId' | 'project'>, taskId: string, meta: TaskMeta, clientRequestId: string) {
  return {
    hubId: s.hubId,
    spaceId: s.spaceId,
    projectId: s.project.id,
    taskId,
    clientRequestId,
    expectedRevision: s.project.revision,
    meta: { priority: meta.priority, dueAt: meta.dueAt, phases: [...meta.phases], currentPhase: meta.currentPhase },
  }
}

// MARK: leitura de todas as tarefas do Space

/** Um Projeto ativo com as suas tarefas. `history` só vem da leitura completa (`donnaProject`). */
export interface TaskBundle {
  project: ProjectSummary
  tasks: ProjectTask[]
  meta: Record<string, TaskMeta>
  history: ProjectHistoryItem[] | null
  declarations: Record<string, TaskDeclaration>
}

export function bundleOf(s: ProjectSnapshot, meta: Record<string, TaskMeta> = {}): TaskBundle {
  return { project: s.project, tasks: s.tasks, meta, history: s.history, declarations: s.declarations }
}

/** Uma página de `donnaTasks` (um Projeto por página), conferida contra a origem pedida. */
export function parseTasksPage(v: unknown, origin: { hubId: string; spaceId: string }, parse: { project: (v: unknown) => ProjectSummary; task: (v: unknown) => ProjectTask }): { bundle: TaskBundle | null; next: string | null } {
  const o = need(isObj(v), v as Obj, 'answer')
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
  const next = o.next ?? null
  need(next === null || typeof next === 'string', null, 'next')
  if (o.page === null || o.page === undefined) return { bundle: null, next: next as string | null }
  const page = need(isObj(o.page), o.page as Obj, 'page')
  const project = parse.project(page.project)
  return { bundle: { project, tasks: arr(page, 'tasks').map(parse.task), meta: parseMetaMap(page.meta), history: null, declarations: parseDeclarations(page.declarations) }, next: next as string | null }
}

/** Resposta de `donnaProjectEnsureDefault` (idempotente): o snapshot do Geral, da mesma origem pedida. */
export function parseEnsureDefault(v: unknown, origin: { hubId: string; spaceId: string }): { snapshot: ProjectSnapshot; created: boolean } {
  const o = need(isObj(v), v as Obj, 'answer')
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
  const snapshot = parseProjectSnapshot(o.snapshot, origin)
  need(snapshot.project.state === 'active', null, 'snapshot.project.state')
  return { snapshot, created: o.created === true }
}

// MARK: comentários

export type CommentAuthor = { kind: 'owner' } | { kind: 'bot'; botId: string } | { kind: 'system' }

export function parseAuthor(v: unknown): CommentAuthor {
  if (v === 'owner') return { kind: 'owner' }
  if (v === 'system') return { kind: 'system' }
  if (typeof v === 'string' && v.startsWith('bot:') && v.length > 4) return { kind: 'bot', botId: v.slice(4) }
  throw new ProjectsAnswerError('author')
}

/** Estado do aviso de um comentário, como o Hub diz. A tela nunca deduz que avisou. */
export const NOTIFY_STATES = ['not_notified', 'notifying', 'responding', 'responded', 'uncertain', 'partial'] as const
export type NotifyState = (typeof NOTIFY_STATES)[number]
/** Situação de cada @ resolvido (TEC-722-15): o resumo nunca pode dizer “ninguém” se algum recebeu. */
export const RECIPIENT_STATES = ['queued', 'sending', 'sent', 'answered', 'failed', 'not_notified'] as const
export type RecipientState = (typeof RECIPIENT_STATES)[number] | 'unknown'
export interface NotifyRecipient { botId: string; state: RecipientState; reason: string | null }
export interface CommentNotify { state: NotifyState; botIds: string[]; reason: string | null; recipients: NotifyRecipient[] }

/** Algum destinatário de fato recebeu (entregue ou respondeu). */
export const someoneReached = (n: Pick<CommentNotify, 'recipients'>) => n.recipients.some((r) => r.state === 'sent' || r.state === 'answered')

export interface TaskComment {
  id: string
  at: number
  author: CommentAuthor
  text: string
  mentions: string[]
  onlyNote: boolean
  notify: CommentNotify | null
}

function parseNotify(v: unknown): CommentNotify | null {
  if (v === null || v === undefined) return null
  const o = need(isObj(v), v as Obj, 'notify')
  const state = str(o, 'state')
  const botIds = arr(o, 'botIds')
  need(botIds.every((b) => typeof b === 'string'), null, 'notify.botIds')
  const reason = o.reason ?? null
  need(reason === null || typeof reason === 'string', null, 'notify.reason')
  const recipients = ((o.recipients ?? []) as unknown[])
  need(Array.isArray(recipients), null, 'notify.recipients')
  const parsed: NotifyRecipient[] = recipients.map((x) => {
    const r = need(isObj(x), x as Obj, 'notify.recipient')
    const st = str(r, 'state')
    const why = r.reason ?? null
    need(why === null || typeof why === 'string', null, 'notify.recipient.reason')
    return { botId: str(r, 'botId'), state: (RECIPIENT_STATES as readonly string[]).includes(st) ? st as RecipientState : 'unknown', reason: why as string | null }
  })
  // Estado novo que esta versão não conhece: mostrado como incerto, nunca como avisado.
  let s: NotifyState = (NOTIFY_STATES as readonly string[]).includes(state) ? state as NotifyState : 'uncertain'
  // “Ninguém avisado” com alguém que recebeu é contradição: vira parcial (nunca afirma zero entregas).
  if (s === 'not_notified' && someoneReached({ recipients: parsed })) s = 'partial'
  return { state: s, botIds: botIds as string[], reason: reason as string | null, recipients: parsed }
}

export function parseComment(v: unknown): TaskComment {
  const o = need(isObj(v), v as Obj, 'comment')
  const mentions = (o.mentions ?? []) as unknown[]
  need(Array.isArray(mentions) && mentions.every((m) => typeof m === 'string'), null, 'comment.mentions')
  return {
    id: String(need(typeof o.id === 'string' || typeof o.id === 'number', o.id, 'comment.id')),
    at: num(o, 'at'),
    author: parseAuthor(o.author),
    text: str(o, 'text'),
    mentions: mentions as string[],
    onlyNote: o.onlyNote === true,
    notify: parseNotify(o.notify),
  }
}

export interface CommentsOrigin { hubId: string; spaceId: string; projectId: string; taskId: string }

export function parseComments(v: unknown, origin: CommentsOrigin): { comments: TaskComment[]; omitted: number } {
  const o = need(isObj(v), v as Obj, 'answer')
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
  need(o.projectId === origin.projectId, null, 'projectId')
  need(o.taskId === origin.taskId, null, 'taskId')
  return { comments: arr(o, 'comments').map(parseComment), omitted: typeof o.omitted === 'number' ? o.omitted : 0 }
}

export function commentBody(origin: CommentsOrigin, text: string, mentions: readonly string[], onlyNote: boolean, clientRequestId: string) {
  return { ...origin, clientRequestId, text, mentions: [...mentions], onlyNote }
}

export function parseCommentAnswer(v: unknown, clientRequestId: string): TaskComment {
  const o = need(isObj(v), v as Obj, 'answer')
  const receipt = need(isObj(o.receipt), o.receipt as Obj, 'receipt')
  need(receipt.clientRequestId === clientRequestId, null, 'receipt.clientRequestId')
  return parseComment(o.comment)
}

/**
 * Rascunho do campo de comentário cujo envio ficou incerto (o mesmo pedido pode ser confirmado depois, pelo Tentar de
 * novo de qualquer faixa). `clear`: o Hub confirmou ESSE pedido e o texto no campo ainda é o enviado — some, para não
 * convidar a mandar de novo (outro pedido, outro comentário, outro aviso pago). `keep`: confirmado mas o dono já
 * escreveu outra coisa, ou o pedido acabou sem confirmação (descartado, recusado) — o texto fica. `wait`: em aberto.
 */
export function pendingDraft(sentText: string, text: string, confirmed: boolean, open: boolean): 'clear' | 'keep' | 'wait' {
  if (confirmed) return text.trim() === sentText ? 'clear' : 'keep'
  return open ? 'wait' : 'keep'
}

/** Nomes juntados como o idioma junta (“A, B e C”, “A、B和C”, “A وB”), cada um isolado (FSI…PDI) para a direção dele. */
export function joinNames(locale: string, names: readonly string[]): string {
  return new Intl.ListFormat(locale, { type: 'conjunction' }).format(names.map((n) => isolado(n)))
}

/**
 * Bots citados no texto (`@Nome`), pelo catálogo do Space; nome mais longo primeiro para “@Ana Clara” não virar “@Ana”.
 * É só a intenção do dono: o Hub confere identidade e audiência e decide quem é avisado.
 */
export function mentionsIn(text: string, bots: readonly ProjectBot[]): string[] {
  const sorted = [...bots].filter((b) => b.name.trim()).sort((a, b) => b.name.length - a.name.length)
  const found: string[] = []
  // Minúsculas Unicode padrão (sem locale), como o Hub (fork/mentions.rs): mesma regra, nenhuma recusa falsa.
  const lower = text.toLowerCase()
  for (let i = lower.indexOf('@'); i >= 0; i = lower.indexOf('@', i + 1)) {
    if (i > 0 && /[\p{L}\p{N}_]/u.test(lower[i - 1]!)) continue
    for (const b of sorted) {
      const name = b.name.toLowerCase()
      if (!lower.startsWith(name, i + 1)) continue
      const after = lower[i + 1 + name.length]
      if (after !== undefined && /[\p{L}\p{N}_]/u.test(after)) continue
      if (!found.includes(b.id)) found.push(b.id)
      break
    }
  }
  return found
}

/** O `@termo` sendo digitado logo antes do cursor (para o menu de @), ou null. */
export function mentionQuery(text: string, caret: number): { start: number; query: string } | null {
  // Cursor negativo: o menu foi fechado (Esc) até a próxima digitação.
  if (caret < 0) return null
  const before = text.slice(0, caret)
  const at = before.lastIndexOf('@')
  if (at < 0 || (at > 0 && /[\p{L}\p{N}_]/u.test(before[at - 1]!))) return null
  const query = before.slice(at + 1)
  if (query.length > 40 || /\n/.test(query)) return null
  return { start: at, query }
}

// MARK: Lista e Quadro

export interface TaskEntry {
  project: ProjectSummary
  task: ProjectTask
  meta: TaskMeta
  depth: number
}

export interface TaskFilter { projectId: string | null; botId: string | null; query: string }
export const NO_FILTER: TaskFilter = { projectId: null, botId: null, query: '' }
export const isFiltered = (f: TaskFilter) => !!(f.projectId || f.botId || f.query.trim())

function normal(s: string) {
  return s.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase()
}

export function matchesFilter(e: TaskEntry, f: TaskFilter): boolean {
  if (f.projectId && e.project.id !== f.projectId) return false
  if (f.botId && e.task.assigneeBotId !== f.botId) return false
  const q = normal(f.query.trim())
  if (q && !normal(`${e.task.title}\n${e.task.description}\n${e.project.name}`).includes(q)) return false
  return true
}

/** Todas as tarefas, em ordem de árvore (pai antes dos filhos), Projeto por Projeto. */
export function entriesOf(bundles: readonly TaskBundle[]): TaskEntry[] {
  const out: TaskEntry[] = []
  for (const b of bundles) {
    const kids = new Map<string | null, ProjectTask[]>()
    const ids = new Set(b.tasks.map((t) => t.id))
    for (const t of b.tasks) {
      // Pai que não existe mais vira raiz: nada some da lista.
      const parent = t.parentTaskId && ids.has(t.parentTaskId) ? t.parentTaskId : null
      kids.set(parent, [...(kids.get(parent) ?? []), t])
    }
    const seen = new Set<string>()
    const walk = (parent: string | null) => {
      for (const t of kids.get(parent) ?? []) {
        if (seen.has(t.id)) continue
        seen.add(t.id)
        out.push({ project: b.project, task: t, meta: metaOf(b.meta, t.id), depth: depthOf(b.tasks, t.id) - 1 })
        walk(t.id)
      }
    }
    walk(null)
    // Ciclo vindo do Hub: as tarefas presas nele aparecem como raiz, sem repetir.
    for (const t of b.tasks) if (!seen.has(t.id)) { seen.add(t.id); out.push({ project: b.project, task: t, meta: metaOf(b.meta, t.id), depth: 0 }) }
  }
  return out
}

/** Ordem dos grupos da Lista: o que anda primeiro, o encerrado por último (recolhido). */
export const LIST_ORDER: readonly ProjectTaskState[] = ['in_progress', 'review', 'backlog', 'blocked', 'done', 'cancelled']
export const CLOSED_STATES: readonly ProjectTaskState[] = ['done', 'cancelled']

const PRIORITY_RANK: Record<TaskPriority, number> = { urgent: 0, high: 1, normal: 2, low: 3 }
function byPriority(a: TaskEntry, b: TaskEntry) {
  const pa = a.meta.priority ? PRIORITY_RANK[a.meta.priority] : 4
  const pb = b.meta.priority ? PRIORITY_RANK[b.meta.priority] : 4
  if (pa !== pb) return pa - pb
  const da = a.meta.dueAt ?? Infinity
  const db = b.meta.dueAt ?? Infinity
  return da === db ? 0 : da < db ? -1 : 1
}

export interface ListGroup { state: ProjectTaskState; rows: TaskEntry[]; count: number }

/**
 * Grupos da Lista. Sem filtro: cada tarefa principal entra no grupo do próprio estado, com as subtarefas logo abaixo
 * (estado próprio de cada uma na linha). Com filtro: só as tarefas que batem, cada uma no grupo do seu estado, sem
 * recuo. “Esperando você” é um atalho (tarefas Em conferência, de qualquer nível), não um estado.
 */
export function listGroups(entries: readonly TaskEntry[], filter: TaskFilter): { waiting: TaskEntry[]; groups: ListGroup[] } {
  const filtered = isFiltered(filter)
  const waiting = entries.filter((e) => e.task.state === 'review' && matchesFilter(e, filter)).map((e) => ({ ...e, depth: 0 })).sort(byPriority)
  const groups: ListGroup[] = LIST_ORDER.map((state) => ({ state, rows: [], count: 0 }))
  const groupOf = new Map(groups.map((g) => [g.state, g]))
  if (filtered) {
    for (const e of entries) if (matchesFilter(e, filter)) { const g = groupOf.get(e.task.state)!; g.rows.push({ ...e, depth: 0 }); g.count++ }
    for (const g of groups) g.rows.sort(byPriority)
    return { waiting, groups }
  }
  const roots: TaskEntry[][] = []
  for (const e of entries) {
    if (e.depth === 0) roots.push([e])
    else roots[roots.length - 1]?.push(e)
  }
  roots.sort((a, b) => byPriority(a[0]!, b[0]!))
  for (const tree of roots) {
    const g = groupOf.get(tree[0]!.task.state)!
    g.rows.push(...tree)
    g.count++
  }
  return { waiting, groups }
}

/** Colunas do Quadro com o conjunto filtrado (mesma fonte da Lista). */
export function boardColumns(entries: readonly TaskEntry[], filter: TaskFilter): { state: ProjectTaskState; cards: TaskEntry[] }[] {
  return PROJECT_TASK_STATES.map((state) => ({ state, cards: entries.filter((e) => e.task.state === state && matchesFilter(e, filter)).sort(byPriority) }))
}

/** Subtarefas concluídas / total (todas as descendentes, sem as canceladas). */
export function progressOf(tasks: readonly ProjectTask[], rootId: string): { done: number; total: number } {
  let done = 0
  let total = 0
  const seen = new Set<string>([rootId])
  const stack = [rootId]
  while (stack.length) {
    const id = stack.pop()!
    for (const t of tasks) {
      if (t.parentTaskId !== id || seen.has(t.id)) continue
      seen.add(t.id)
      stack.push(t.id)
      if (t.state === 'cancelled') continue
      total++
      if (t.state === 'done') done++
    }
  }
  return { done, total }
}

/**
 * Concluída **declarada** (decisão 2): só pelo fato gravado no Hub (`declarations`), nunca pelo histórico — que não
 * vem na Lista paginada, tem janela e pode mudar depois. Devolve o bot que declarou; null sem declaração.
 */
export function declaredBy(declarations: Readonly<Record<string, TaskDeclaration>> | null | undefined, task: Pick<ProjectTask, 'id' | 'state'>): string | null {
  if (task.state !== 'done') return null
  return declarations?.[task.id]?.botId ?? null
}

/** A última mudança registrada da tarefa (quem e quando), pelo histórico do Hub. */
export function lastUpdateOf(history: readonly ProjectHistoryItem[] | null, taskId: string): ProjectHistoryItem | null {
  let last: ProjectHistoryItem | null = null
  for (const h of history ?? []) if (h.taskId === taskId && (!last || h.at > last.at)) last = h
  return last
}

/**
 * Projeto já escolhido na Nova tarefa: o último usado neste aparelho (se ainda ativo), senão o padrão do Hub
 * (“Geral”, só quando o Hub o anuncia), senão o primeiro ativo. Sem nenhum: null (a tela abre Novo projeto).
 */
export function pickProject(projects: readonly ProjectSummary[], lastUsed: string | null, defaultId: string | null): string | null {
  const active = projects.filter((p) => p.state === 'active')
  if (lastUsed && active.some((p) => p.id === lastUsed)) return lastUsed
  if (defaultId && active.some((p) => p.id === defaultId)) return defaultId
  return active[0]?.id ?? null
}

/** Valor de `<input type="datetime-local">` no fuso do aparelho, e de volta (vazio = null). */
export function toLocalInput(ms: number | null): string {
  if (ms === null) return ''
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`
}
export function fromLocalInput(v: string): number | null {
  if (!v) return null
  const ms = new Date(v).getTime()
  return Number.isFinite(ms) ? ms : null
}

/** Prazo vencido (e a tarefa ainda aberta). */
export function isOverdue(meta: TaskMeta, state: ProjectTaskState, now: number): boolean {
  return meta.dueAt !== null && meta.dueAt < now && !CLOSED_STATES.includes(state)
}

// MARK: iniciar, agenda e sessões vinculadas (Hub B, só com as capacidades)

/** Resultado de `donnaTaskStart`. `failed`: não começou agora; a agenda do Hub tenta de novo sozinha. */
export const START_STATES = ['started', 'already_running', 'refused', 'failed'] as const
export type StartState = (typeof START_STATES)[number]

export function parseStartAnswer(v: unknown, clientRequestId: string): { state: StartState; reason: string | null } {
  const o = need(isObj(v), v as Obj, 'answer')
  const receipt = need(isObj(o.receipt), o.receipt as Obj, 'receipt')
  need(receipt.clientRequestId === clientRequestId, null, 'receipt.clientRequestId')
  const start = need(isObj(o.start), o.start as Obj, 'start')
  const state = str(start, 'state')
  need((START_STATES as readonly string[]).includes(state), null, 'start.state')
  const reason = start.reason ?? null
  need(reason === null || typeof reason === 'string', null, 'start.reason')
  return { state: state as StartState, reason: reason as string | null }
}

/** `starting` é passagem interna do Hub (< 30 s): a tela mostra como agendado. Estado novo: `unknown`, nunca “começou”. */
export const AGENDA_STATES = ['scheduled', 'starting', 'started', 'refused', 'cancelled', 'failed'] as const
export type AgendaState = (typeof AGENDA_STATES)[number] | 'unknown'

export interface TaskAgenda {
  projectId: string | null
  taskId: string | null
  botId: string | null
  startAt: number
  state: AgendaState
  sessionId: string | null
  attempts: number
  lastError: string | null
}

export function parseAgenda(v: unknown): TaskAgenda {
  const o = need(isObj(v), v as Obj, 'agenda')
  const state = str(o, 'state')
  const opt = (k: string) => need(o[k] === null || o[k] === undefined || typeof o[k] === 'string', (o[k] ?? null) as string | null, `agenda.${k}`)
  return {
    projectId: opt('projectId'),
    taskId: opt('taskId'),
    botId: opt('botId'),
    startAt: num(o, 'startAt'),
    state: (AGENDA_STATES as readonly string[]).includes(state) ? state as AgendaState : 'unknown',
    sessionId: opt('sessionId'),
    attempts: typeof o.attempts === 'number' && Number.isFinite(o.attempts) ? o.attempts : 0,
    lastError: opt('lastError'),
  }
}

export function parseAgendaList(v: unknown, origin: { hubId: string; spaceId: string }): TaskAgenda[] {
  const o = need(isObj(v), v as Obj, 'answer')
  need(o.version === 1, null, 'version')
  need(o.hubId === origin.hubId, null, 'hubId')
  need(o.spaceId === origin.spaceId, null, 'spaceId')
  return arr(o, 'agendas').map(parseAgenda).filter((a) => a.projectId && a.taskId)
}

export function parseScheduleAnswer(v: unknown, clientRequestId: string): TaskAgenda | null {
  const o = need(isObj(v), v as Obj, 'answer')
  const receipt = need(isObj(o.receipt), o.receipt as Obj, 'receipt')
  need(receipt.clientRequestId === clientRequestId, null, 'receipt.clientRequestId')
  return o.agenda === null || o.agenda === undefined ? null : parseAgenda(o.agenda)
}

/** A agenda que ainda vai cutucar o bot (para a Lista e o detalhe). */
export const agendaPending = (a: TaskAgenda | null | undefined): boolean => !!a && (a.state === 'scheduled' || a.state === 'starting')

export const SESSION_STATES = ['idle', 'working', 'waiting_owner', 'done', 'not_opened'] as const
export type LinkedSessionState = (typeof SESSION_STATES)[number] | 'unknown'
export interface LinkedSession { botId: string; sessionId: string | null; title: string; state: LinkedSessionState; parentSessionId: string | null }

export function parseSessions(v: unknown): { sessions: LinkedSession[]; agenda: TaskAgenda | null } {
  const o = need(isObj(v), v as Obj, 'answer')
  const sessions = arr(o, 'sessions').map((x) => {
    const s = need(isObj(x), x as Obj, 'session')
    const state = str(s, 'state')
    const opt = (k: string) => need(s[k] === null || s[k] === undefined || typeof s[k] === 'string', (s[k] ?? null) as string | null, `session.${k}`)
    return { botId: str(s, 'botId'), sessionId: opt('sessionId'), title: typeof s.title === 'string' ? s.title : '', state: (SESSION_STATES as readonly string[]).includes(state) ? state as LinkedSessionState : 'unknown', parentSessionId: opt('parentSessionId') }
  })
  return { sessions, agenda: o.agenda === null || o.agenda === undefined ? null : parseAgenda(o.agenda) }
}
