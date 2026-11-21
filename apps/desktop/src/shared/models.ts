// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { SpaceHello } from './fork-space-hello'
import type { BrowserPreference } from './fork-browser.ts'
import { readSkillSelectionStamp, type SkillSelectionStamp } from './fork-skill-selection.ts'
import type { ContentOrigin } from './guard-hub.ts'
import { configForRole, validAvatar, type AvatarConfig, type MexeriquinhaConfig, validMexeriquinha, projetarAvatarLegado, equivalenciaAvatar } from './fork-avatar-config.ts'
import { parseActivityCode, type ActivityCode } from './fork-activity-code.ts'
import { onlyQuestionsPending, readPendingQuestions, type PendingQuestion } from './fork-owner-question.ts'
// Wire types for codync-host (see host/src/api/). Field names match the
// host's camelCase JSON. Timestamps are epoch milliseconds.

/**
 * `unavailable` (runtime-audiencia-01): o Hub não comprovou a audiência deste runtime neste Space. É falta de
 * informação, não um estado: nunca vira Livre, trabalho, espera, falha, concluído ou zero trabalhos.
 */
export type BotStatus = 'idle' | 'working' | 'needsInput' | 'waiting' | 'error' | 'unavailable'
const KNOWN_STATUS: ReadonlySet<string> = new Set<BotStatus>(['idle', 'working', 'needsInput', 'waiting', 'error', 'unavailable'])

export interface ScreenAccess {
  target: string
  name: string
  available: boolean
  enabled: boolean
  /** Recibo de Agora neste Hub: permite encerrar, sem afirmar validade atual. */
  canEndTemporary?: boolean
  notice: string
}

export interface Bot {
  id: string
  /** D169: origem persistida no Hub; ausente somente no protocolo legado. */
  spaceId?: string
  /** `agent`, or `group`: several bots and the user in one chat. */
  kind: string
  members: string[]
  /** O líder do canal, um dos `members`; null sem líder, em bots e com Hub antigo. */
  leader: string | null
  name: string
  description: string
  /** D368: "Suas preferências para a Donna", só na principal; vazio nos demais e com Hub antigo. */
  ownerPreferences?: string
  avatarColor: string
  avatarShape: string
  avatarConfig?: AvatarConfig
  mexeriquinha?: MexeriquinhaConfig
  /** D133: a função do bot, etiqueta ao lado do nome; vazia em grupos e com Hub antigo. */
  title: string
  backend: string
  forkEditor?: { engineAccountId: string | null }
  command?: string | null
  cwd: string
  /** Host-allocated personal workspace rather than a user-selected project. */
  managedWorkspace: boolean
  /** Aviso do Hub para revisar uma pasta antiga recusada pela política atual. */
  workspaceIssue?: string | null
  /** `ask` or `auto`. */
  permission: string
  model?: string | null
  /** The model's own settings (effort, fast…): agent option id → value. */
  modelOptions: Record<string, string>
  /** The agent's Fast mode (Claude Code, Codex): faster replies for more usage. */
  fastMode: boolean
  /** What the agent says about Fast mode: `unknown` (no session yet), `offered`, `unsupported`,
   *  or `refused` (it turned it off; `fastModeIssue` says why). */
  fastModeSupport: string
  fastModeIssue: string | null
  pinned: boolean
  /** Where the user dragged it, lowest first (after pinned bots); ties sort by recent activity. */
  position: number
  hidden: boolean
  notify?: boolean | null
  connectors: string[]
  /** Saved logins (ids) the bot may list and type (D82); none until the user turns one on. */
  logins: string[]
  skills: string[]
  /** Condição opaca da seleção emitida pelo Hub (c1); não faz parte do draft. */
  skillSelection?: SkillSelectionStamp | null
  /** The bot can see and operate the computer's desktop. */
  computer: boolean
  /** Preferência do dono; não concede computador, tela ou perfil. */
  browserPreference: BrowserPreference
  /** D510: projeção do Hub, sem herdar config.computer. */
  screenAccess?: ScreenAccess[] | null
  /** The bot can create bots (which can too) and change any bot's setup (the built-in `manage` tools); null from an older host. */
  manageTeam: boolean | null
  /** The bot that created this one; null when the user did. */
  createdBy: string | null
  /** Uses the memory all the user's bots share ("About you"). */
  sharedMemory: boolean
  /** Keeps the harness's own memory (Claude Code's…) on beside Codync's; null where it has no switch. */
  harnessMemory: boolean | null
  /** Moves whenever memory changes, so an open memory screen fetches it again. */
  memoryRev: number
  autoName: boolean
  createdAt: number
  rev: number
  status: BotStatus
  activity: string
  /** MX-147: código estável da atividade (o app traduz); ausente, desconhecido ou malformado fica `null` e vale `activity`. */
  activityCode?: ActivityCode | null
  /** Reset previsto da assinatura, em epoch ms; null quando desconhecido. */
  resumeAt?: number | null
  startedAt?: number | null
  workingChat?: string | null
  /** MB-1b: perguntas `ask_owner` pendentes (até 5, mais novas primeiro); `[]` com Hub antigo. */
  pendingQuestions: PendingQuestion[]
  unread: number
  lastMessage?: string | null
  lastAt: number
}

