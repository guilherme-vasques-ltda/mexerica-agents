import {
  createBody, newUuid, parseProject, parseProjectSnapshot, parseProjectsList, parseTask, parseWriteAnswer, saveBody,
  type ProjectDraft, type ProjectSnapshot, type ProjectsList,
} from '../../../../shared/fork-projetos.ts'
import {
  EMPTY_META, TASK_CAPABILITIES, TASK_LIMITS, bundleOf, mergeMeta, parseEnsureDefault, commentBody, metaBody, parseAgendaList, parseCommentAnswer, parseComments, parseMetaMap,
  parseScheduleAnswer, parseSessions, parseStartAnswer, parseTasksPage,
  type LinkedSession, type StartState, type TaskAgenda, type TaskBundle, type TaskCapability, type TaskComment, type TaskMeta,
} from '../../../../shared/fork-tarefas.ts'
import { Observable } from '../../../lib/observable.ts'
import type { TowerLink, TowerSource } from '../torre/torre-session.ts'

// D541 fatia 1 (docs/reference/redesenho/components/ProjetosKanban/README.md): Projetos manuais de um Hub + Space, em qualquer
// Space válido. Cada Space tem os seus Projetos e o seu catálogo de bots; nada passa de um Space para outro. Trocar de Space
// (como trocar de Hub ou de época) apaga a projeção e o pedido guardado, e resposta do Space anterior nunca pinta.
// Como o Mission Board (torre-session.ts): assina o stream do space ANTES de ler; `donnaProjectsChanged` só invalida
// (nunca é fonte), avisos seguidos viram uma releitura; toda resposta é conferida contra a origem (Hub, link, geração)
// antes de pintar. Sem link: última fotografia em memória, marcada como antiga, e nenhuma mudança aceita. 403 explícito ou
// outro Hub apagam tudo. Erro de autoridade ambíguo do Hub (`not_allowed`, HTTP 400: autorização original encerrada,
// inclusive na entrada dispatch) NÃO prova revogação: a fotografia da MESMA origem (Hub + Space + época) fica, marcada
// como antiga, e nada grava até uma leitura autorizada nova; gravação que bate nele não é reenviada sozinha. Nada vai para disco nem para o mirror: o Hub é a única fonte. Uma gravação por vez; a tela só muda
// com a resposta do Hub. Falha incerta guarda o MESMO pedido (chave e corpo) para Tentar de novo; conflito relê e
// não reenvia nada sozinho.

export const PROJECTS_EVENT = 'donnaProjectsChanged'
const REREAD_DELAY = 250
const RELISTEN_MIN = 2_000
const RELISTEN_MAX = 30_000

export type ProjectsFailure = 'offline' | 'unavailable' | 'lostAccess' | 'authority' | 'invalid' | 'failed'

export type ProjectsPhase =
  | { kind: 'loading' }
  | { kind: 'ready' }
  | { kind: 'error'; reason: ProjectsFailure }
  | { kind: 'moved' }

export type ProjectView =
  | { kind: 'none' }
  | { kind: 'loading'; projectId: string }
  | { kind: 'ready'; projectId: string }
  | { kind: 'error'; projectId: string; reason: ProjectsFailure }

export type WriteStatus = 'sending' | 'uncertain' | 'conflict' | 'rejected'
/** MX-142: `meta` grava prioridade/prazo/fases (`donnaTaskMetaSave`); `comment`, um comentário do dono. */
export type WriteOp = 'create' | 'save' | 'meta' | 'comment' | 'start' | 'schedule' | 'ensureDefault'
/**
 * TEC-722-16: as RPCs que devolvem o snapshot do Projeto recebem `historyAuthors: true` para o Hub mandar o histórico
 * com os autores reais (`owner`, `system`, `bot:<id>`); sem a flag ele projeta só o dono, para leitores antigos.
 */
const SNAPSHOT_OPS: ReadonlySet<WriteOp> = new Set(['create', 'save', 'meta', 'ensureDefault'])
/**
 * Recusa do Hub anterior ao TEC-722-16 ao ver o campo: o `parse` de `projetos/store.rs` (serde com
 * `deny_unknown_fields`) devolve HTTP 400 com esta frase genérica, SEM nome de campo, antes de qualquer efeito
 * (nada lido nem gravado). Hub que cite o campo pelo nome também conta.
 */
const LEGACY_PARSE_REFUSAL = /^\s*invalid_field: solicitação de Projetos inválida\.?\s*$/
export function legacyHistoryRefusal(e: unknown): boolean {
  if (statusOf(e) !== 400) return false
  const message = messageOf(e)
  return LEGACY_PARSE_REFUSAL.test(message) || /\bhistoryAuthors\b/.test(message)
}
const WRITE_METHOD: Record<WriteOp, string> = {
  create: 'donnaProjectCreate', save: 'donnaProjectSave', meta: 'donnaTaskMetaSave', comment: 'donnaTaskComment',
  start: 'donnaTaskStart', schedule: 'donnaTaskSchedule', ensureDefault: 'donnaProjectEnsureDefault',
}
export interface WriteState {
  op: WriteOp
  /** Só para o rótulo e para saber de qual Projeto é. */
  projectId: string | null
  clientRequestId: string
  /** O corpo exato enviado; Tentar de novo reenvia este, nunca um novo. */
  body: Record<string, unknown>
  status: WriteStatus
  /** Frase do Hub numa recusa (texto, nunca HTML). */
  detail: string | null
  /**
   * Criação em 2 passos (tarefa, depois metadados): `created` = a tarefa já foi gravada e só o 2º passo falhou. A tela
   * diz isso (sucesso parcial) e Tentar de novo refaz só os metadados.
   */
  partial?: 'created' | 'saved'
  /** Os metadados pedidos (op `meta`): refazer depois de conflito/recusa relê o Projeto e envia de novo. */
  /** `base`: o meta que o dono viu; se o do snapshot relido for outro (mudou lá), nada é enviado. */
  meta?: { projectId: string; taskId: string; meta: TaskMeta; base?: TaskMeta }
  /** Iniciar respondido sem começar (`refused`: recusado; `failed`: a agenda do Hub tenta de novo sozinha). */
  start?: StartState
}

/** Sessões vinculadas e agenda da tarefa aberta (só com `taskSessions`). */
export interface LinkedView {
  projectId: string
  taskId: string
  state: 'loading' | 'ready' | 'error'
  sessions: LinkedSession[]
  agenda: TaskAgenda | null
  attention: 'owner' | null
  reason: ProjectsFailure | null
}

/** Comentários da tarefa aberta (só com `taskComments`). */
export interface CommentsView {
  projectId: string
  taskId: string
  state: 'loading' | 'ready' | 'error'
  list: TaskComment[]
  omitted: number
  reason: ProjectsFailure | null
}

/**
 * `outdated`: o rascunho foi montado sobre uma fotografia que já não é a atual; nada foi enviado. `requestId` (só no
 * incerto): o pedido que ficou para Tentar de novo, para quem enviou saber quando ESSE pedido for confirmado.
 */
export type WriteResult = { ok: true; projectId: string } | { ok: false; status: Exclude<WriteStatus, 'sending'> | 'blocked' | 'outdated'; requestId?: string }