/** Lenient: a host that is a little newer or older than the app still yields a usable bot. */
export function normalizeBot(raw: Partial<Bot> & { id: string }): Bot {
  const createdAt = raw.createdAt ?? 0
  return withRuntime({
    id: raw.id,
    ...(typeof raw.spaceId === 'string' ? { spaceId: raw.spaceId } : {}),
    kind: raw.kind ?? 'agent',
    members: raw.members ?? [],
    leader: raw.leader ?? null,
    name: raw.name ?? 'Bot',
    description: raw.description ?? '',
    ownerPreferences: raw.ownerPreferences ?? '',
    avatarColor: raw.avatarColor ?? 'blue',
    avatarShape: raw.avatarShape ?? 'blob',
    avatarConfig: validAvatar(raw.avatarConfig) ? raw.avatarConfig : configForRole(raw.name, raw.description, raw),
    ...(validMexeriquinha(raw.mexeriquinha) && (!validAvatar(raw.avatarConfig) || equivalenciaAvatar(projetarAvatarLegado(raw.mexeriquinha), raw.avatarConfig)) ? { mexeriquinha: raw.mexeriquinha } : {}),
    title: raw.title ?? '',
    backend: raw.backend ?? 'custom',
    forkEditor: { engineAccountId: raw.forkEditor?.engineAccountId ?? null },
    command: raw.command ?? null,
    cwd: raw.cwd ?? '',
    managedWorkspace: raw.managedWorkspace ?? false,
    workspaceIssue: raw.workspaceIssue ?? null,
    permission: raw.permission ?? 'auto',
    model: raw.model ?? null,
    modelOptions: raw.modelOptions ?? {},
    fastMode: raw.fastMode ?? false,
    fastModeSupport: raw.fastModeSupport ?? 'unknown',
    fastModeIssue: raw.fastModeIssue ?? null,
    pinned: raw.pinned ?? false,
    position: raw.position ?? 0,
    hidden: raw.hidden ?? false,
    notify: raw.notify ?? null,
    connectors: raw.connectors ?? [],
    logins: raw.logins ?? [],
    skills: raw.skills ?? [],
    skillSelection: readSkillSelectionStamp(raw.skillSelection),
    computer: raw.computer ?? false,
    browserPreference: raw.browserPreference === 'computer' || raw.browserPreference === 'off' ? raw.browserPreference : 'integrated',
    screenAccess: Array.isArray(raw.screenAccess) ? raw.screenAccess : null,
    manageTeam: raw.manageTeam ?? null,
    createdBy: raw.createdBy ?? null,
    sharedMemory: raw.sharedMemory ?? raw.managedWorkspace ?? false,
    harnessMemory: raw.harnessMemory ?? null,
    memoryRev: raw.memoryRev ?? 0,
    autoName: raw.autoName ?? false,
    createdAt,
    rev: raw.rev ?? 0,
    activity: raw.activity ?? '',
    activityCode: parseActivityCode(raw.activityCode),
    resumeAt: raw.resumeAt ?? null,
    startedAt: raw.startedAt ?? null,
    workingChat: raw.workingChat ?? null,
    pendingQuestions: readPendingQuestions(raw.pendingQuestions),
    unread: raw.unread ?? 0,
    lastMessage: raw.lastMessage ?? null,
    lastAt: raw.lastAt ?? createdAt,
  }, raw.status)
}

/**
 * runtime-audiencia-01: status ausente ou desconhecido é indisponível, não Livre. Indisponível sempre chega sem
 * atividade, horários ou conversa de trabalho, mesmo de um Hub ou cache que mande sobras.
 */
function withRuntime(bot: Omit<Bot, 'status'> & { status?: unknown }, status: unknown): Bot {
  if (typeof status === 'string' && KNOWN_STATUS.has(status) && status !== 'unavailable') return { ...bot, status: status as BotStatus }
  return unavailableRuntime(bot)
}

/** Apaga o runtime de um bot e o marca como Estado indisponível; o resto (perfil, prévia, não lidas) fica. */
export function unavailableRuntime<T extends object>(bot: T): T & Pick<Bot, 'status' | 'activity' | 'activityCode' | 'startedAt' | 'resumeAt' | 'workingChat' | 'pendingQuestions'> {
  return { ...bot, status: 'unavailable', activity: '', activityCode: null, startedAt: null, resumeAt: null, workingChat: null, pendingQuestions: [] }
}

export const isGroup = (b: Bot) => b.kind === 'group'
// MB-1b: Precisa de você só por pergunta (sem turno) não é trabalho: sem Parar, fila nem indicador de trabalho.
export const isWorking = (b: Bot) => b.status === 'working' || (b.status === 'needsInput' && !onlyQuestionsPending(b))
export const needsInput = (b: Bot) => b.status === 'needsInput'
/** runtime-audiencia-01: sem informação de runtime aqui. Não é Livre, trabalho, espera, erro nem conclusão. */
export const isUnavailable = (b: Pick<Bot, 'status'>) => b.status === 'unavailable'
export const UNAVAILABLE_TEXT = 'Estado indisponível'
/** Working right now in this chat (its own, or a group it's in). */
export const isWorkingIn = (b: Bot, chat: string) => isWorking(b) && (b.workingChat ?? b.id) === chat
export const folderName = (b: Bot) =>
  b.managedWorkspace ? 'Personal workspace' : (b.cwd.split('/').filter(Boolean).pop() ?? b.cwd)

export interface FileDiff {
  path: string
  added: number
  removed: number
  isNew: boolean
  startLine: number
  patch: string
}

export interface PermissionOption {
  optionId: string
  name: string
  /** allow_once | allow_always | reject_once | reject_always */
  kind: string
}

export interface PlanItem {
  content: string
  status: string
  priority?: string
}

/** One bot's emoji on a message. */
export interface BotReaction {
  bot: string
  emoji: string
}

/** The bots' reactions on one message, by emoji in the order each first appeared. */
export function groupBotReactions(reactions: BotReaction[]): { emoji: string; bots: string[] }[] {
  const groups: { emoji: string; bots: string[] }[] = []
  for (const r of reactions) {
    const group = groups.find((g) => g.emoji === r.emoji)
    if (group) group.bots.push(r.bot)
    else groups.push({ emoji: r.emoji, bots: [r.bot] })
  }
  return groups
}

/** The message a user message replies to, as the host quoted it. */
export interface ReplyQuote {
  id: string
  /** `user`, or the bot that wrote it. */
  author: string
  /** Its text (or file names), at most 500 characters. */
  text: string
}

export interface Attachment {
  id: string
  name: string
  size: number
}

export interface SharedFile extends Attachment { sha256: string }

export const isImageName = (name: string) =>
  ['png', 'jpg', 'jpeg', 'heic', 'gif', 'webp', 'tiff', 'bmp'].includes(name.split('.').pop()?.toLowerCase() ?? '')

export interface ConnectionRequest {
  [key: string]: unknown
}

export interface BotMessage {
  sourceBotId: string
  targetBotId: string
  text: string
  reply?: string
  detail?: string
  /** O dono editou o pedido na fila antes do envio (aviso do destinatário). */
  edited?: boolean
  /** T2a (DON-912): contrato curto proposto por quem pediu; orientação, não permissão (`host/src/fork/delegacao/contrato.rs`). */
  contract?: TaskContract
}

/** Os campos do contrato da tarefa; só `result` é obrigatório. */
export interface TaskContract {
  result: string
  acceptance?: string
  scope?: string
  limits?: string
  rule?: string
}

/** Procedência validada pelo Hub; quem gravou é um dado independente. */
export interface MemoryOrigin {
  schema: 1
  kind: 'user' | 'conversation' | 'external' | 'unknown'
  sources: { messageId: string; kind: MemoryOrigin['kind']; detail?: string }[]
  external: boolean
}

export type MemoryAvailability = 'transient' | 'noSelection' | 'full' | 'expired' | 'ineligible'
/** `confirmed`: o dono aprovou uma lembrança que o Hub achou na mensagem dele; não é fala literal (2b1d-2). */
export type MemoryNature = 'said' | 'inferred' | 'conflict' | 'confirmed' | 'unknown'
export interface MemoryVersion {
  text?: string
  rev?: number
  at?: number
  state?: string
  origin?: MemoryOrigin | null
  nature?: MemoryNature
  natureBot?: string | null
  conflictText?: string | null
}