interface Origin { source: TowerSource; hostId: string; client: TowerLink; epoch: number; gen: number }
interface TowerEventWithTask {
  type: string
  spaceId?: string
  hubId?: string
  id?: string
  bot?: { id?: string; deleted?: boolean; hidden?: boolean; spaceId?: string }
  conversation?: { botId?: string; closed?: boolean }
  session?: { id?: string; state?: string; origin?: { kind?: string; taskId?: string } }
  sessionId?: string
  entry?: { kind?: string; botId?: string; sessionId?: string; data?: { card?: string } }
}
/**
 * Uma gravação em voo: só ela pode mexer em `write` quando voltar, e só na mesma autoridade (fonte, Hub e Space). A
 * época (`principalEpoch`) NÃO é identidade: no app ela sobe a cada hello, inclusive na reconexão do mesmo Hub; resposta
 * de outra época/link vira incerta (o mesmo pedido fica para Tentar de novo), nunca apaga nada.
 */
interface WriteTicket { seq: number; source: TowerSource; hostId: string; spaceId: string }

function statusOf(e: unknown): number | null {
  return typeof e === 'object' && e && 'status' in e ? Number((e as { status: unknown }).status) : null
}
function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e ? String((e as { message: unknown }).message) : String(e)
}

/** `not_allowed` fora de 401/403: autoridade em dúvida (ex.: autorização original encerrada), não revogação provada. */
function ambiguousAuthority(status: number | null, message: string) {
  return status !== 401 && status !== 403 && /^\s*not_allowed\b/i.test(message)
}

/** Falha de leitura, pelo status do Hub (`api::error_status`). */
export function readFailure(e: unknown): ProjectsFailure {
  const status = statusOf(e)
  const message = messageOf(e)
  if (status === 404 || /unknown method/i.test(message)) return 'unavailable'
  if (status === 403 || status === 401) return 'lostAccess'
  if (ambiguousAuthority(status, message)) return 'authority'
  if (/projects_invalid_answer/.test(message)) return 'invalid'
  if (status === 0) return 'offline'
  return 'failed'
}

/** Falha de gravação: só “incerto” quando o pedido pode ter chegado (link caiu, tempo esgotado). */
export function writeFailure(e: unknown): Exclude<WriteStatus, 'sending'> | 'lostAccess' | 'unavailable' | 'authority' {
  const status = statusOf(e)
  const message = messageOf(e)
  if (status === 403 || status === 401) return 'lostAccess'
  if (ambiguousAuthority(status, message)) return 'authority'
  if (status === 404 || /unknown method/i.test(message)) return 'unavailable'
  if (status === 409 || /conflict|revision|revis[aã]o/i.test(message)) return 'conflict'
  if (status === 0 || status === null || /timed? ?out|timeout|projects_invalid_answer/i.test(message)) return 'uncertain'
  return 'rejected'
}

/** A frase do Hub sem o código técnico (`projects_limit: …` → `…`). */
function hubDetail(e: unknown): string | null {
  const m = messageOf(e).trim()
  if (!m) return null
  // Qualquer código técnico na frente (`projects_limit:`, `invalid_field:`, `task_forbidden:` …) sai; fica a frase pt-BR.
  return m.replace(/^[a-z][a-z_]*:\s*/i, '').slice(0, 300) || null
}

export class ProjectsSession extends Observable {
  phase: ProjectsPhase = { kind: 'loading' }
  link: 'live' | 'offline' = 'live'
  events: 'live' | 'paused' = 'live'
  list: ProjectsList | null = null
  view: ProjectView = { kind: 'none' }
  project: ProjectSnapshot | null = null
  /** Quando a fotografia deixou de ser atualizável. */
  offlineSince: number | null = null
  /** Quando a última leitura boa chegou. */
  readAt: number | null = null
  /** Releitura falhou com dados já na tela: eles ficam, com aviso. */
  rereadFailed = false
  write: WriteState | null = null
  /** `pending`: o Hub pôs a autoridade em dúvida; a fotografia fica, antiga, até uma leitura autorizada nova. */
  authority: 'ok' | 'pending' = 'ok'
  /** MX-142: as tarefas de todos os Projetos ativos do Space (Lista e Quadro), só quando a tela as pede. */
  tasks: TaskBundle[] | null = null
  /** Falha da leitura das tarefas sem nada na tela ainda. */
  tasksError: ProjectsFailure | null = null
  /** Metadados do Projeto aberto, quando o `donnaProject` os traz (senão valem os da lista). */
  projectMeta: Record<string, TaskMeta> | null = null
  comments: CommentsView | null = null
  linked: LinkedView | null = null
  /** A agenda de início de todas as tarefas do Space (`taskAgenda`), para a Lista. */
  agendas: TaskAgenda[] | null = null
  /** Comentários confirmados pelo Hub nesta sessão (`clientRequestId` do recibo), inclusive por um Tentar de novo. */
  private confirmedComments = new Set<string>()

  private origin: Origin | null = null
  /** A fonte (o BotStore de um contexto/conta + computador): outra fonte é outra identidade e apaga tudo. */
  private sourceRef: TowerSource | null = null
  private sourceGen = 0
  private hostId: string | null = null
  /** Leitura autorizada feita NESTA origem; antes dela, a tela antiga fica só leitura. */
  private verified = false
  private writeSeq = 0
  private writeOp: WriteTicket | null = null
  private spaceId: string | null = null
  private gen = 0
  private reads = 0
  /** A origem da leitura em voo; leitura de outra origem (outro Space, link ou época) não bloqueia a nova. */
  private reading: Origin | null = null
  private dirty = false
  private unwatch: (() => void) | null = null
  private stopEvents: (() => void) | null = null
  private rereadTimer: ReturnType<typeof setTimeout> | null = null
  private linkedTimer: ReturnType<typeof setTimeout> | null = null
  private relistenTimer: ReturnType<typeof setTimeout> | null = null
  private relistenDelay = RELISTEN_MIN
  private now: () => number
  private uuid: () => string
  private wantTasks = false
  /** Releitura do Projeto antes de gravar em curso: nenhuma outra gravação começa nesse meio-tempo. */
  private preparing = false
  private commentsReads = 0
  /**
   * Hub anterior ao TEC-722-16 recusa o campo desconhecido `historyAuthors` (nada foi aplicado): a mesma chamada sai
   * de novo sem ele e o resto da conexão segue sem a flag. Volta a tentar a cada conexão nova.
   */
  private legacyHistory = false
  private linkedReads = 0

  constructor(now: () => number = Date.now, uuid: () => string = newUuid) {
    super()
    this.now = now
    this.uuid = uuid
  }

  /**
   * A fotografia só vale como “agora” com link e eventos vivos E depois da leitura autorizada desta origem (link novo,
   * época nova do hello ou dúvida de autoridade: continua antiga até ela); antiga, nada muda.
   */
  get stale() { return this.link === 'offline' || this.events === 'paused' || this.authority === 'pending' || !this.verified }
  /** Link e eventos vivos, só falta a leitura autorizada desta origem (a faixa diz “Conferindo…”, não “Sem conexão”). */
  get confirming() { return this.link === 'live' && this.events === 'live' && this.authority === 'ok' && !this.verified }
  /** Muda quando a fonte (outra conta/contexto) muda: a tela remonta e nenhum rascunho passa de uma fonte para outra. */
  get identity() { return this.sourceGen }
  get canWrite() {
    return this.verified && !this.stale && this.phase.kind === 'ready' && !!this.list?.capable && !this.preparing && this.write?.status !== 'sending' && this.write?.status !== 'uncertain'
  }