/** A bot's memory write: what it did and whether the user can still take it back. */
export interface MemoryWrite {
  action?: 'remember' | 'revise' | 'archive' | string
  text?: string
  shared?: boolean
  confirmation?: boolean
  /** done or accepted (undo offered) · undone · changed (the entry changed since) · pending, declined, failed */
  status?: string
  /** The entry's revision when the line was posted; lines from before Mexerica have none and stay hidden. */
  rev?: number
  origin?: MemoryOrigin | null
  previousOrigin?: MemoryOrigin | null
  jobId?: string
  messageIds?: string[]
  responseIds?: string[]
  availability?: MemoryAvailability | null
  reason?: string
  nature?: MemoryNature
  natureBot?: string | null
  conflictText?: string | null
  previous?: string | null
  previousAt?: number
  previousRev?: number
  previousNature?: MemoryNature
  previousNatureBot?: string | null
  previousConflictText?: string | null
  at?: number
}

/** Whether the chat shows a memory line: only the ones Mexerica posts, which can tell whether undo still applies. */
export function memoryShown(memory: MemoryWrite | undefined): boolean {
  if (memory?.status === 'noChange') return false
  return typeof memory?.rev === 'number' || memory?.confirmation === true || typeof memory?.jobId === 'string' || memory?.availability != null
}

/** Whether the user can still undo a bot's memory write from its line. */
export function memoryUndoable(memory: MemoryWrite | undefined): boolean {
  return memoryShown(memory) && (memory?.status === 'done' || memory?.status === 'accepted')
}

/** Só disposições explícitas das notas legadas migradas; nunca reativa o cartão antigo. */
export function migratedMemoryNote(data: EntryData): MemoryWrite | undefined {
  const file = data.memoryFile
  if (!file || typeof file !== 'object' || Array.isArray(file)) return undefined
  const raw = file as Record<string, unknown>
  const availability = ['transient', 'noSelection', 'full', 'expired', 'ineligible'].includes(String(raw.availability)) ? raw.availability as MemoryAvailability : undefined
  const disposition = ['waiting', 'expired', 'notQueued', 'ineligible'].includes(String(raw.status))
  const jobId = typeof raw.jobId === 'string' && raw.jobId.length > 0 ? raw.jobId : undefined
  if (!jobId && !availability && !disposition) return undefined
  const origin = raw.origin && typeof raw.origin === 'object' ? raw.origin as MemoryOrigin : undefined
  return {
    action: 'note', status: disposition ? String(raw.status) : 'waiting',
    confirmation: false, jobId, availability,
    origin: origin?.schema === 1 && Array.isArray(origin.sources) ? origin : undefined,
  }
}

/** "Earlier context was summarized": the harness's summary, when it gave one. */
export interface Compaction {
  summary?: string | null
  /** The host started it ahead of time; otherwise the harness did when its context filled. */
  byHost?: boolean
}

/** What a new conversation carries when the bot's own chat starts over; editable until the next message sends it. */
/** DON-907 C9a: a linha de troca de modelo (host `agent/bot/fork_troca_motor.rs`). */
export interface ModelSwitch {
  from?: { backend?: string; model?: string | null; label?: string }
  to?: { backend?: string; model?: string | null; label?: string }
  /** `provider`: outro motor; `model`: o mesmo motor, outro modelo. */
  kind?: string
  /** `session`: a sessão inteira foi retomada; `handoff`: o motor novo recebeu o resumo e os últimos pedidos. */
  carried?: string
  /** O que o motor novo leu no primeiro turno; `null` até ele responder. */
  resentTokens?: number | null
}

export interface HandoffCard {
  /** draft · sent · kept */
  status: string
  /** cold · away · full · engineSwitch · continuation · restart (os três últimos sem "Manter esta conversa") */
  reason?: string
  text?: string
}

/** Motivos do rollover em que a conversa antiga ainda pode continuar. `engineSwitch`, `continuation` e `restart`
 *  (Sessões PR5) não têm conversa antiga: o Hub recusa `keepConversation`. Motivo desconhecido também não oferece. */
const KEEPABLE = new Set(['cold', 'away', 'full'])

/** Se o cartão oferece "Manter esta conversa". */
export function canKeepConversation(card: HandoffCard): boolean {
  return card.reason === undefined || KEEPABLE.has(card.reason)
}

/** Referência pública; detalhes privados só no RPC do operador Local. Não concede aprovação. */
export interface ConectorOperation {
  tentativaId: string
  cardId: string
  state: 'aguardando' | 'autorizada' | 'em_envio' | 'recusada' | 'expirada' | 'cancelada' | 'concluida' | 'falha_comprovada' | 'incerta'
  /** 2: cartão com conta de saída e rótulos curados; 1 só em recibos antigos. */
  decisionProtocol: 1 | 2
}

export interface EntryData {
  /** MB-1b: `ownerQuestion` marca o cartão de pergunta do bot (ver `fork-owner-question.ts`). */
  card?: string
  ownerQuestion?: unknown
  conectorOperation?: ConectorOperation
  conectorDraft?: { draftId: string; revision: number; deadline: number; state: 'draft' | 'closed' }
  /** Marcador de sessão do Hub para apps antigos (D229-5); escondido aqui. */
  sessionMarker?: { sessionId: string }
  /** Metadado do Hub, sem autoridade de grant ou recibo de leitura. */
  spaceContext?: import('./fork-spaces').SpaceContext
  /**
   * DON-907 C4: a espera do limite. `waitId` identifica esta espera (sem ele, Hub antigo: sem botões novos);
   * `cancelled` = "Não retomar" gravado; `snoozed` = em soneca; `autoResume` = a retomada automática dela;
   * `ready` = chegou o prazo informado para o limite (não prova que voltou); `retry` = o Hub aceita retomar à mão agora;
   * `alreadyStarted` = o cancelamento chegou depois de a retomada ter sido autorizada.
   */
  subscriptionWait?: { resumeAt?: number | null; retry?: boolean; waitId?: string; cancelled?: boolean; snoozed?: boolean; autoResume?: boolean; ready?: boolean; alreadyStarted?: boolean }
  /**
   * MX-263: o aviso é a espera de uma execução de rotina. Preserva os controles da espera (não
   * usa `routineId` no topo, que o app roteia para o botão "abrir a rotina") e o texto do Hub.
   */
  routineWait?: { routineId: string; runId: string }

  /** Structured conversation data added to the existing team-tool notice. */
  botMessage?: BotMessage
  /** notice: the agent summarized its context; kept out of the chat. */
  compaction?: Compaction
  /** notice: the bot's chat starts over with this handoff (see `HandoffNotice`). */
  handoff?: HandoffCard
  /** notice: DON-907 C9a, o modelo ou o motor da conversa mudou (`fork/troca-modelo`). */
  modelSwitch?: ModelSwitch
  /** D67: o snapshot de restauração permanece privado no host. */
  teamChange?: { status: 'available' | 'undone'; targetBotId: string }
  heading?: string
  delegationId?: string
  /** D371: só no aviso do remetente; versão das regras de delegação em vigor ao delegar. */
  delegationRulesVersion?: number
  sourceBotId?: string
  targetBotId?: string
  /** Pedidos de bots na fila: a lane do remetente (conversa e sessão extra) no aviso do destinatário. */
  sourceChat?: string
  sourceSession?: string | null
  connectionRequest?: ConnectionRequest
  /** A bot's memory write in its chat: "Lembrei: …" with an undo (see `MemoryNotice`). */
  memory?: MemoryWrite
  /** A memory-file line an older host put in the chat. Notes live in the bot's memory settings, so it isn't shown. */
  memoryFile?: unknown
  routineId?: string
  runId?: string
  text?: string
  /** Continuação interna criada pelo Hub, não uma mensagem da pessoa. */
  from?: string
  final?: boolean
  /** user: queued | sent | cancelled | failed · tool: pending | in_progress | completed | failed · permission: pending | answered | cancelled | expired */
  status?: string
  clientNonce?: string
  /** Só na bolha local que não foi enviada: o motivo da recusa da Open, como chave de texto. */
  recusa?: string
  /** Código fixo de um aviso do Hub, traduzido pela janela quando ela o conhece. */
  code?: string
  contentOrigin?: ContentOrigin
  pasted?: string[]
  /** DON-907 (D431): a posição na fila depois de uma reordenação (`queuedMove`); sem ela, vale `seq`. */
  queueOrder?: number
  title?: string
  toolKind?: string
  /** D98: authoritative Hub classification; missing/unknown means in-app only. */
  voiceApproval?: 'allow_once' | 'app_only'
  /** D511: metadados fixos do cartão gerados pelo Hub, sem motivo do bot. */
  screenAccess?: { target: string; generation: number; execution: string; expiresAt: number; effect?: boolean }
  output?: string
  diffs?: FileDiff[]
  locations?: { path: string; line?: number }[]
  options?: PermissionOption[]
  selected?: string
  command?: string
  detail?: string
  cwd?: string
  entries?: PlanItem[]
  /** notice: info | error */
  style?: string
  /** Falha de conta/motor (Hub novo): código fechado + conta e motor do turno. Nunca rótulo, e-mail nem token; o app resolve a conta pela lista. */
  reason?: { code: string; accountId?: string; backend?: string }
  /** user (failed): o texto fixo que o Hub grava junto do `reason`. */
  error?: string
  author?: string
  /** user: the message this one replies to. */
  replyTo?: ReplyQuote
  reactions?: string[]
  /** The bots' reactions, one per bot, oldest first. */
  botReactions?: BotReaction[]
  attachments?: Attachment[]
  files?: SharedFile[]
  callSeconds?: number
}