  /** O Hub anunciou a capacidade (fail-closed: sem lista ou sem anúncio, não). */
  has(cap: TaskCapability) { return !!this.list?.capabilities.includes(cap) }

  /** Metadados de uma tarefa: os do Projeto aberto (mais novos, se vieram) ou os da lista. */
  metaOf(projectId: string, taskId: string): TaskMeta | null {
    if (this.projectMeta && this.project?.project.id === projectId && this.projectMeta[taskId]) return this.projectMeta[taskId]!
    return this.tasks?.find((b) => b.project.id === projectId)?.meta[taskId] ?? null
  }

  /** A tela de Tarefas liga a leitura de todas as tarefas do Space (Lista e Quadro). */
  watchTasks(on: boolean) {
    if (this.wantTasks === on) return
    this.wantTasks = on
    if (!on) {
      this.tasks = null
      this.tasksError = null
      this.changed()
    } else this.reread()
  }

  /** Sessões vinculadas e agenda da tarefa aberta, só com `taskSessions` (sem o anúncio, nada é pedido). */
  private openLinked(target: { projectId: string; taskId: string } | null) {
    if (this.linkedTimer) clearTimeout(this.linkedTimer)
    this.linkedTimer = null
    this.linkedReads++
    if (!target || !this.has(TASK_CAPABILITIES.sessions)) {
      this.linked = null
      return
    }
    if (this.linked?.projectId !== target.projectId || this.linked.taskId !== target.taskId) {
      this.linked = { ...target, state: 'loading', sessions: [], agenda: null, attention: null, reason: null }
    }
    const origin = this.origin
    if (origin && this.current(origin)) void this.readLinked(origin)
  }

  /** A agenda de início de uma tarefa: a do detalhe (mais nova) ou a da lista. */
  agendaOf(projectId: string, taskId: string): TaskAgenda | null {
    if (this.linked?.projectId === projectId && this.linked.taskId === taskId && this.linked.state === 'ready') return this.linked.agenda
    return this.agendas?.find((a) => a.projectId === projectId && a.taskId === taskId) ?? null
  }

  needsOwnerAttention(projectId: string, taskId: string): boolean {
    const l = this.linked
    return !this.stale && l?.state === 'ready' && l.projectId === projectId && l.taskId === taskId && l.attention === 'owner'
  }

  /** Abre (ou fecha, com null) os comentários de uma tarefa do Projeto aberto. */
  openComments(target: { projectId: string; taskId: string } | null) {
    this.openLinked(target)
    if (!target) {
      this.comments = null
      this.commentsReads++
      this.changed()
      return
    }
    if (this.comments?.projectId !== target.projectId || this.comments.taskId !== target.taskId) {
      this.comments = { ...target, state: 'loading', list: [], omitted: 0, reason: null }
      this.commentsReads++
    }
    this.changed()
    const origin = this.origin
    if (origin && this.current(origin)) void this.readComments(origin)
  }


  /** O Space desta sessão; a tela não pinta nada de uma sessão ainda presa a outro Space. */
  get space() { return this.spaceId }
  /** O Hub da última origem; com o Space, forma a chave da tela (rascunhos de outro Hub/Space não sobrevivem). */
  get hub() { return this.hostId }

  open(source: TowerSource, spaceId: string) {
    this.release()
    if (this.sourceRef !== source) this.sourceGen++
    if (this.sourceRef && this.sourceRef !== source) {
      // Outra conta/contexto (outro BotStore) no mesmo computador: nada da fonte anterior continua nem pinta.
      this.gen++
      this.origin = null
      this.forget()
      this.hostId = null
      this.view = { kind: 'none' }
    }
    this.sourceRef = source
    if (this.spaceId !== spaceId) {
      // Outro Space é outro conjunto de Projetos: nada do anterior (lista, Projeto aberto, pedido incerto) continua,
      // e o que ainda estiver em voo dele fica sem origem para pintar.
      this.gen++
      this.origin = null
      this.forget()
      this.view = { kind: 'none' }
    }
    this.spaceId = spaceId
    this.unwatch = source.subscribe(() => this.watch(source))
    this.connect(source)
  }

  close() {
    this.release()
    this.gen++
    this.origin = null
    this.verified = false
    // Fechar a tela abandona a gravação em voo: a resposta dela não volta a pintar nada nem guarda o pedido.
    this.dropWrite()
    this.changed()
  }

  /** Abre um Projeto (ou volta à lista com null); a leitura é da origem atual. */
  openProject(projectId: string | null) {
    if (!projectId) {
      this.view = { kind: 'none' }
      this.project = null
    } else if (this.view.kind === 'none' || this.view.projectId !== projectId) {
      this.view = { kind: 'loading', projectId }
      this.project = null
    }
    this.changed()
    this.reread()
  }

  reread() {
    const origin = this.origin
    if (origin && this.current(origin)) void this.read(origin)
  }

  /** Descarta o aviso de gravação (incerto: desiste de confirmar e relê). */
  dismissWrite() {
    if (this.write?.status === 'sending') return
    this.dropWrite()
    this.changed()
    this.reread()
  }

  create(fields: { name: string; objective: string; coordinatorBotId: string | null }): Promise<WriteResult> {
    const origin = this.origin
    if (!origin || !this.spaceId || !this.canWrite) return Promise.resolve({ ok: false, status: 'blocked' })
    const id = this.uuid()
    const body = createBody({ hubId: origin.hostId, spaceId: this.spaceId }, fields, id)
    return this.send({ op: 'create', projectId: null, clientRequestId: id, body, status: 'sending', detail: null })
  }

  /**
   * `basis`: a fotografia sobre a qual o rascunho foi montado. Precisa ser a atual (mesmo objeto): uma releitura que
   * chegou depois e ainda não foi desenhada não pode receber conteúdo velho com a revisão nova. Assim a revisão
   * esperada é sempre a do conteúdo que o dono viu, e o CAS do Hub fecha a corrida que sobrar.
   */
  save(draft: ProjectDraft, basis: ProjectSnapshot): Promise<WriteResult> {
    const s = this.project
    if (!s || s.spaceId !== this.spaceId || !this.canWrite || this.view.kind !== 'ready') return Promise.resolve({ ok: false, status: 'blocked' })
    if (basis !== s) return Promise.resolve({ ok: false, status: 'outdated' })
    const id = this.uuid()
    return this.send({ op: 'save', projectId: s.project.id, clientRequestId: id, body: saveBody(s, draft, id), status: 'sending', detail: null })
  }

  /** Tentar de novo: o mesmo `clientRequestId` e o mesmo corpo; o Hub devolve o mesmo recibo se já gravou. */
  retry(): Promise<WriteResult> {
    const w = this.write
    if (!w || w.status !== 'uncertain' || this.stale || !this.origin || !this.verified) return Promise.resolve({ ok: false, status: 'blocked' })
    return this.send({ ...w, status: 'sending', detail: null })
  }