export interface Entry {
  spaceId?: string
  conversationId?: string
  id: string
  seq: number
  botId: string
  rev: number
  /** user | agent | thought | tool | plan | permission | notice */
  kind: string
  turn: number
  data: EntryData
  createdAt: number
  updatedAt: number
  /** Sessão da entrada; a principal tem o id da conversa. Hubs anteriores não mandam. */
  sessionId?: string
}

/** Uma sessão do bot (`sessions`); a principal tem o id da conversa e fica sempre no topo. */
export interface Session {
  id: string
  chatId: string
  principal: boolean
  origin: { kind: string } & Record<string, unknown>
  sourceSession: string | null
  parentSession: string | null
  depth: number
  title: string
  titleSource: string
  state: 'open' | 'closed'
  closedReason: string | null
  closedAt: number | null
  createdAt: number
  lastUsedAt: number
  revision: number
  /**
   * D372: a seleção de motor própria desta conversa (Hub com o anúncio `engine`). `null` = o Hub não tem uma
   * seleção legível para ela (fora do Pessoal ou gravada ilegível); ausente = Hub anterior. Nunca é o modelo do bot.
   */
  engine?: SessionEngine | null
}

/** D372 (`host/src/fork/sessoes/engine_choice.rs` `EngineView`): `null` em conta/modelo é ausência real. */
export interface SessionEngine {
  accountId: string | null
  backend: string
  model: string | null
  modelOptions: Record<string, string>
  fastMode: boolean
  /** `botDefault` = retrato do padrão do bot na criação da conversa (não acompanha o padrão depois). */
  source: 'botDefault' | 'own'
  /** A mesma `Session.revision` (CAS do `sessionEngineSet`). */
  revision: number
}

/** Entradas internas continuam disponíveis ao motor, mas não são falas da pessoa. */
export function isHostMessage(e: Entry): boolean {
  return e.kind === 'user' && e.data.from === 'host'
}

/** Chat-visible (Grok Bot model): user messages, final replies, approval cards, notices. */
export function isChat(e: Entry): boolean {
  switch (e.kind) {
    case 'user':
      return !isHostMessage(e)
    case 'permission':
      return true
    case 'notice':
      // A bot's memory write gets a line with its undo; notes live in the bot's memory settings, a summary keeps the
      // same conversation going, and a kept handoff changed nothing.
      // O marcador de sessão é só para apps antigos; este app mostra a sessão no topo da conversa.
      return (!e.data.memory || memoryShown(e.data.memory)) && (!e.data.memoryFile || !!migratedMemoryNote(e.data)) && !e.data.compaction && e.data.handoff?.status !== 'kept' && !e.data.sessionMarker
    case 'agent':
      return e.data.final === true
    default:
      return false
  }
}

/** Messages a reply can quote: the user's and the bots' final replies, once the computer has them. */
export function isReplyable(e: Entry): boolean {
  return !isHostMessage(e) && !e.id.startsWith('local-') && (e.kind === 'user' || (e.kind === 'agent' && e.data.final === true))
}

/** The quote a sending reply shows until the computer's own replaces it. */
export function quoteOf(e: Entry): ReplyQuote {
  const names = [...(e.data.attachments ?? []), ...(e.data.files ?? [])].map((f) => f.name).join(', ')
  return { id: e.id, author: e.kind === 'user' ? 'user' : (e.data.author ?? e.botId), text: (e.data.text || names).slice(0, 500) }
}

export interface UsageWindow {
  id: string
  label: string
  percent: number
  resetsAt?: number | null
  resetsText?: string | null
}

export interface UsageProvider {
  id: string
  name: string
  windows: UsageWindow[]
  source: string
  updatedAt: number
}

export interface Usage {
  providers: UsageProvider[]
}

export interface Backend {
  id: string
  name: string
  available: boolean
  command: string
  installHint: string
  registry?: string
  description?: string
  installed?: boolean
  curated?: boolean
  signedIn?: boolean
  canInstall?: boolean
  /** Only for an agent that runs on Node.js, on a computer without its own: Codync's copy is missing or added. */
  node?: 'missing' | 'added'
}

export interface ScreenDisplay {
  id: number
  name: string
  width: number
  height: number
  main: boolean
}

export interface ScreenState {
  /** The helper's display name in operating-system permission settings. */
  permissionApp?: string
  enabled: boolean
  connected: boolean
  platform: string
  capture: boolean
  input: boolean
  displays: ScreenDisplay[]
  userControl: boolean
  /**
   * R1 (decisão A): uso por bot projetado para este Space. `unavailable` não diz quem, onde nem se o computador está
   * livre; só `using` traz `agentBot`. Ausente ou desconhecido vira `unavailable`.
   */
  agentUse: AgentUse
  agentBot: string | null
  /** Bots with computer use can act here (Windows: always; elsewhere Remote screen is on). */
  computerUse: boolean
  viewers: number
}

export type AgentUse = 'idle' | 'using' | 'unavailable'
export const SCREEN_USE_UNAVAILABLE_TEXT = 'Uso do computador indisponível'

/** Every screen state from the Hub (hello, RPC answers, events) goes through here before any view reads it. */
export function normalizeScreen(raw: Partial<Omit<ScreenState, 'agentUse'>> & { agentUse?: unknown } | undefined | null): ScreenState | null {
  if (!raw) return null
  const agent = typeof raw.agentBot === 'string' && raw.agentBot ? raw.agentBot : null
  // Só `using` com um ID é uso comprovado; `idle` nunca carrega identidade e o resto é indisponível.
  const agentUse: AgentUse = raw.agentUse === 'using' && agent ? 'using' : raw.agentUse === 'idle' && !agent ? 'idle' : 'unavailable'
  return {
    enabled: raw.enabled ?? false,
    connected: raw.connected ?? false,
    platform: raw.platform ?? '',
    permissionApp: raw.permissionApp,
    capture: raw.capture ?? false,
    input: raw.input ?? false,
    displays: raw.displays ?? [],
    userControl: raw.userControl ?? false,
    agentUse,
    agentBot: agentUse === 'using' ? agent : null,
    computerUse: raw.computerUse ?? false,
    viewers: raw.viewers ?? 0,
  }
}

/**
 * R2: the same screen without the observation of who uses it. For a state no longer current (link dropped, a late
 * answer): enabled, connection, permissions and `computerUse` stay; who uses it becomes unknown, never Ready.
 */
export const screenWithoutAgent = (screen: ScreenState): ScreenState => ({ ...screen, agentUse: 'unavailable', agentBot: null })

/** The bot using this computer, only when this Space may know it. */
export const screenAgent = (screen: ScreenState | null | undefined): string | null =>
  screen?.agentUse === 'using' ? screen.agentBot : null

/**
 * R1 (decisão A): the tray's line once the screen is on, connected and allowed. Use by a bot comes first: unless
 * it is `idle` or a named `using`, it says Uso do computador indisponível, never a viewer count or Ready instead.
 */
export function screenUseSubtitle(screen: ScreenState, nameOf: (botId: string) => string | undefined): string {
  const agent = screenAgent(screen)
  const name = agent ? nameOf(agent) : undefined
  if (screen.agentUse !== 'idle' && !name) return SCREEN_USE_UNAVAILABLE_TEXT
  if (screen.viewers > 0) return screen.viewers === 1 ? 'Seu celular está vendo' : `${screen.viewers} aparelhos vendo`
  if (name) return `${name} está usando`
  const display = screen.displays.find((d) => d.main)?.name ?? screen.displays[0]?.name
  return display ? `Pronto · ${display}` : 'Pronto'
}

export interface Hello extends SpaceHello {
  spaceBirthContract?: number
  /** D543 E1: o Hub ativa o Berga em Space legado e exclui Space. Ausente nos Hubs antigos. */
  spaceLifecycleContract?: number
  spacesContract?: number
  spaceId?: string
  conversations?: import('./fork-spaces').SpaceConversation[]
  spaces?: import('./fork-spaces').SpacesSnapshot
  /** D115: o Hub valida e persiste avatarConfig. Ausente nos Hubs antigos. */
  avatarEditing?: boolean
  /** Contrato fechado Mexeriquinhas v2; independente do editor legado. */
  avatarConfigVersion?: number
  hostId: string
  name: string
  version: string
  os: string
  device?: string
  home?: string
  backends: Backend[]
  rev: number
  screen?: ScreenState
  urls?: string[]
  computerId?: string
  signKey?: string
  boxKey?: string
  minApp?: string
  cloud?: string | null
  /** Product analytics on this computer: on unless its owner turned it off. */
  analytics?: boolean
}