  /**
   * Gravar a partir da Lista, do Quadro ou do detalhe: SEMPRE relê o Projeto inteiro (`donnaProject`) e monta a mudança
   * sobre ele, com a revisão dele (CAS por Projeto). `build` devolve null para desistir e `'outdated'` quando a tarefa
   * mudou em outro lugar de um jeito que só o dono resolve (nada é enviado).
   */
  async saveFresh(projectId: string, build: (snap: ProjectSnapshot) => ProjectDraft | 'outdated' | null): Promise<WriteResult> {
    const prepared = await this.prepare(projectId)
    if (!prepared) return { ok: false, status: 'blocked' }
    const snap = prepared.snap
    const draft = build(snap)
    if (draft === null) return { ok: false, status: 'blocked' }
    if (draft === 'outdated') return { ok: false, status: 'outdated' }
    const id = this.uuid()
    return this.send({ op: 'save', projectId, clientRequestId: id, body: saveBody(snap, draft, id), status: 'sending', detail: null })
  }

  /** Prioridade, prazo e fases (`taskMeta`), sobre a revisão relida do Projeto. */
  /**
   * `base`: o meta sobre o qual o dono editou. O meta conferido é o do MESMO snapshot relido cuja revisão vai no CAS
   * (nunca o da lista `donnaTasks` com a revisão de outra leitura): se ele mudou em outro lugar, nada é enviado e o
   * aviso é de conflito; Tentar de novo manda os mesmos valores sobre a versão que o dono acabou de ver.
   */
  async saveMeta(projectId: string, taskId: string, meta: TaskMeta, partial?: 'created' | 'saved', base?: TaskMeta): Promise<WriteResult> {
    if (!this.has(TASK_CAPABILITIES.meta)) return { ok: false, status: 'blocked' }
    const prepared = await this.prepare(projectId, partial)
    if (!prepared) return { ok: false, status: 'blocked' }
    const { snap, meta: fresh } = prepared
    if (!snap.tasks.some((t) => t.id === taskId)) return { ok: false, status: 'rejected' }
    if (base) {
      // Sem o meta no snapshot não dá para conferir a mesma revisão: não grava às cegas.
      if (!fresh) {
        this.write = { op: 'meta', projectId, clientRequestId: '', body: {}, status: 'rejected', detail: null, ...(partial ? { partial } : {}) }
        this.changed()
        return { ok: false, status: 'rejected' }
      }
      const now = fresh[taskId] ?? EMPTY_META
      const merged = mergeMeta(base, meta, now)
      if (merged.conflict) {
        // Para Tentar de novo: os campos do dono sobre a versão que ele acabou de ver (nunca os valores velhos dos outros).
        this.write = { op: 'meta', projectId, clientRequestId: '', body: {}, status: 'conflict', detail: null, meta: { projectId, taskId, meta: merged.meta, base: now }, ...(partial ? { partial } : {}) }
        this.changed()
        return { ok: false, status: 'conflict' }
      }
      // Só os campos que o dono mudou; o resto é o da resposta relida (mesma revisão do CAS).
      meta = merged.meta
      base = now
    }
    const id = this.uuid()
    return this.send({ op: 'meta', projectId, clientRequestId: id, body: metaBody(snap, taskId, meta, id), status: 'sending', detail: null, meta: { projectId, taskId, meta, ...(base ? { base } : {}) }, ...(partial ? { partial } : {}) })
  }

  /**
   * Nova tarefa (e, se pedidos, os metadados) em 2 passos: o Hub não grava os dois juntos. Se só o 2º falhar, a
   * tarefa fica criada e a tela mostra o sucesso parcial, com Tentar de novo só para os metadados.
   */
  async createTask(projectId: string, build: (snap: ProjectSnapshot) => ProjectDraft | null, taskId: string, meta: TaskMeta | null, start = false): Promise<WriteResult> {
    const r = await this.saveFresh(projectId, build)
    if (!r.ok) return r
    if (meta && this.has(TASK_CAPABILITIES.meta)) {
      // Tarefa recém-criada: a base é o meta vazio.
      const m = await this.saveMeta(projectId, taskId, meta, 'created', EMPTY_META)
      if (!m.ok) return { ok: false, status: m.status === 'blocked' && this.write?.partial ? 'rejected' : m.status }
    }
    // “Criar e já iniciar” (decisão 5): só com `taskStart`; atribuir sozinho nunca dispara.
    if (start && this.has(TASK_CAPABILITIES.start)) return this.startTask(projectId, taskId, 'created')
    return r
  }

  /**
   * Projeto Geral (`defaultProject`): o Hub cria ou adota o Geral de forma idempotente (sem chave de pedido; repetir
   * devolve o mesmo). Usado quando a Nova tarefa não tem nenhum Projeto para escolher (decisão 7).
   */
  ensureDefaultProject(): Promise<WriteResult> {
    const origin = this.origin
    if (!origin || !this.spaceId || !this.canWrite || !this.has(TASK_CAPABILITIES.defaultProject)) return Promise.resolve({ ok: false, status: 'blocked' })
    const body = { hubId: origin.hostId, spaceId: this.spaceId }
    return this.send({ op: 'ensureDefault', projectId: null, clientRequestId: '', body, status: 'sending', detail: null })
  }

  /** Iniciar agora (`taskStart`). Incerto: Tentar de novo reenvia o MESMO pedido (o Hub não duplica). */
  startTask(projectId: string, taskId: string, partial?: 'created' | 'saved'): Promise<WriteResult> {
    const origin = this.origin
    if (!origin || !this.spaceId || !this.canWrite || !this.has(TASK_CAPABILITIES.start)) return Promise.resolve({ ok: false, status: 'blocked' })
    const id = this.uuid()
    const body = { hubId: origin.hostId, spaceId: this.spaceId, projectId, taskId, clientRequestId: id }
    return this.send({ op: 'start', projectId, clientRequestId: id, body, status: 'sending', detail: null, ...(partial ? { partial } : {}) })
  }

  /** Agendar (ms) ou limpar (null) a hora em que o bot é cutucado para começar (`taskSchedule`). */
  scheduleTask(projectId: string, taskId: string, startAt: number | null, partial?: 'created' | 'saved'): Promise<WriteResult> {
    const origin = this.origin
    if (!origin || !this.spaceId || !this.canWrite || !this.has(TASK_CAPABILITIES.schedule)) return Promise.resolve({ ok: false, status: 'blocked' })
    const id = this.uuid()
    const body = { hubId: origin.hostId, spaceId: this.spaceId, projectId, taskId, clientRequestId: id, startAt }
    return this.send({ op: 'schedule', projectId, clientRequestId: id, body, status: 'sending', detail: null, ...(partial ? { partial } : {}) })
  }

  /** O Hub confirmou o comentário deste pedido (o campo limpa o rascunho só com isso; ver `pendingDraft`). */
  commentConfirmed(clientRequestId: string) { return this.confirmedComments.has(clientRequestId) }

  /** Comentário do dono na tarefa aberta (`taskComments`). As menções são só a intenção: o Hub decide quem avisa. */
  /**
   * `target` é a tarefa em que o texto foi escrito: se a tarefa aberta agora for outra (o dono trocou de tarefa com o
   * rascunho no campo), nada é enviado. Um comentário nunca cai na tarefa errada.
   */
  sendComment(text: string, mentions: readonly string[], onlyNote: boolean, target: { projectId: string; taskId: string }): Promise<WriteResult> {
    const c = this.comments
    const origin = this.origin
    if (!origin || !this.spaceId || !c || !this.canWrite || !this.has(TASK_CAPABILITIES.comments)) return Promise.resolve({ ok: false, status: 'blocked' })
    if (c.projectId !== target.projectId || c.taskId !== target.taskId) return Promise.resolve({ ok: false, status: 'blocked' })
    const id = this.uuid()
    const body = commentBody({ hubId: origin.hostId, spaceId: this.spaceId, projectId: c.projectId, taskId: c.taskId }, text, mentions, onlyNote, id)
    return this.send({ op: 'comment', projectId: c.projectId, clientRequestId: id, body, status: 'sending', detail: null })
  }

  /**
   * Tentar de novo de um pedido de metadados: incerto reenvia o MESMO pedido (o Hub devolve o mesmo recibo se já
   * gravou); conflito ou recusa relê o Projeto e envia de novo os mesmos metadados (o anterior com certeza não gravou).
   */
  retryMeta(): Promise<WriteResult> {
    const w = this.write
    if (!w?.meta) return Promise.resolve({ ok: false, status: 'blocked' })
    if (w.status === 'uncertain') return this.retry()
    if (w.status !== 'conflict' && w.status !== 'rejected') return Promise.resolve({ ok: false, status: 'blocked' })
    const { projectId, taskId, meta, base } = w.meta
    const partial = w.partial
    this.dropWrite()
    return this.saveMeta(projectId, taskId, meta, partial, base)
  }

  /** Relê o Projeto inteiro antes de gravar; nenhuma outra gravação começa até terminar. */
  private async prepare(projectId: string, partial?: 'created' | 'saved'): Promise<{ snap: ProjectSnapshot; meta: Record<string, TaskMeta> | null } | null> {
    const origin = this.origin
    const spaceId = this.spaceId
    if (!origin || !spaceId || !this.canWrite) return null
    this.preparing = true
    this.changed()
    try {
      const answer = await this.callSnapshot(origin.client, 'donnaProject', { hubId: origin.hostId, spaceId, projectId })
      if (!this.current(origin) || this.spaceId !== spaceId) return null
      const snap = parseProjectSnapshot(answer, { hubId: origin.hostId, spaceId, projectId })
      if (snap.project.state !== 'active') {
        this.write = { op: 'save', projectId, clientRequestId: '', body: {}, status: 'rejected', detail: null, ...(partial ? { partial } : {}) }
        return null
      }
      const raw = (answer as { meta?: unknown }).meta
      const meta = raw === undefined ? null : parseMetaMap(raw)
      if (meta) this.freshMeta(projectId, meta)
      return { snap, meta }
    } catch (e) {
      if (!this.current(origin) || this.spaceId !== spaceId) return null
      const reason = readFailure(e)
      if (reason === 'lostAccess') { this.lostAccess(); return null }
      if (reason === 'authority') { this.authorityInDoubt(); return null }
      // A releitura falhou: nada foi enviado. A tela diz que não salvou (sem Tentar de novo automático).
      this.write = { op: 'save', projectId, clientRequestId: '', body: {}, status: 'rejected', detail: hubDetail(e), ...(partial ? { partial } : {}) }
      return null
    } finally {
      this.preparing = false
      this.changed()
    }
  }

  /** Meta relido de um Projeto (mesmo snapshot da revisão): atualiza o detalhe aberto e a lista. */
  private freshMeta(projectId: string, meta: Record<string, TaskMeta>) {
    if (this.project?.project.id === projectId) this.projectMeta = meta
    if (this.tasks) this.tasks = this.tasks.map((b) => (b.project.id === projectId ? { ...b, meta } : b))
  }

  private dropWrite() {
    this.writeSeq++
    this.writeOp = null
    this.write = null
  }

  /** A operação ainda é a atual e a autoridade (Hub, época, space) é a mesma de quando saiu. */
  private sameAuthority(op: WriteTicket) {
    return this.writeOp === op && op.seq === this.writeSeq && this.sourceRef === op.source && this.hostId === op.hostId && this.spaceId === op.spaceId
  }

  /** Chamada que devolve snapshot: com `historyAuthors`, salvo Hub antigo (recusa explícita do campo → sem ele). */
  private async callSnapshot(client: TowerLink, method: string, body: Record<string, unknown>): Promise<unknown> {
    if (this.legacyHistory) return client.call(method, body)
    try {
      return await client.call(method, { ...body, historyAuthors: true })
    } catch (e) {
      // Só a recusa de parse (antes de qualquer efeito); qualquer outro erro — inclusive outro invalid_field, queda
      // de link ou tempo esgotado — segue o caminho normal, sem reenvio.
      if (!legacyHistoryRefusal(e)) throw e
      this.legacyHistory = true
      return client.call(method, body)
    }
  }