export interface PairingInfo {
  pairingUrl: string
  urls: string[]
  expiresAt?: number
}

export interface DirItem {
  name: string
  path: string
  isGit: boolean
}

export interface DirListing {
  path: string
  parent?: string | null
  isGit: boolean
  dirs: DirItem[]
}

/** A pending request as the computer shows it (loopback `accessRequests`). */
export interface AccessRequest {
  requestId: string
  deviceKey: string
  deviceName: string
  platform?: string | null
  email?: string | null
  /** The SAS; nil until the phone revealed its nonce (Approve stays disabled). */
  code?: string | null
  /** Binds a local approval to the exact request incarnation the person saw. */
  transcript?: string | null
  deadline?: number | null
  createdAt?: number | null
  expiresAt?: number | null
}

export type DecideResult =
  | { result: 'approved'; deviceKey: string; grantId: string; confirmCode?: string }
  | { result: 'denied' }
  | { result: 'refused'; reason: 'transcriptMismatch' | 'requestExpired' | 'consuming' | 'notFound' }
  | { result: 'unknown'; reason: 'superseded' | 'cloudUnreachable' }

/** A device the host authorized (loopback `devices`). */
export interface AuthorizedDevice {
  key: string
  name: string
  platform?: string | null
  /** `local` (QR) or `account`. */
  source: string
  scopes?: string[]
  leaseUntil?: number | null
  createdAt?: number | null
  lastSeenAt?: number | null
  connected?: boolean | null
}

export interface CloudOwner {
  userId: string
  email?: string | null
}

/** `code`: compare the 6-digit code at the computer; `auto`: let account devices in. */
export type AccountApproval = 'code' | 'auto'

export interface CloudStatus {
  enabled: boolean
  url?: string | null
  registered?: boolean | null
  connected?: boolean | null
  owner?: CloudOwner | null
  lastError?: string | null
  approval?: AccountApproval | null
}

export interface ClaimSignature {
  computerId: string
  signKey: string
  boxKey: string
  name: string
  platform: string
  device?: string | null
  version: string
  sig: string
}

/** A computer this device talks to. Holds no secrets: only pinned public keys and addresses. */
export interface Computer {
  id: string
  name: string
  signKey: string
  boxKey?: string | null
  urls: string[]
  cloud?: string | null
  device?: string | null
  color?: string | null
}

/** Create / update payload for a bot. */
export interface BotDraft {
  id?: string
  name: string
  description: string
  ownerPreferences?: string
  avatarColor: string
  avatarShape: string
  avatarConfig?: AvatarConfig
  mexeriquinha?: MexeriquinhaConfig
  title?: string
  backend: string
  forkEditor?: { engineAccountId: string | null }
  command?: string | null
  cwd: string
  permission: string
  permissionChosenByUser?: boolean
  model?: string | null
  modelOptions?: Record<string, string>
  fastMode?: boolean
  pinned?: boolean
  hidden?: boolean
  notify?: boolean | null
  connectors?: string[]
  logins?: string[]
  skills?: string[]
  computer?: boolean
  browserPreference?: BrowserPreference
  sharedMemory?: boolean
  harnessMemory?: boolean
}

export function draftOf(bot: Bot): BotDraft {
  return {
    id: bot.id,
    name: bot.name,
    description: bot.description,
    avatarColor: bot.avatarColor,
    avatarShape: bot.avatarShape,
    avatarConfig: bot.avatarConfig,
    ...(bot.mexeriquinha ? {mexeriquinha: bot.mexeriquinha} : {}),
    title: bot.title,
    backend: bot.backend,
    forkEditor: bot.forkEditor,
    command: bot.command ?? null,
    cwd: bot.managedWorkspace ? '' : bot.cwd,
    permission: bot.permission,
    model: bot.model ?? null,
    modelOptions: bot.modelOptions,
    fastMode: bot.fastMode,
    pinned: bot.pinned,
    hidden: bot.hidden,
    notify: bot.notify ?? null,
    connectors: bot.connectors,
    logins: bot.logins,
    skills: bot.skills,
    computer: bot.computer,
    browserPreference: bot.browserPreference,
  }
}

export interface GroupDraft {
  id?: string
  kind: 'group'
  name: string
  description: string
  members: string[]
  /** Ausente mantém o líder salvo; null tira; um id de `members` troca. */
  leader?: string | null
  pinned?: boolean
}