  private async send(w: WriteState): Promise<WriteResult> {
    const origin = this.origin!
    const spaceId = this.spaceId!
    const op: WriteTicket = { seq: ++this.writeSeq, source: origin.source, hostId: origin.hostId, spaceId }
    this.writeOp = op
    this.write = w
    this.changed()
    const method = WRITE_METHOD[w.op]
    try {
      const answer = SNAPSHOT_OPS.has(w.op) ? await this.callSnapshot(origin.client, method, w.body) : await origin.client.call(method, w.body)
      // 403, outra fonte, outro Hub/Space, close ou Descartar no meio: a resposta é descartada sem ressuscitar nada.
      if (!this.sameAuthority(op)) return { ok: false, status: 'blocked' }
      if (!this.current(origin)) {
        // Mesma autoridade, link ou época trocados (queda e volta): não pinta; o mesmo pedido fica para Tentar de novo.
        this.write = { ...w, status: 'uncertain' }
        this.changed()
        return { ok: false, status: 'uncertain', requestId: w.clientRequestId }
      }
      if (w.op === 'ensureDefault') {
        const { snapshot } = parseEnsureDefault(answer, { hubId: origin.hostId, spaceId })
        this.writeOp = null
        this.write = null
        if (this.tasks && !this.tasks.some((b) => b.project.id === snapshot.project.id)) {
          const raw = (answer as { snapshot?: { meta?: unknown } }).snapshot?.meta
          this.tasks = [...this.tasks, bundleOf(snapshot, raw === undefined ? {} : parseMetaMap(raw))]
        }
        this.changed()
        // A lista (e o defaultProjectId) vem do Hub, não deste snapshot.
        this.scheduleRead(origin)
        return { ok: true, projectId: snapshot.project.id }
      }
      if (w.op === 'start' || w.op === 'schedule') {
        const body = w.body as { projectId: string; taskId: string }
        // Valida ANTES de soltar a operação (TEC-722-13): resposta fora do formato cai no catch ainda com a mesma
        // autoridade e vira “incerto” (o mesmo pedido fica para Tentar de novo), nunca uma sessão presa em sending.
        const started = w.op === 'start' ? parseStartAnswer(answer, w.clientRequestId) : null
        const agenda = w.op === 'schedule' ? parseScheduleAnswer(answer, w.clientRequestId) : null
        this.writeOp = null
        let result: WriteResult = { ok: true, projectId: body.projectId }
        if (started) {
          // Respondido sem começar não é falha de envio: a tela diz o que o Hub disse (e nada é reenviado sozinho).
          this.write = started.state === 'refused' || started.state === 'failed' ? { ...w, status: 'rejected', detail: started.reason, start: started.state } : null
          if (this.write) result = { ok: false, status: 'rejected' }
        } else {
          this.write = null
          const rest = (this.agendas ?? []).filter((a) => !(a.projectId === body.projectId && a.taskId === body.taskId))
          if (this.agendas) this.agendas = agenda ? [...rest, { ...agenda, projectId: body.projectId, taskId: body.taskId }] : rest
          if (this.linked?.projectId === body.projectId && this.linked.taskId === body.taskId) this.linked = { ...this.linked, agenda }
        }
        this.changed()
        this.scheduleRead(origin)
        if (this.linked) void this.readLinked(origin)
        return result
      }
      if (w.op === 'comment') {
        const comment = parseCommentAnswer(answer, w.clientRequestId)
        this.writeOp = null
        this.write = null
        this.confirmedComments.add(w.clientRequestId)
        const c = this.comments
        const body = w.body as { projectId?: string; taskId?: string }
        if (c && c.projectId === body.projectId && c.taskId === body.taskId && !c.list.some((x) => x.id === comment.id)) {
          this.comments = { ...c, list: [...c.list, comment] }
        }
        this.changed()
        this.scheduleRead(origin)
        return { ok: true, projectId: w.projectId ?? '' }
      }
      const { snapshot } = parseWriteAnswer(answer, { hubId: origin.hostId, spaceId, ...(w.projectId ? { projectId: w.projectId } : {}) }, w.clientRequestId)
      // O meta vem no topo (donnaTaskMetaSave) e/ou dentro do snapshot (contrato do Hub, aditivo).
      const rawMeta = (answer as { meta?: unknown }).meta ?? (answer as { snapshot?: { meta?: unknown } }).snapshot?.meta
      const meta = rawMeta === undefined ? null : parseMetaMap(rawMeta)
      this.writeOp = null
      this.write = null
      // Criar abre o Projeto novo; gravar de outra tela (Lista, Quadro) não troca o Projeto aberto.
      if (w.op === 'create' || (this.view.kind !== 'none' && this.view.projectId === snapshot.project.id)) {
        this.project = snapshot
        this.view = { kind: 'ready', projectId: snapshot.project.id }
        if (meta) this.projectMeta = meta
        else if (w.op === 'create') this.projectMeta = null
      }
      if (this.tasks) {
        this.tasks = this.tasks.map((b) => (b.project.id === snapshot.project.id ? bundleOf(snapshot, meta ?? b.meta) : b))
        if (w.op === 'create' && snapshot.project.state === 'active' && !this.tasks.some((b) => b.project.id === snapshot.project.id)) this.tasks = [...this.tasks, bundleOf(snapshot)]
      }
      this.readAt = this.now()
      this.changed()
      // A lista (nomes, contagem, arquivados) vem do Hub, não deste snapshot.
      this.scheduleRead(origin)
      return { ok: true, projectId: snapshot.project.id }
    } catch (e) {
      if (!this.sameAuthority(op)) return { ok: false, status: 'blocked' }
      const kind = writeFailure(e)
      if (kind === 'lostAccess') {
        this.lostAccess()
        return { ok: false, status: 'rejected' }
      }
      if (kind === 'authority') {
        // Nenhum efeito com autoridade velha: o pedido não fica para Tentar de novo e nada é reenviado sozinho; o
        // formulário continua aberto com o rascunho. Só uma leitura autorizada (mesma origem) libera gravar de novo.
        this.dropWrite()
        this.authorityInDoubt()
        this.reread()
        return { ok: false, status: 'blocked' }
      }
      if (kind === 'unavailable') {
        this.dropWrite()
        this.list = null
        this.project = null
        this.verified = false
        this.phase = { kind: 'error', reason: 'unavailable' }
        this.changed()
        return { ok: false, status: 'rejected' }
      }
      // Erro que chegou por um link já trocado não diz nada da origem atual: no máximo, incerto.
      const status = this.current(origin) ? kind : 'uncertain'
      this.writeOp = null
      this.write = { ...w, status, detail: status === 'rejected' ? hubDetail(e) : null }
      this.changed()
      if (status === 'conflict') this.reread()
      return status === 'uncertain' ? { ok: false, status, requestId: w.clientRequestId } : { ok: false, status }
    }
  }

  private connect(source: TowerSource) {
    this.stopEvents?.()
    this.stopEvents = null
    const gen = ++this.gen
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    this.verified = false
    this.invalidateAttention()
    this.legacyHistory = false
    // Outro Hub: outra autoridade; nada do que estava na tela (nem o pedido) continua. Outra época do MESMO Hub é só
    // outro ciclo do stream (o BotSync sobe a época a cada hello, inclusive na reconexão): a fotografia fica, antiga,
    // e só a leitura autorizada desta origem nova libera gravar.
    if (hostId && this.hostId && this.hostId !== hostId) this.forget()
    if (hostId) {
      this.hostId = hostId
    }
    if (!hostId || !client || !this.spaceId) {
      this.origin = null
      if (this.link === 'live') this.offlineSince = this.now()
      this.link = 'offline'
      if (!this.list) this.phase = { kind: 'error', reason: 'offline' }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, gen }
    this.origin = origin
    this.link = 'live'
    this.offlineSince = null
    this.events = 'live'
    this.relistenDelay = RELISTEN_MIN
    if (!this.list) this.phase = { kind: 'loading' }
    this.changed()
    // Primeiro a inscrição, depois a leitura: um aviso entre as duas não se perde.
    this.listen(origin, this.spaceId)
    void this.read(origin)
  }

  private listen(origin: Origin, spaceId: string) {
    this.stopEvents = origin.client.inSpace(spaceId).events(origin.source.hello?.rev ?? 0, 'desktop', (e) => {
      if (!this.current(origin)) return
      this.relistenDelay = RELISTEN_MIN
      const ev = e as TowerEventWithTask
      if ((ev.spaceId !== undefined && ev.spaceId !== spaceId) || (ev.hubId !== undefined && ev.hubId !== origin.hostId)) return
      const l = this.linked
      const entry = ev.entry
      const linkedSession = (id: string | undefined) => !!id && !!l && (l.state !== 'ready' || l.sessions.some((s) => s.sessionId === id))
      const linkedBot = (id: string | undefined) => !!id && !!l && (l.state !== 'ready' || l.sessions.some((s) => s.botId === id))
      const lostBot = (ev.type === 'botDeleted' && linkedBot(ev.id))
        || (ev.type === 'bot' && ev.bot && linkedBot(ev.bot.id)
          && (ev.bot.deleted || ev.bot.hidden || (ev.bot.spaceId !== undefined && ev.bot.spaceId !== spaceId)))
      const card = ev.type === 'entry' && entry && entry.sessionId
        && (entry.kind === 'permission' || (entry.kind === 'notice' && entry.data?.card === 'ownerQuestion'))
      const needsRead = (card && linkedSession(entry.sessionId))
        || (ev.type === 'sessionDeleted' && linkedSession(ev.sessionId))
        || (ev.type === 'session' && ev.session?.state !== 'open' && linkedSession(ev.session?.id))
        || (ev.type === 'conversation' && ev.conversation?.closed && linkedBot(ev.conversation.botId))
      const resync = ev.type === 'hello' || ev.type === 'resync' || ev.type === 'spaces'
      const projects = ev.type === PROJECTS_EVENT && ev.spaceId === spaceId
      const newTaskSession = ev.type === 'session' && ev.session?.state === 'open'
        && ev.session.origin?.kind === 'task' && ev.session.origin.taskId === l?.taskId
      if (resync || lostBot || needsRead) {
        this.invalidateAttention()
      }
      if (l && (lostBot || needsRead || projects || newTaskSession)) {
        void this.readLinked(origin)
      } else if (l && !this.linkedTimer && ((card && entry.sessionId !== entry.botId)
        || (ev.type === 'conversation' && linkedBot(ev.conversation?.botId)))) {
        this.linkedTimer = setTimeout(() => {
          this.linkedTimer = null
          if (this.current(origin) && this.events === 'live') void this.readLinked(origin)
        }, REREAD_DELAY)
      }
      if (projects || resync || ev.type === 'bot' || ev.type === 'botDeleted') {
        this.scheduleRead(origin)
      }
    }, () => {
      if (!this.current(origin)) return
      this.events = 'paused'
      this.invalidateAttention()
      if (this.offlineSince === null) this.offlineSince = this.now()
      this.changed()
      if (this.relistenTimer) clearTimeout(this.relistenTimer)
      this.relistenTimer = setTimeout(() => {
        this.relistenTimer = null
        if (!this.current(origin)) return
        this.events = 'live'
        this.offlineSince = null
        this.stopEvents?.()
        this.listen(origin, spaceId)
        void this.read(origin)
      }, this.relistenDelay)
      this.relistenDelay = Math.min(RELISTEN_MAX, this.relistenDelay * 2)
    })
  }

  /** Outro Hub encerra e apaga; link novo do mesmo Hub relê; sem link, só a última fotografia, como antiga. */
  private watch(source: TowerSource) {
    const hostId = source.hello?.hostId ?? null
    if (this.hostId && hostId && hostId !== this.hostId) {
      this.release()
      this.gen++
      this.origin = null
      this.forget()
      this.phase = { kind: 'moved' }
      this.changed()
      return
    }
    const client = source.connectedClient
    if (!client) {
      if (this.link === 'live') this.connect(source)
      return
    }
    const origin = this.origin
    if (!origin || origin.client !== client || origin.epoch !== source.principalEpoch || this.link === 'offline') this.connect(source)
  }

  private scheduleRead(origin: Origin) {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = setTimeout(() => {
      this.rereadTimer = null
      if (this.current(origin)) void this.read(origin)
    }, REREAD_DELAY)
  }

  private current(origin: Origin) {
    const { source } = origin
    return this.origin === origin && this.gen === origin.gen && source.hello?.hostId === origin.hostId
      && source.connectedClient === origin.client && source.principalEpoch === origin.epoch
  }

  private forget() {
    this.dropWrite()
    this.verified = false
    this.list = null
    this.project = null
    this.tasks = null
    this.tasksError = null
    this.projectMeta = null
    this.agendas = null
    this.linked = null
    this.comments = this.comments ? { ...this.comments, state: 'loading', list: [], omitted: 0, reason: null } : null
    this.view = this.view.kind === 'none' ? this.view : { kind: 'loading', projectId: this.view.projectId }
    this.offlineSince = null
    this.readAt = null
    this.rereadFailed = false
    this.authority = 'ok'
    this.phase = { kind: 'loading' }
  }

  /** Autoridade em dúvida: a fotografia da mesma origem fica (antiga) e nada grava até a próxima leitura autorizada. */
  private authorityInDoubt() {
    this.verified = false
    this.invalidateAttention()
    if (this.authority === 'ok' && this.offlineSince === null) this.offlineSince = this.now()
    this.authority = 'pending'
    this.changed()
  }

  private lostAccess() {
    // Perder o acesso apaga a projeção e o pedido guardado, não só desliga botões.
    this.dropWrite()
    this.verified = false
    this.authority = 'ok'
    this.list = null
    this.project = null
    this.tasks = null
    this.projectMeta = null
    this.comments = null
    this.linked = null
    this.agendas = null
    this.readAt = null
    this.phase = { kind: 'error', reason: 'lostAccess' }
    if (this.view.kind !== 'none') this.view = { kind: 'error', projectId: this.view.projectId, reason: 'lostAccess' }
    this.changed()
  }

  private release() {
    if (this.rereadTimer) clearTimeout(this.rereadTimer)
    this.rereadTimer = null
    if (this.linkedTimer) clearTimeout(this.linkedTimer)
    this.linkedTimer = null
    if (this.relistenTimer) clearTimeout(this.relistenTimer)
    this.relistenTimer = null
    this.stopEvents?.()
    this.stopEvents = null
    this.unwatch?.()
    this.unwatch = null
  }

  /** Leitura completa e autorizada desta origem: só ela libera gravar (e encerra a dúvida de autoridade). */
  private authorized() {
    this.verified = true
    if (this.authority === 'pending') {
      this.authority = 'ok'
      if (this.link === 'live' && this.events === 'live') this.offlineSince = null
    }
  }

  /**
   * Todas as tarefas dos Projetos ativos: pela leitura paginada `donnaTasks` quando o Hub a anuncia (traz os
   * metadados), senão um `donnaProject` por Projeto ativo (traz o histórico). Só pinta o conjunto inteiro.
   */
  private async readTasks(origin: Origin, list: ProjectsList, fresh: () => boolean): Promise<'ok' | 'stop'> {
    const spaceId = this.spaceId!
    const where = { hubId: origin.hostId, spaceId }
    try {
      const bundles: TaskBundle[] = []
      if (list.capabilities.includes(TASK_CAPABILITIES.list)) {
        let after: string | null = null
        for (let page = 0; page < TASK_LIMITS.pages; page++) {
          const answer = await origin.client.call('donnaTasks', { ...where, afterProjectId: after })
          if (!fresh()) return 'stop'
          const { bundle, next } = parseTasksPage(answer, where, { project: parseProject, task: parseTask })
          if (bundle && bundle.project.state === 'active') bundles.push(bundle)
          if (!next || next === after) break
          after = next
        }
      } else {
        for (const p of list.projects.filter((x) => x.state === 'active')) {
          const answer = await this.callSnapshot(origin.client, 'donnaProject', { ...where, projectId: p.id })
          if (!fresh()) return 'stop'
          const snap = parseProjectSnapshot(answer, { ...where, projectId: p.id })
          const meta = (answer as { meta?: unknown }).meta
          if (snap.project.state === 'active') bundles.push(bundleOf(snap, meta === undefined ? {} : parseMetaMap(meta)))
        }
      }
      this.tasks = bundles
      this.tasksError = null
      if (list.capabilities.includes(TASK_CAPABILITIES.agenda)) {
        try {
          const answer = await origin.client.call('donnaTaskAgenda', where)
          if (!fresh()) return 'stop'
          this.agendas = parseAgendaList(answer, where)
        } catch {
          // Sem a agenda, a Lista só não mostra “Começa…”; nada mais muda.
          if (!fresh()) return 'stop'
          this.agendas = null
        }
      } else this.agendas = null
      return 'ok'
    } catch (e) {
      if (!fresh()) return 'stop'
      const reason = readFailure(e)
      if (reason === 'lostAccess') { this.lostAccess(); return 'stop' }
      if (reason === 'authority') {
        this.authorityInDoubt()
        return 'stop'
      }
      if (!this.tasks || reason === 'invalid' || reason === 'unavailable') {
        this.tasks = null
        this.tasksError = reason
      } else this.rereadFailed = true
      return 'ok'
    }
  }

  private invalidateAttention() {
    this.linkedReads++
    if (this.linked?.attention) {
      this.linked = { ...this.linked, attention: null }
      this.changed()
    }
  }

  private async readLinked(origin: Origin) {
    const l = this.linked
    const spaceId = this.spaceId
    if (!l || !spaceId || !this.has(TASK_CAPABILITIES.sessions)) return
    const read = ++this.linkedReads
    const where = { hubId: origin.hostId, spaceId, projectId: l.projectId, taskId: l.taskId }
    const fresh = () => this.current(origin) && this.events === 'live' && read === this.linkedReads && this.linked?.projectId === l.projectId && this.linked.taskId === l.taskId
    try {
      const answer = await origin.client.call('donnaTaskSessions', where)
      if (!fresh()) return
      const { sessions, agenda, attention } = parseSessions(answer, where)
      this.linked = { ...l, state: 'ready', sessions, agenda, attention, reason: null }
    } catch (e) {
      if (!fresh()) return
      const reason = readFailure(e)
      if (reason === 'lostAccess') return this.lostAccess()
      if (reason === 'authority') this.authorityInDoubt()
      if (this.linked?.state !== 'ready') this.linked = { ...l, state: 'error', sessions: [], agenda: null, attention: null, reason }
      else this.linked = { ...this.linked, attention: null }
    } finally {
      if (this.current(origin)) this.changed()
    }
  }

  private async readComments(origin: Origin) {
    const c = this.comments
    const spaceId = this.spaceId
    if (!c || !spaceId) return
    if (!this.has(TASK_CAPABILITIES.comments)) {
      this.comments = { ...c, state: 'error', list: [], reason: 'unavailable' }
      this.changed()
      return
    }
    const read = ++this.commentsReads
    const where = { hubId: origin.hostId, spaceId, projectId: c.projectId, taskId: c.taskId }
    const fresh = () => this.current(origin) && read === this.commentsReads && this.spaceId === spaceId
      && this.comments?.projectId === c.projectId && this.comments.taskId === c.taskId
    try {
      const answer = await origin.client.call('donnaTaskComments', where)
      if (!fresh()) return
      const { comments, omitted } = parseComments(answer, where)
      this.comments = { ...c, state: 'ready', list: comments, omitted, reason: null }
    } catch (e) {
      if (!fresh()) return
      const reason = readFailure(e)
      if (reason === 'lostAccess') return this.lostAccess()
      if (reason === 'authority') return this.authorityInDoubt()
      // Com comentários na tela, eles ficam (a faixa de releitura avisa); sem nada, o erro aparece no lugar.
      if (this.comments?.state === 'ready' && reason !== 'invalid') this.rereadFailed = true
      else this.comments = { ...c, state: 'error', list: [], omitted: 0, reason }
    } finally {
      if (this.current(origin)) this.changed()
    }
  }

  private async read(origin: Origin) {
    if (this.reading === origin) {
      this.dirty = true
      return
    }
    this.reading = origin
    this.dirty = false
    const read = ++this.reads
    const spaceId = this.spaceId!
    const fresh = () => this.current(origin) && read === this.reads && this.spaceId === spaceId
    try {
      const answer = await origin.client.call('donnaProjects', { hubId: origin.hostId, spaceId })
      if (!fresh()) return
      const list = parseProjectsList(answer, { hubId: origin.hostId, spaceId })
      if (!list.capable) {
        // Hub sem o anúncio: indisponível, sem alternativa.
        this.verified = false
        this.list = null
        this.project = null
        this.phase = { kind: 'error', reason: 'unavailable' }
        return
      }
      this.list = list
      this.phase = { kind: 'ready' }
      this.rereadFailed = false
      this.readAt = this.now()
      if (this.wantTasks) {
        const ok = await this.readTasks(origin, list, fresh)
        if (ok === 'stop') return
      }
      const view = this.view
      // Mudança só depois de uma leitura completa desta origem (lista e, aberto, o Projeto).
      if (view.kind === 'none') {
        this.authorized()
        return
      }
      const projectId = view.projectId
      try {
        const snap = await this.callSnapshot(origin.client, 'donnaProject', { hubId: origin.hostId, spaceId, projectId })
        if (!fresh() || this.view.kind === 'none' || this.view.projectId !== projectId) return
        this.project = parseProjectSnapshot(snap, { hubId: origin.hostId, spaceId, projectId })
        const meta = (snap as { meta?: unknown }).meta
        this.projectMeta = meta === undefined ? null : parseMetaMap(meta)
        this.view = { kind: 'ready', projectId }
        this.authorized()
        if (this.comments?.projectId === projectId) void this.readComments(origin)
        if (this.linked?.projectId === projectId) void this.readLinked(origin)
      } catch (e) {
        if (!fresh() || this.view.kind === 'none' || this.view.projectId !== projectId) return
        const reason = readFailure(e)
        if (reason === 'lostAccess') return this.lostAccess()
        if (reason === 'authority') {
          // Sem prova de revogação: mantém a fotografia (se houver) e bloqueia gravar até a leitura autorizada.
          this.authorityInDoubt()
          if (this.project) return
        }
        if (!this.project || reason === 'unavailable' || reason === 'invalid') {
          this.project = null
          this.view = { kind: 'error', projectId, reason }
        } else this.rereadFailed = true
      }
    } catch (e) {
      if (!fresh()) return
      const reason = readFailure(e)
      if (reason === 'lostAccess') return this.lostAccess()
      if (reason === 'authority') {
        this.authorityInDoubt()
        if (this.list) return
      }
      if (!this.list || reason === 'unavailable' || reason === 'invalid') {
        this.verified = false
        this.list = null
        this.project = null
        this.phase = { kind: 'error', reason }
      } else this.rereadFailed = true
    } finally {
      if (this.reading === origin) this.reading = null
      if (this.current(origin)) this.changed()
      const latest = this.origin
      if (this.dirty && latest && this.current(latest)) this.scheduleRead(latest)
    }
  }
}
