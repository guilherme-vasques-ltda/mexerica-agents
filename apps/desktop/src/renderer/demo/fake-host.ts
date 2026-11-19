import { projetarAvatarLegado, validMexeriquinha } from '../../shared/fork-avatar-config'
import type { SessionDeleted } from '../../shared/fork-session-delete'
import type { CallError } from '@shared/ipc'
import type { RoutingPolicy, RoutingView } from '@shared/fork-roteamento'
import { ROUTING_PERSONAL } from '@shared/fork-roteamento'
import type { BotEditor, BotEditorSave } from '@shared/fork-bot-editor'
import type { Space, SpaceTile, SpacesSnapshot } from '@shared/fork-spaces'
import { isChat, isReplyable, normalizeBot, quoteOf, type Attachment, type Bot, type BotDraft, type Entry, type GroupDraft, type ScreenState } from '@shared/models'
import { FakeSkills } from './fake-skills'
import { FakeWorkplaces } from './fake-workplaces'
import { FakeWhatsapp } from './fake-whatsapp'
import type { InstalledSkillV2 } from '@shared/fork-skills'
import { abasDemo, comUso, conectoresAbasDemo, conectoresDemo, extensoesDemo, LIGADOS, LIGADOS_ABAS, loginsAbasDemo, loginsDemo, skillsAbasDemo } from './fake-extensoes'
import { FakeConector, conectorDemo, entradasConector } from './fake-conector'
import { relatorioDemo } from './fake-armazenamento'
import { FakeLimites } from './fake-limites'
import { entradasLongas, longaDemo } from './fake-longa'
import { escolhaDemo, perguntaDemo, perguntaId, responderDemo } from './fake-escolha'
import { arquivarDemo, cartaoContinuacao, FakeArquivar } from './fake-arquivar'
import { contasDemo, engineDemo, modelosDemo, removerContaDemo, seletorDemo, trocarDemo, visibilidadeDemo } from './fake-seletor'
import { FakeDelegacao } from './fake-delegacao'
import { FakeMemoria } from './fake-memoria'
import { FakeNuncaFazer } from './fake-nunca-fazer'
import { FakeBotFundo } from './fake-bot-fundo'
import { entradasTroca, trocaDemo } from './fake-troca-modelo'
import { BOT_COM_CONTA, BOT_SEM_CONTA, BOT_TROCA, CONTA_EMPRESA, CONTA_PESSOAL, CONTA_RESERVA, contasFalhaDemo, cotaContaDemo, entradasFalhaConta, falhaContaDemo, fallbackDemo, usoCotaDemo } from './fake-falha-conta'
import * as story from './story'
import { FakeContexto } from './fake-contexto'
import { FakeCusto } from './fake-custo'
import { atendeFila, BOT_FILA, chamarFila, entradasFila, filaDemo } from './fake-fila'
import { atendePedidos, BOT_PEDIDOS, chamarPedidos, entradasPedidos, pedidosDemo } from './fake-pedidos'
// Entrega 3 (MX-6): o cenário grande do seletor e de Contas de IA (`?seletor=1&modelos=e3`).
import { e3Demo, MODELOS_DOS_BOTS_E3 } from './fake-seletor'

// An in-memory codync-host for the website demo: the methods the renderer calls, with the
// host's JSON shapes (host/src/api/), and the events stream (hello, then catch-up in rev
// order, then live). Every mutation stamps a global rev, like the real host.

const TYPING_MS = 1500
const spacesE1Demo = () => new URLSearchParams(location.search).get('spaces') === 'e1'
/** UI-01: o anúncio do Hub em `sessions` (host/src/fork/sessoes/api.rs `SESSION_ACTIONS`). */
const SESSION_ACTIONS = ['create', 'send', ...(escolhaDemo() ? ['choice'] : [])]

export const DEMO_UNAVAILABLE = 'Not available in the web demo. Install Mexerica to use this.'

export class FakeHost {
  /** Controles sintéticos exclusivos do demo, para evidências do editor. */
  editorScenario: 'normal' | 'error' | 'load_error' | 'conflict' | 'apply_pending' | 'workspace' = 'normal'
  editorDelay = 0
  // Fixtures da rota-02: nenhuma chamada real, persistência apenas em memória.
  routingScenario: 'normal' | 'error' | 'conflict' = 'normal'
  routingDelay = 0
  readonly routingRequests: Record<string, unknown>[] = []
  private routing = new Map<string, { revision: number; policy: RoutingPolicy }>()
  private routingGeneral = { revision: 0, value: '' }
  readonly editorRequests: BotEditorSave[] = []
  readonly botUpdateRequests: Record<string, unknown>[] = []
  /** Muda só pelo controle do demo (`donnabotSkillsDemo.trocarHub`), para simular outro Hub. */
  hostId = 'demo-host'
  /** Catálogo D166 sintético (fake-skills.ts). */
  readonly skills = new FakeSkills()
  /** D371: regras de delegação e o quadro de Equipes com Marketing (fake-delegacao.ts). */
  readonly delegacao = new FakeDelegacao()
  /** D389: modelo da memória, modo de aprender e Sugestões (fake-memoria.ts). */
  readonly memoria = new FakeMemoria()
  /** MX-111: Nunca fazer por Space e bot, com o contrato do #700 (fake-nunca-fazer.ts). */
  readonly nuncaFazer = new FakeNuncaFazer()
  /** Entrega 4 (MX-7): Bot de fundo e Atividade, só com `?botfundo` (fake-bot-fundo.ts). */
  readonly botFundo = new FakeBotFundo()
  readonly workplaces = new FakeWorkplaces(() => this.hostId)
  /** whatsapp R5: ajustes em memória (fake-whatsapp.ts). */
  readonly whatsapp = new FakeWhatsapp(() => [...this.bots.values()].filter((b) => b.kind !== 'group').map((b) => b.id), () => this.primaryBotId, (botId) => this.emit({ type: 'whatsappChanged', botId }))
  /** R6a: cartões de conector sintéticos, só com `?conector` (fake-conector.ts). */
  readonly conector = new FakeConector()
  /** DON-907 C4: limites de uso e a espera em Crivo, só com `?limites` (fake-limites.ts). */
  readonly limites = new FakeLimites()
  /** DON-907 C5: encerrar, continuar e Arquivar sozinho, só com `?arquivar` (fake-arquivar.ts). */
  readonly arquivar = new FakeArquivar()
  /** DON-906 (C2): medidor de contexto fictício (fake-contexto.ts). */
  readonly contexto = new FakeContexto(() => this.hostId)
  /** DON-908 (C8): custo estimado fictício (fake-custo.ts). */
  readonly custo = new FakeCusto(() => this.hostId)
  private rev = 0
  private seq = 0
  private bots = new Map<string, Bot>()
  private entries: Entry[] = []
  private uploads = new FakeSpaceUploads()
  private listeners = new Set<(line: string) => void>()
  private turns = new Map<string, number>()
  private nextReply = new Map<string, number>()
  /** Already chosen, so the demo never asks for consent. */
  private analytics = true
  private primaryBotId: string | null = 'sprint'
  private deletedSessions = new Map<string, SessionDeleted>()
  private deleteDemo() { return new URLSearchParams(location.search).get('excluir') === '1' }
  private sessionsSeed = 0
  /** D543 E1 (`?spaces=e1`): Spaces do Hub de mentira — Pessoal, um com Berga e um legado sem. */
  private spacesState: { revision: number; spaces: Space[] } | null = null

  /** QA do seletor Navegador: tela projetada no hello; `null` mantém o comportamento de sempre. */
  screenDemo: ScreenState | null = null

  /** QA do seletor Navegador: ajusta campos do bot fictício e reemite o espelho. */
  fixtureBot(id: string, patch: Partial<Bot>) {
    const bot = this.bots.get(id)
    if (!bot) throw new Error(`fixture_bot: ${id} não existe`)
    Object.assign(bot, patch)
    this.emitBot(bot)
  }

  /** Cartão sintético da mesma forma que o Hub, exclusivo da fixture em memória. */
  fixtureApproval(botId: string) {
    if (!this.bots.has(botId)) throw fail('bot_not_found')
    return this.add({ botId, kind: 'permission', data: { author: botId, status: 'pending', title: 'Aprovar operação fictícia',
      options: [{ optionId: 'allow-once', name: 'Aprovar uma vez', kind: 'allow_once' }] } })
  }

  constructor(
    readonly version: string,
    now = Date.now(),
  ) {
    for (const b of story.bots) {
      this.bots.set(b.id, normalizeBot({ status: 'idle', ...b, manageTeam: b.id === this.primaryBotId, cwd: b.kind === 'group' ? '' : story.cwd, createdAt: now - 86_400_000, permission: b.permission ?? 'auto',
        // MX-8: `?sobrevoce` liga a memória compartilhada (menos no Crivo), para Ajustes → Memória contar quem lê.
        ...(new URLSearchParams(location.search).has('sobrevoce') && b.kind !== 'group' && b.id !== 'crivo' ? { sharedMemory: true } : {}) }))
    }
    if (spacesE1Demo()) this.seedSpaces()
    if (e3Demo()) for (const [id, model] of Object.entries(MODELOS_DOS_BOTS_E3)) Object.assign(this.bots.get(id) ?? {}, { model })
    // Revisão do app (Entrega 2): `?extensoes`, bots com skills, conectores e logins ligados, e o WhatsApp ligado.
    if (extensoesDemo()) {
      for (const [id, ligados] of Object.entries(LIGADOS)) Object.assign(this.bots.get(id)!, ligados)
      if (abasDemo()) Object.assign(this.bots.get('sprint')!, LIGADOS_ABAS)
      this.whatsapp.enabled = true
    }
    for (const s of story.entries) this.add({ botId: s.botId, kind: s.kind, data: s.data }, s.id, now - s.at * 60_000, false)
    if (conectorDemo()) entradasConector('prosa').forEach((e, i) => this.add({ botId: 'prosa', kind: 'permission', data: e.data }, e.id, now - (30 - i) * 60_000, false))
    const espera = this.limites.aviso(now)
    if (espera) this.add({ botId: 'crivo', kind: 'notice', data: espera }, 'espera-crivo', now - 60_000, false)
    if (falhaContaDemo()) {
      Object.assign(this.bots.get(BOT_COM_CONTA)!, { forkEditor: { engineAccountId: CONTA_EMPRESA } })
      Object.assign(this.bots.get(BOT_TROCA)!, { forkEditor: { engineAccountId: CONTA_RESERVA } })
      if (cotaContaDemo()) Object.assign(this.bots.get(BOT_SEM_CONTA)!, { backend: 'codex', forkEditor: { engineAccountId: CONTA_PESSOAL } })
      else entradasFalhaConta().forEach((e) => this.add({ botId: e.botId, kind: e.kind, data: e.data }, e.id, now - e.at * 60_000, false))
    }
    if (trocaDemo()) entradasTroca().forEach((e, i) => this.add({ botId: 'byte', kind: e.kind, data: e.data }, e.id, now - (1 - i) * 30_000, false))
    for (const s of story.sessionEntries) this.add({ botId: s.botId, kind: s.kind, data: s.data, sessionId: s.sessionId }, s.id, now - s.at * 60_000, false)
    if (escolhaDemo()) this.add(perguntaDemo(), perguntaId, now - 2 * 60_000, false)
    if (filaDemo()) {
      entradasFila().forEach((e, i) => this.add({ botId: BOT_FILA, kind: e.kind, data: e.data }, `fila-${i}`, now - (5 - i) * 60_000, false))
      Object.assign(this.bots.get(BOT_FILA)!, { status: 'working', activity: 'Lendo extrato-cartao-outubro.pdf', startedAt: now - 4 * 60_000, workingChat: BOT_FILA })
    }
    if (pedidosDemo()) {
      entradasPedidos().forEach((e, i) => this.add({ botId: BOT_PEDIDOS, kind: e.kind, data: e.data }, `pedido-${i}`, now - (8 - i) * 60_000, false))
      Object.assign(this.bots.get(BOT_PEDIDOS)!, { status: 'working', activity: 'Revisando src/pace.js', startedAt: now - 6 * 60_000, workingChat: BOT_PEDIDOS })
    }
    // D432: `?longa=N`, o Crivo com 34 mensagens e N respostas não lidas no Hub (sem N, 3).
    if (longaDemo()) { for (const e of entradasLongas(now)) this.add({ botId: 'crivo', kind: e.kind, data: e.data }, e.id, e.at, false); const v = new URLSearchParams(location.search).get('longa'); this.bots.get('crivo')!.unread = v ? Number(v) || 0 : 3 }
    this.entries.sort((a, b) => a.createdAt - b.createdAt).forEach((e, i) => { e.seq = i + 1 })
    for (const [id, leader] of Object.entries(story.leaders)) Object.assign(this.bots.get(id)!, { leader })
    this.sessionsSeed = now
    for (const b of this.bots.values()) {
      this.refreshPreview(b)
      b.rev = ++this.rev
    }
  }

  // MARK: events

  /** `GET /events?since=`: hello, everything after `since` in rev order, then live events. */
  subscribe(since: number, onData: (line: string) => void, spaceId?: string) {
    spaceId ??= this.spacesState ? ROUTING_PERSONAL : undefined
    if (spaceId) this.requireSpace(spaceId)
    const deliver = (line: string) => {
      if (!spaceId) return onData(line)
      const event = this.spaceEvent(spaceId, JSON.parse(line) as Record<string, unknown>)
      if (event) onData(JSON.stringify(event))
    }
    const catchUp: { rev: number; line: object }[] = [
      ...[...this.bots.values()].filter((b) => b.rev > since).map((b) => ({ rev: b.rev, line: { type: 'bot', rev: b.rev, bot: this.botJSON(b) } })),
      ...this.entries.filter((e) => e.rev > since).map((e) => ({ rev: e.rev, line: { type: 'entry', rev: e.rev, entry: e } })),
      ...[...this.deletedSessions.values()].map((line) => ({ rev: line.rev, line })),
    ].sort((a, b) => a.rev - b.rev)
    const hello = { type: 'hello', hostId: this.hostId, computerId: story.COMPUTER_ID, rev: this.rev, usage: story.usage(Date.now()), screen: this.screenDemo }
    let open = true
    const listener = (line: string) => open && deliver(line)
    setTimeout(() => {
      if (!open) return
      for (const item of [{ line: hello }, ...catchUp]) deliver(JSON.stringify(item.line))
      this.listeners.add(listener)
    }, 0)
    return () => {
      open = false
      this.listeners.delete(listener)
    }
  }

  private requireSpace(space: string) {
    if (space !== ROUTING_PERSONAL && !this.spacesState?.spaces.some(s => s.id === space)) throw fail('space_not_found')
  }

  /** Filtro da fixture para bots e conversas canônicas: não carimba linhas de outro Space. */
  private spaceEvent(space: string, event: Record<string, unknown>) {
    if (event.type === 'hello') {
      return { ...event, spaceId: space, spacesContract: 1,
        ...(this.spacesState ? { spaces: this.spacesSnapshot() } : {}), conversations: this.spaceConversations(space) }
    }
    if (event.type === 'spaces' || event.type === 'resync') return event
    const bot = event.bot as Partial<Bot> | undefined
    const entry = event.entry as Entry | undefined
    const session = event.session as { chatId?: string } | undefined
    const chat = bot?.id ?? entry?.botId ?? session?.chatId
    if (chat) {
      const source = bot ?? this.bots.get(chat)
      if (!source) return null // História de bot removido não vira Pessoal por omissão.
      const owner = source.spaceId ?? ROUTING_PERSONAL
      if (owner !== space) return null
      return { ...event, spaceId: space, ...(entry ? { entry: { ...entry, spaceId: space, conversationId: chat } } : {}) }
    }
    if (event.spaceId === space) return event
    return space === ROUTING_PERSONAL && ['usage', 'screen', 'accessRequests', 'cloud', 'functionModelsChanged'].includes(String(event.type))
      ? { ...event, spaceId: space } : null
  }

  private checkReferences(method: string, body: Record<string, unknown>) {
    if (!body.spaceId) return // Demo antiga sem contexto mantém os seus cenários.
    const space = String(body.spaceId)
    this.requireSpace(space)
    if (body.hubId !== undefined && body.hubId !== this.hostId) throw fail('hub_context_mismatch')
    const check = (id: unknown) => {
      if (typeof id !== 'string' || !id) return
      const bot = this.bots.get(id)
      if (!bot) throw fail('bot_not_found')
      if ((bot.spaceId ?? ROUTING_PERSONAL) !== space) throw fail('cross_space_reference')
    }
    for (const key of ['botId', 'chatId', 'peerId']) check(body[key])
    if (method === 'updateBot') check(body.id)
    if (['createBot', 'updateBot'].includes(method) && Array.isArray(body.members)) {
      for (const member of body.members) {
        const bot = typeof member === 'string' ? this.bots.get(member) : null
        if (bot && (bot.spaceId ?? ROUTING_PERSONAL) !== space) throw fail('cross_space_member')
      }
    }
    for (const key of ['entryId', 'replyTo']) {
      const id = body[key]
      if (typeof id !== 'string' || !id) continue
      const entry = this.entries.find(e => e.id === id)
      if (!entry) throw fail('entry_not_found')
      check(entry.botId)
    }
  }

  private emit(event: object) {
    const line = JSON.stringify(event)
    for (const l of this.listeners) l(line)
  }

  private emitBot(bot: Bot) {
    bot.rev = ++this.rev
    this.emit({ type: 'bot', rev: bot.rev, bot: this.botJSON(bot) })
    // A group shows its busy member's status: it changes with them.
    if (bot.kind !== 'group') for (const g of this.bots.values()) if (g.kind === 'group' && g.members.includes(bot.id)) this.emitBot(g)
  }

  private emitEntry(e: Entry) {
    e.rev = ++this.rev
    e.updatedAt = Date.now()
    this.emit({ type: 'entry', rev: e.rev, entry: e })
  }

  /** The host's `bot_json`: a group is busy while one of its members works in it. */
  private botJSON(b: Bot): Bot {
    if (b.kind !== 'group') return b
    const busy = b.members.map((m) => this.bots.get(m)).find((m) => m && m.status !== 'idle' && m.workingChat === b.id)
    return busy
      ? { ...b, status: busy.status, activity: busy.activity, startedAt: busy.startedAt, workingChat: b.id }
      : b
  }

  // MARK: entries

  /** Barra05: o catálogo de sessões do Hub (host/src/fork/sessoes/api.rs), com a principal sempre presente. */
  /** UI-01: as sessões criadas na demo (`sessionCreate`), com título pela 1ª mensagem. */
  private created: { chatId: string; id: string; title: string; at: number; revision: number; source?: string }[] = []

  private sessionsOf(chatId: string) {
    const row = (id: string, principal: boolean, title: string, at: number, closed = false) => ({
      id, chatId, principal, origin: { version: 1, kind: 'owner' }, sourceSession: null, parentSession: null, depth: 0, title, titleSource: principal ? 'principal' : 'owner',
      state: closed ? 'closed' : 'open', closedReason: closed ? 'arquivada' : null, closedAt: closed ? at : null, createdAt: at, lastUsedAt: at, revision: 1,
    })
    // UI01b / D372: a seleção própria de cada conversa, fictícia: o padrão inicial do bot, uma escolhida na própria
    // conversa e uma ilegível (o app mostra indisponível, sem copiar o padrão).
    const padrao = (revision: number) => (falhaContaDemo()
      ? { accountId: chatId === BOT_COM_CONTA ? CONTA_EMPRESA : chatId === BOT_TROCA ? CONTA_RESERVA : cotaContaDemo() && chatId === BOT_SEM_CONTA ? CONTA_PESSOAL : null, backend: chatId === BOT_COM_CONTA || chatId === BOT_TROCA ? 'claude' : 'codex', model: null, modelOptions: {}, fastMode: false, source: 'botDefault', revision }
      : { accountId: `ac_${'a'.repeat(64)}`, backend: 'codex', model: 'modelo-exemplo-a', modelOptions: { reasoning_effort: 'high' }, fastMode: false, source: 'botDefault', revision })
    const propria = (revision: number) => ({ accountId: `ac_${'b'.repeat(64)}`, backend: 'claude', model: 'modelo-exemplo-b', modelOptions: {}, fastMode: true, source: 'own', revision })
    const chat = this.bots.get(chatId)
    const extras = story.sessions.filter((s) => s.chatId === chatId)
    return [{ ...row(chatId, true, '', chat?.lastAt ?? this.sessionsSeed), engine: padrao(1) },
      ...extras.map((s, i) => ({ ...row(s.id, false, s.title, this.sessionsSeed - s.at * 60_000, s.closed), engine: i === 0 ? propria(1) : i === 1 ? null : padrao(1) })),
      ...this.created.filter((s) => s.chatId === chatId).map((s) => ({ ...row(s.id, false, s.title, s.at), titleSource: 'first_message', revision: s.revision, engine: padrao(s.revision),
        ...(s.source ? { origin: { version: 1, kind: 'continuation' }, sourceSession: s.source, titleSource: 'continuation' } : {}) }))]
      .filter((r) => !this.deletedSessions.has(r.id)).map((r) => this.arquivar.ajustar(r))
  }

  private add(e: Pick<Entry, 'botId' | 'kind' | 'data'> & { sessionId?: string }, id: string = crypto.randomUUID(), at = Date.now(), emit = true) {
    const scope = this.spacesState ? { spaceId: this.bots.get(e.botId)?.spaceId ?? ROUTING_PERSONAL, conversationId: e.botId } : {}
    const entry: Entry = { ...e, ...scope, id, seq: ++this.seq, rev: ++this.rev, turn: 1, createdAt: at, updatedAt: at }
    this.entries.push(entry)
    if (emit) {
      this.emit({ type: 'entry', rev: entry.rev, entry })
      const bot = this.bots.get(entry.botId)
      if (bot && isChat(entry)) {
        this.refreshPreview(bot)
        if (entry.kind !== 'user') bot.unread++
        this.emitBot(bot)
      }
    }
    return entry
  }

  private refreshPreview(b: Bot) {
    const last = this.entries.filter((e) => e.botId === b.id && (e.kind === 'user' || (e.kind === 'agent' && e.data.final))).at(-1)
    if (!last) return
    const text = last.data.text ?? ''
    b.lastAt = last.createdAt
    b.lastMessage = last.kind === 'user' ? `You: ${text}` : b.kind === 'group' ? `${this.bots.get(last.data.author ?? '')?.name ?? 'A bot'}: ${text}` : text
  }

  // MARK: turns

  private setStatus(botId: string, status: string, activity = '', chat: string | null = null) {
    const bot = this.bots.get(botId)
    if (!bot) return
    Object.assign(bot, { status, activity, startedAt: status === 'idle' ? null : Date.now(), workingChat: chat })
    this.emitBot(bot)
  }

  private pending(botId: string) {
    return this.entries.find((e) => e.kind === 'permission' && e.data.author === botId && e.data.status === 'pending') ?? null
  }

  private settle(botId: string) {
    const waiting = this.pending(botId)
    if (waiting) this.setStatus(botId, 'needsInput', `Needs approval: ${waiting.data.title ?? ''}`, waiting.botId)
    else this.setStatus(botId, 'idle')
  }

  /** One bot answers in a chat after a short "Typing…". */
  private async answer(responder: string, chat: string, text: string, sessionId?: string) {
    const turn = (this.turns.get(responder) ?? 0) + 1
    this.turns.set(responder, turn)
    this.setStatus(responder, 'working', 'Typing…', chat)
    await sleep(TYPING_MS + Math.random() * 600)
    if (this.turns.get(responder) !== turn) return
    this.add({ botId: chat, kind: 'agent', data: { text, final: true, author: responder }, ...(sessionId ? { sessionId } : {}) })
    this.settle(responder)
  }

  private replyText(botId: string) {
    if (this.pending(botId)) return story.waitingReply
    const pool = story.replies[botId] ?? ['Got it. I\'ll take a look and report back.']
    const i = this.nextReply.get(botId) ?? 0
    this.nextReply.set(botId, i + 1)
    return pool[i % pool.length]!
  }

  private async converse(chat: Bot, text: string, quoted: string | undefined, sessionId?: string) {
    let responders = [chat.id]
    if (chat.kind === 'group') {
      const members = chat.members.filter((m) => this.bots.has(m))
      const mentioned = members.filter((m) => text.toLowerCase().includes(`@${this.bots.get(m)!.name.toLowerCase()}`))
      // A room turn: who was asked, the bot whose message it replies to, or the first two members.
      responders = mentioned.length ? mentioned : quoted && members.includes(quoted) ? [quoted] : members.slice(0, 2)
    }
    for (const r of responders) await this.answer(r, chat.id, this.replyText(r), sessionId)
  }

  // MARK: Spaces (D543 E1, só `?spaces=e1`)

  private spacesSnapshot(): SpacesSnapshot & { hubId: string } {
    const state = this.spacesState!
    return { hubId: this.hostId, version: 1, personalSpaceId: ROUTING_PERSONAL, revision: state.revision, spaces: state.spaces.map((s) => ({ ...s })) }
  }
  private emitSpaces() {
    this.emit({ type: 'spaces', spaceId: ROUTING_PERSONAL, rev: ++this.rev, snapshot: this.spacesSnapshot() })
  }
  private spaceE1(id: string): Space {
    const space = this.spacesState?.spaces.find((s) => s.id === id)
    if (!space) throw fail('space_not_found: space não encontrado')
    return space
  }
  private seedSpaces() {
    const appearance = { wallpaper: 'none' as const, blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }
    const tile = (color: SpaceTile['color'], icon: string): SpaceTile => ({ color, style: 'filled', symbol: { kind: 'icon', value: icon } })
    const personal: Space = { id: ROUTING_PERSONAL, kind: 'personal', name: 'Pessoal', position: 0, revision: 1, principalBotId: 'sprint', tile: null, appearanceConfigured: false, appearance, owner: null }
    const estudio: Space = { id: 'space-estudio', kind: 'space', name: 'Estúdio', position: 1, revision: 1, principalBotId: null, tile: tile('violeta', 'briefcase'), appearanceConfigured: true, appearance, owner: null }
    const oficina: Space = { id: 'space-oficina', kind: 'space', name: 'Oficina', position: 2, revision: 1, principalBotId: null, tile: tile('cobre', 'code'), appearanceConfigured: true, appearance, owner: null }
    const jardim: Space = { ...oficina, id: 'space-jardim', name: 'Jardim', position: 3, tile: tile('musgo', 'leaf') }
    this.spacesState = { revision: 1, spaces: [personal, estudio, oficina, jardim] }
    const bot = this.saveBot({ name: 'Berga', spaceId: estudio.id, description: 'Berga do Estúdio.', mexeriquinha: { v: 2, estilo: 'simples', aparencia: { cor: 'roxo', olhos: 'oculos-redondos', roupa: 'colete', acessorio: 'nenhum', objeto: 'nenhum' } } } as unknown as BotDraft, 'berga-estudio')
    const jardimBot = this.saveBot({ name: 'Berga', spaceId: jardim.id, description: 'Berga do Jardim.' } as unknown as BotDraft, 'berga-jardim')
    jardim.principalBotId = jardimBot.id
    estudio.principalBotId = bot.id
    estudio.revision = 2
    this.spacesState.revision = 2
  }
  private spaceConversations(spaceId: string) {
    const space = this.spacesState?.spaces.find(s => s.id === spaceId)
    const rows = [...this.bots.values()].filter(b => (b.spaceId ?? ROUTING_PERSONAL) === spaceId).map(bot => ({
      ...this.spaceConversation({ ...space!, principalBotId: bot.id })!, primary: bot.id === space?.principalBotId,
    }))
    const base = rows[0]
    if (spaceId === 'space-estudio' && base) rows.push({ ...base, id: 'legacy-personal-estudio', botId: 'sprint', title: 'Conversa antiga', primary: false })
    return rows
  }
  private spaceConversation(space: Space) {
    if (!space.principalBotId) return null
    const bot = this.bots.get(space.principalBotId)
    return {
      id: space.principalBotId, spaceId: space.id, botId: space.principalBotId,
      title: bot?.name ?? 'Berga', revision: 1, stateRevision: 1, primary: true, closed: false,
      readRevision: 0, engineAccountId: null, engineSelection: null, accountRevision: null, continuity: null,
    }
  }

  // MARK: API

  async call(method: string, b: Record<string, unknown>): Promise<unknown> {
    if (this.nuncaFazer.atende(method)) {
      return this.nuncaFazer.call(method, b, (id) => { try { this.requireSpace(id); return true } catch { return false } },
        (id) => { const bot = this.bots.get(id); return bot ? bot.spaceId ?? ROUTING_PERSONAL : null })
    }
    this.checkReferences(method, b)
    if (method.startsWith('donnaWorkplace')) return this.workplaces.call(method, b)
    const str = (k: string) => (typeof b[k] === 'string' ? (b[k] as string) : '')
    if (this.skills.atende(method)) {
      const r = await this.skills.call(method, b, fail)
      // Só o desenho do "último uso" (precisa do Hub): o Hub de mentira acrescenta o campo na lista de instaladas.
      if (extensoesDemo() && method === 'skills' && r && typeof r === 'object' && 'items' in r) return { ...r, items: abasDemo() === 'vazio' ? [] : comUso([...(r as { items: InstalledSkillV2[] }).items, ...(abasDemo() === 'muitos' ? skillsAbasDemo() : [])]) }
      return r
    }
    if (this.whatsapp.atende(method)) return this.whatsapp.call(method, b, fail)
    if (this.conector.atende(method)) return this.conector.call(method, b, fail)
    if (this.limites.atende(method)) {
      const aviso = this.entries.find((e) => e.id === 'espera-crivo')
      return this.limites.call(method, b, aviso, (data) => { if (aviso) { aviso.data = data; this.emitEntry(aviso) } }, fail)
    }
    // Pedido de bot (aviso com delegationId + botMessage) segue o ramo do pedido; mensagem do dono segue o da fila —
    // como o Host decide pelo tipo do aviso dentro do queued_control, não pelo método.
    const alvo = this.entries.find((e) => e.id === b.entryId)
    const pedido = alvo?.kind === 'notice' && !!alvo.data.delegationId && !!alvo.data.botMessage
    // Métodos só da fila (queuedMove) não valem para o pedido, mas ele ainda responde (erro do Hub) em vez de cair noutro ramo.
    if (pedido && (atendePedidos(method) || !atendeFila(method))) {
      return chamarPedidos(method, b, {
        entries: this.entries,
        emit: (e) => this.emitEntry(e),
        fail,
      })
    }
    if (atendeFila(method)) {
      return chamarFila(method, b, {
        entries: this.entries,
        emit: (e) => this.emitEntry(e),
        notice: (botId, text) => this.add({ botId, kind: 'notice', data: { text, style: 'info', author: botId } }),
        working: (botId) => this.bots.get(botId)?.status === 'working',
        fail,
      })
    }
    if (this.arquivar.atende(method)) {
      return this.arquivar.call(method, b, {
        rows: (chatId) => this.sessionsOf(chatId),
        criar: (chatId, source) => {
          const created = { chatId, id: `demo-${crypto.randomUUID().slice(0, 8)}`, title: `Continuação de ${source.title}`.slice(0, 60), at: Date.now(), revision: ++this.rev, source: source.id }
          this.created.push(created)
          const handoff = this.add({ botId: chatId, kind: 'notice', data: cartaoContinuacao(source.title), sessionId: created.id })
          return { session: this.sessionsOf(chatId).find((r) => r.id === created.id)!, handoff }
        },
        emit: (session) => this.emit({ type: 'session', session } as never),
        rev: () => ++this.rev,
        fail,
      })
    }
    if (this.delegacao.atende(method)) return this.delegacao.call(method, b, [...this.bots.values()].filter((x) => x.kind !== 'group'), this.primaryBotId, fail)
    if (this.contexto.atende(method)) return this.contexto.call(method, b, fail)
    if (this.botFundo.atende(method)) return this.botFundo.call(method, b, fail)
    if (this.memoria.atende(method)) return this.memoria.call(method, b, fail)
    if (this.custo.atende(method)) return this.custo.call(b, fail)
    switch (method) {
      case 'donnaStorageReport': return relatorioDemo()
      case 'donnaEngineAccess': return { capability: 'engine.accounts', canManage: true }
      case 'donnaRouting':
      case 'donnaRoutingCheck': return this.routingResponse(str('botId'))
      case 'donnaRoutingSave': {
        this.routingRequests.push(structuredClone(b))
        if (this.routingDelay) await sleep(this.routingDelay)
        const before = this.routingResponse(str('botId'))
        if (this.routingScenario === 'error') throw fail('save_failed: erro sintético de roteamento')
        if (this.routingScenario === 'conflict' || b.expectedRevision !== before.policy.revision || b.expectedGeneralRevision !== before.generalGuide.revision) throw fail('settings_conflict: preferências alteradas')
        this.routing.set(str('botId'), { revision: before.policy.revision + 1, policy: structuredClone(b.policy as RoutingPolicy) })
        if (b.generalGuide !== this.routingGeneral.value) this.routingGeneral = { revision: this.routingGeneral.revision + 1, value: str('generalGuide') }
        return this.routingResponse(str('botId'))
      }
      case 'donnaBotEditor':
        if (this.editorScenario === 'load_error') throw fail('demo_timeout: leitura indisponível')
        return this.editorResponse(str('botId'))
      case 'donnaBotEditorSave': {
        const request = b as unknown as BotEditorSave
        this.editorRequests.push(structuredClone(request))
        if (this.editorDelay) await sleep(this.editorDelay)
        const current = this.editorResponse(request.botId)
        if (this.editorScenario === 'error') throw fail('save_failed: falha sintética')
        if (this.editorScenario === 'conflict' || request.expectedVersion !== current.settingsVersion) throw fail('settings_conflict: versão alterada')
        const patch = request.patch
        if (patch.name !== undefined && (!patch.name.trim() || Array.from(patch.name.trim()).length > 40 || /^(user|usuário|você|todos|all|everyone)$/iu.test(patch.name.trim()))) throw fail('invalid_field: nome inválido')
        if (patch.description !== undefined && Array.from(patch.description).length > 4000) throw fail('invalid_field: instruções longas')
        this.saveBot({ id: request.botId, ...patch, ...(patch.name === undefined ? {} : { name: patch.name.trim() }), ...(patch.description === undefined ? {} : { description: patch.description.trim() }) } as BotDraft)
        return this.editorResponse(request.botId)
      }
      case 'teamSettings':
        return { primaryBotId: this.primaryBotId, permission: 'ask', workspaceRoot: '/Users/usuario/code' }
      case 'setTeamSettings': {
        if (Object.hasOwn(b, 'primaryBotId')) {
          const id = typeof b.primaryBotId === 'string' ? b.primaryBotId : null
          if (id && (!this.bots.has(id) || this.bots.get(id)!.kind === 'group')) throw fail('Escolha um bot existente.')
          this.primaryBotId = id
          for (const bot of this.bots.values()) {
            const manageTeam = bot.id === id
            if (bot.manageTeam !== manageTeam) this.emitBot(Object.assign(bot, { manageTeam }))
          }
        }
        return this.call('teamSettings', {})
      }
      case 'donnaSpaces':
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        return this.spacesSnapshot()
      case 'donnaSpaceCreate': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const space: Space = { id: `space-${crypto.randomUUID().slice(0, 8)}`, kind: 'space', name: String(b.name ?? '').slice(0, 60) || 'Space', position: state.spaces.length, revision: 1, principalBotId: null, tile: (b.tile as SpaceTile) ?? null, appearanceConfigured: false, appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null }
        state.spaces.push(space); state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaSpaceUpdate': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const space = this.spaceE1(String(b.spaceId))
        const patch = (b.patch ?? {}) as { name?: string; tile?: SpaceTile | null }
        if (patch.name !== undefined) space.name = patch.name.slice(0, 60)
        if (patch.tile !== undefined) space.tile = patch.tile
        space.revision++; state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaSpaceReorder': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const ids = (b.ids as string[]) ?? []
        const byId = new Map(state.spaces.map((s) => [s.id, s]))
        const ordered = ids.map((id) => byId.get(id)).filter((s): s is Space => !!s)
        for (const s of state.spaces) if (!ids.includes(s.id)) ordered.push(s)
        state.spaces = ordered.map((s, i) => ({ ...s, position: i }))
        state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaOwnerPresentation': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const personal = state.spaces.find((s) => s.id === ROUTING_PERSONAL)
        if (personal) personal.owner = (b.owner as Space['owner']) ?? null
        state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaSpaceBirth': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const principal = (b.principal ?? {}) as { name?: string; backend?: string; engineAccountId?: string; appearance?: unknown }
        if (principal.name !== undefined && principal.name !== 'Berga') throw fail('space_principal_name_fixed: o nome do Berga é sempre Berga')
        const space: Space = { id: `space-${crypto.randomUUID().slice(0, 8)}`, kind: 'space', name: String(b.name ?? '').slice(0, 60) || 'Space', position: state.spaces.length, revision: 1, principalBotId: null, tile: (b.tile as SpaceTile) ?? null, appearanceConfigured: false, appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null }
        const bot = this.saveBot({ name: 'Berga', spaceId: space.id, description: 'Berga do Space.', ...(principal.appearance ? { mexeriquinha: principal.appearance } : {}) } as unknown as BotDraft)
        space.principalBotId = bot.id; space.revision = 2
        state.spaces.push(space); state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaSpacePrincipal': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        if (this.spacesState.revision !== 1 && b.expectedRevision !== undefined && b.expectedRevision !== this.spacesState.revision) throw fail('settings_conflict: lista mudou')
        const space = this.spaceE1(String(b.spaceId))
        if (space.kind !== 'space') throw fail('personal_space_immutable: o Pessoal não recebe principal')
        if (space.principalBotId) throw fail('space_principal_exists: este Space já tem um Berga')
        const principal = (b.principal ?? {}) as { name?: string; appearance?: unknown }
        if (principal.name !== undefined && principal.name !== 'Berga') throw fail('space_principal_name_fixed: o nome do Berga é sempre Berga')
        const bot = this.saveBot({ name: 'Berga', spaceId: space.id, description: 'Berga do Space.', ...(principal.appearance ? { mexeriquinha: principal.appearance } : { mexeriquinha: { v: 2, estilo: 'simples', aparencia: { cor: 'laranja', olhos: 'ponto', roupa: 'nenhuma', acessorio: 'nenhum', objeto: 'nenhum' } } }) } as unknown as BotDraft)
        space.principalBotId = bot.id; space.revision++; this.spacesState.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaSpaceDelete': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const state = this.spacesState
        if (b.expectedRevision !== state.revision) throw fail('settings_conflict: lista mudou')
        const space = this.spaceE1(String(b.spaceId))
        if (space.kind !== 'space') throw fail('personal_space_immutable: o Pessoal não pode ser excluído')
        if (String(b.confirmName ?? '') !== space.name) throw fail('space_delete_confirmation_mismatch: o nome digitado não confere')
        const busy = [...this.bots.values()].filter((bot) => bot.spaceId === space.id && bot.status === 'working').map((bot) => ({ botId: bot.id, name: bot.name }))
        if (busy.length) return { hubId: this.hostId, code: 'space_busy', busy }
        state.spaces = state.spaces.filter((s) => s.id !== space.id)
        for (const bot of [...this.bots.values()]) if (bot.spaceId === space.id) {
          this.bots.delete(bot.id)
          this.emit({ type: 'bot', rev: ++this.rev, bot: { id: bot.id, spaceId: bot.spaceId ?? ROUTING_PERSONAL, rev: this.rev, deleted: true } })
        }
        state.revision++
        this.emitSpaces()
        return this.spacesSnapshot()
      }
      case 'donnaConversations': {
        if (!this.spacesState) throw fail('spaces_unavailable: spaces indisponíveis')
        const space = this.spaceE1(String(b.spaceId))
        return { hubId: this.hostId, spaceId: space.id, conversations: this.spaceConversations(space.id), principal: space.principalBotId ? { id: space.principalBotId, name: this.bots.get(space.principalBotId)?.name ?? 'Berga' } : null }
      }
      case 'donnaConversationHistory': {
        const conversationId = str('conversationId')
        const space = this.spaceE1(String(b.spaceId))
        const conversation = this.spaceConversations(space.id).find(c => c.id === conversationId)
        if (!conversation) throw fail('conversation_not_found')
        const entries = conversationId === 'legacy-personal-estudio' ? [{ id: 'legacy-estudio-1', botId: conversationId, conversationId, spaceId: space.id,
          seq: 1, rev: 1, kind: 'user', turn: 0, createdAt: Date.now() - 86_400_000, updatedAt: 0,
          data: { text: 'Mensagem antiga desta conversa do Estúdio. Histórico preservado, sem envio novo.' } }] : []
        return { hubId: this.hostId, context: { version: 1, spaceId: space.id, conversationId, botId: conversation.botId, conversationRevision: 1 }, entries, hasMore: false }
      }
      case 'donnaConversationRead': {
        const space = this.spaceE1(String(b.spaceId))
        const conversation = this.spaceConversation(space)
        if (!conversation) throw fail('conversation_not_found: conversa não encontrada')
        return { conversation }
      }
      case 'hello':
        return {
          ...((arquivarDemo() || this.deleteDemo() || this.spacesState) && new URLSearchParams(location.search).get('spaces') !== 'legacy' ? { spacesContract: 1, spaceBirthContract: this.spacesState ? 1 : undefined, spaceLifecycleContract: this.spacesState && new URLSearchParams(location.search).get('lifecycle') !== 'old' ? 1 : undefined, spaceId: typeof b.spaceId === 'string' ? b.spaceId : ROUTING_PERSONAL, conversations: this.spaceConversations(String(b.spaceId ?? ROUTING_PERSONAL)), spaces: this.spacesState ? this.spacesSnapshot() : { version: 1, personalSpaceId: ROUTING_PERSONAL, revision: 1, spaces: [{ id: ROUTING_PERSONAL, kind: 'personal', name: 'Pessoal', position: 0, revision: 1, tile: null, appearanceConfigured: false, appearance: { wallpaper: 'cidade', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null }] } } : {}),
          hostId: this.hostId, computerId: story.COMPUTER_ID, signKey: '', boxKey: '', cloud: null, name: story.HOST_NAME,
          avatarEditing: true, avatarConfigVersion: 2, version: this.version, minApp: this.version, os: 'macos', device: 'MacBookPro', home: '/Users/usuario', rev: this.rev, analytics: this.analytics,
          urls: [], backends: [
            { id: 'claude', name: 'Claude Code', available: true, installed: true, signedIn: true, command: 'claude', installHint: '', registry: 'claude-acp' },
            { id: 'codex', name: 'Codex', available: true, installed: true, signedIn: true, command: 'codex', installHint: '', registry: 'codex-acp' },
            ...(e3Demo() ? [{ id: 'opencode', name: 'OpenCode', available: true, installed: true, signedIn: true, command: 'opencode', installHint: '', registry: 'opencode' }] : []),
          ],
        }
      case 'accessRequests':
        return { requests: [] }
      case 'cloudStatus':
        return { enabled: false }
      case 'usage':
        return story.usage(Date.now())
      case 'history':
        return { entries: this.entries.filter(e => e.botId === str('botId')) }
      case 'upload':
        return this.uploads.append(str('spaceId') || ROUTING_PERSONAL, str('botId'), b)
      case 'readUpload':
        return this.uploads.read(str('spaceId') || ROUTING_PERSONAL, str('botId'), str('uploadId'), Number(b.offset ?? 0))
      case 'sessionChoiceAnswer': {
        const pergunta = this.entries.find((e) => e.id === perguntaId)
        return responderDemo(b, this.sessionsOf(str('botId')).find((r) => r.id === b.sessionId), escolhaDemo() ? pergunta : undefined, (data, resposta) => {
          Object.assign(pergunta!, { data, rev: ++this.rev, updatedAt: Date.now() })
          this.emitEntry(pergunta!)
          return this.add(resposta)
        }, fail)
      }
      case 'sessionDelete': {
        if (!this.deleteDemo()) throw fail('unknown method sessionDelete')
        if (b.hubId !== this.hostId || b.spaceId !== ROUTING_PERSONAL) throw fail('cross_space_reference')
        const previous = this.deletedSessions.get(str('sessionId'))
        if (previous) {
          if (previous.chatId !== b.botId) throw fail('unknown_session')
          return previous
        }
        const row = this.sessionsOf(str('botId')).find((r) => r.id === b.sessionId)
        if (!row) throw fail('unknown_session')
        if (row.principal) throw fail('principal_not_deletable')
        if (row.revision !== b.expectedRevision) throw fail('session_revision_conflict')
        if (row.id === 'byte-bench' || this.entries.some((e) => e.sessionId === row.id && (e.data.status === 'queued' || e.data.status === 'pending'))) throw fail('session_busy')
        this.entries = this.entries.filter((e) => e.botId !== row.chatId || e.sessionId !== row.id)
        const event: SessionDeleted = { type: 'sessionDeleted', spaceId: ROUTING_PERSONAL, chatId: row.chatId, sessionId: row.id, rev: ++this.rev }
        this.deletedSessions.set(row.id, event)
        this.emit(event)
        return event
      }
      case 'sessions':
        if (seletorDemo()) return { sessionActions: [...SESSION_ACTIONS, 'engine', ...this.arquivar.actions(), ...(this.deleteDemo() ? ['delete'] : [])], sessions: this.sessionsOf(str('botId')).map((r) => ({ ...r, engine: engineDemo(`${r.chatId}/${r.id}`, 1) })) }
        return { sessionActions: [...SESSION_ACTIONS, ...this.arquivar.actions(), ...(this.deleteDemo() ? ['delete'] : [])], sessions: this.sessionsOf(str('botId')) }
      case 'donnaEngineAccounts':
        if (falhaContaDemo()) return contasFalhaDemo()
        if (!seletorDemo() && !spacesE1Demo()) throw fail(DEMO_UNAVAILABLE)
        return contasDemo()
      case 'donnaEngineAccountUsage':
        if (!cotaContaDemo()) throw fail(DEMO_UNAVAILABLE)
        return usoCotaDemo(str('engineAccountId'))
      case 'donnaEngineFallback':
      case 'donnaEngineFallbackSave':
      case 'donnaEngineFallbackAction':
        if (!cotaContaDemo()) throw fail(DEMO_UNAVAILABLE)
        return fallbackDemo(method, b)
      case 'donnaEngineAccountRemove':
        if (!seletorDemo()) throw fail(DEMO_UNAVAILABLE)
        removerContaDemo(str('engineAccountId'))
        return {}
      case 'donnaEngineAccountModelVisibility': {
        if (!seletorDemo() && !spacesE1Demo()) throw fail(DEMO_UNAVAILABLE)
        const engineAccountId = str('engineAccountId')
        visibilidadeDemo(engineAccountId, str('modelId'), b.hidden === true)
        const account = contasDemo().accounts.find((a) => a.id === engineAccountId)
        if (!account) throw fail('unknown account')
        return account
      }
      case 'sessionEngineSet': {
        if (!seletorDemo()) throw fail('unknown method sessionEngineSet')
        const chatId = str('botId')
        const row = this.sessionsOf(chatId).find((r) => (b.sessionId ? r.id === b.sessionId : r.principal))
        if (!row) throw fail('unknown_session')
        const key = `${chatId}/${row.id}`
        const swap = trocarDemo(key, Number(b.expectedRevision), b.engine as Parameters<typeof trocarDemo>[2], ++this.rev)
        const session = { ...row, revision: this.rev, engine: engineDemo(key, this.rev) }
        this.emit({ type: 'session', rev: this.rev, session } as never)
        return { session, switch: swap }
      }
      case 'sessionCreate': {
        const chatId = str('botId')
        if (!this.bots.has(chatId)) throw fail('unknown bot')
        const created = { chatId, id: `demo-${crypto.randomUUID().slice(0, 8)}`, title: '', at: Date.now(), revision: ++this.rev }
        this.created.push(created)
        const session = this.sessionsOf(chatId).find((s) => s.id === created.id)!
        this.emit({ type: 'session', session })
        return { session }
      }
      case 'send': {
        const chat = this.bots.get(str('botId'))
        if (!chat) throw fail('That bot was deleted.')
        // UI-01: `sessionId` de uma extra, conferido como no Hub (api.rs `send_lane`).
        const sessionId = str('sessionId') && str('sessionId') !== chat.id ? str('sessionId') : undefined
        if (sessionId) {
          const session = this.sessionsOf(chat.id).find((s) => s.id === sessionId)
          if (!session) throw fail('unknown_session')
          if (session.state !== 'open') throw fail('session_closed')
        }
        const target = this.entries.find((e) => e.id === str('replyTo') && e.botId === chat.id && isReplyable(e))
        const replyTo = target && quoteOf(target)
        const existing = typeof b.clientNonce === 'string' && b.clientNonce ? this.entries.find(e => e.botId === chat.id && e.kind === 'user' && e.data.clientNonce === b.clientNonce) : undefined
        if (existing) return { entry: existing }
        const attachments = Array.isArray(b.attachments) ? b.attachments.map(id => this.uploads.attachment(str('spaceId') || ROUTING_PERSONAL, chat.id, String(id))) : []
        const entry = this.add({ botId: chat.id, kind: 'user', data: { text: str('text'), status: 'sent', clientNonce: str('clientNonce'), ...(attachments.length ? { attachments } : {}), ...(replyTo ? { replyTo } : {}) }, ...(sessionId ? { sessionId } : {}) })
        const fresh = this.created.find((s) => s.id === sessionId && !s.title)
        if (fresh) {
          fresh.title = str('text').split('\n')[0]!.slice(0, 60)
          fresh.revision = ++this.rev
          this.emit({ type: 'session', session: this.sessionsOf(chat.id).find((s) => s.id === fresh.id)! })
        }
        void this.converse(chat, str('text'), replyTo?.author, sessionId)
        return { entry }
      }
      case 'markRead': {
        const bot = this.bots.get(str('botId'))
        if (bot && bot.unread > 0) {
          bot.unread = 0
          this.emitBot(bot)
        }
        return {}
      }
      case 'respondPermission':
        return this.respond(str('entryId'), str('optionId') || null)
      case 'react': {
        const e = this.entries.find((x) => x.id === str('entryId'))
        if (!e) throw fail('That message is gone.')
        const reactions = new Set(e.data.reactions ?? [])
        if (!reactions.delete(str('emoji'))) reactions.add(str('emoji'))
        e.data = { ...e.data, reactions: [...reactions] }
        this.emitEntry(e)
        return { entry: e }
      }
      case 'stop': {
        const id = str('botId')
        this.turns.set(id, (this.turns.get(id) ?? 0) + 1)
        this.settle(id)
        return {}
      }
      case 'newSession':
        return {}
      case 'createBot':
        return { bot: this.saveBot(b as unknown as BotDraft | GroupDraft) }
      case 'updateBot':
        this.botUpdateRequests.push(structuredClone(b))
        if ('cwd' in b && this.editorScenario === 'workspace') this.editorScenario = 'normal'
        return { bot: this.saveBot(b as unknown as BotDraft | GroupDraft) }
      case 'reorderBots': {
        ;(b.ids as string[]).forEach((id, position) => {
          const bot = this.bots.get(id)
          if (bot) this.emitBot(Object.assign(bot, { position }))
        })
        return {}
      }
      case 'deleteBot': {
        const id = str('botId')
        const spaceId = this.bots.get(id)?.spaceId ?? ROUTING_PERSONAL
        this.bots.delete(id)
        this.emit({ type: 'bot', rev: ++this.rev, bot: { id, spaceId, rev: this.rev, deleted: true } })
        return {}
      }
      case 'refreshBackends':
        return { backends: ((await this.call('hello', {})) as { backends: unknown }).backends }
      case 'listDirs':
        return { path: str('path') || '/Users/usuario', parent: '/Users/usuario', isGit: false, dirs: str('path') === story.cwd ? [] : [{ name: 'pace', path: story.cwd, isGit: true }] }
      case 'agentModels':
        if (seletorDemo()) return modelosDemo(str('backend'))
        return { currentModelId: str('backend') === 'codex' ? 'gpt-5.3-codex' : 'claude-sonnet-4-6', fastMode: true, models: [
          { id: str('backend') === 'codex' ? 'gpt-5.3-codex' : 'claude-sonnet-4-6', name: str('backend') === 'codex' ? 'Codex (demo)' : 'Sonnet (demo)', options: [{ id: 'effort', name: 'Raciocínio', default: 'high', values: [{ id: 'high', name: 'Alto' }, { id: 'medium', name: 'Médio' }] }] },
          { id: 'demo-alternativo', name: 'Alternativo (demo)' },
        ] }
      case 'memory':
        return this.memoria.view({
          shared: true, yours: { entries: this.memoria.compartilhadas(), used: this.memoria.compartilhadas().length, cap: 400 }, bot: { entries: [], used: 0, cap: 1200 },
          dreams: [], projectMemory: null, memoryRev: 0,
        })
      case 'routines': {
        const demo = demoRoutines()
        return { routines: demo.routines.filter((r) => r.botId === str('botId')), runs: demo.runs.filter((r) => r.botId === str('botId')).reverse() }
      }
      case 'routinesOverview':
        return { now: Date.now(), teams: [], dense: [], ...demoRoutines() }
      case 'connectors':
        if (abasDemo()) return conectoresAbasDemo(abasDemo()!)
        if (extensoesDemo()) return conectoresDemo()
        return { items: [{ id: 'demo-connector', name: 'Documentos (demo)', description: 'Conector sintético', kind: 'http', url: 'https://example.invalid/mcp' }] }
      case 'credentialLogins':
        if (abasDemo()) return loginsAbasDemo(abasDemo()!)
        if (extensoesDemo()) return loginsDemo()
        return { items: [{ id: 'demo-login', site: 'example.invalid', username: 'demo', label: 'Conta de teste (demo)' }] }
      case 'marketConnectors':
      case 'connectorInfo':
        throw fail('mcp_registry_unavailable: MCP Registry foi retirado. Use a conexão MCP nativa.')
      case 'devices':
        return { devices: [] }
      case 'voiceStatus':
        return { providers: [] }
      case 'credentialStatus':
        return { provider: '', onePasswordConnected: extensoesDemo() }
      case 'setAnalytics':
        this.analytics = b.enabled === true
        return { enabled: this.analytics }
      case 'track':
        return {}
      default:
        throw fail(DEMO_UNAVAILABLE)
    }
  }

  private async respond(entryId: string, optionId: string | null) {
    const e = this.entries.find((x) => x.id === entryId)
    if (!e || e.data.status !== 'pending') return {}
    const option = e.data.options?.find((o) => o.optionId === optionId)
    const allowed = !!option && option.kind.startsWith('allow')
    e.data = { ...e.data, status: optionId ? 'answered' : 'cancelled', selected: optionId ?? undefined }
    this.emitEntry(e)
    const author = e.data.author ?? e.botId
    void (async () => {
      this.setStatus(author, 'working', allowed ? 'Editing src/pace.js' : 'Typing…', e.botId)
      await sleep(TYPING_MS)
      this.add({ botId: e.botId, kind: 'agent', data: { text: allowed ? story.approvedReply : story.deniedReply, final: true, author } })
      this.settle(author)
    })()
    return {}
  }

  private saveBot(draft: BotDraft | GroupDraft, fixtureId?: string) {
    const existing = draft.id ? this.bots.get(draft.id) : undefined
    // A edição legada explícita invalida a escolha avançada, como no Hub.
    if (existing && Object.hasOwn(draft, 'avatarConfig') && !Object.hasOwn(draft, 'mexeriquinha')) delete existing.mexeriquinha
    const appearance = (draft as Partial<Bot>).mexeriquinha
    const bot = normalizeBot({
      ...(existing ?? { createdAt: Date.now(), cwd: 'cwd' in draft ? draft.cwd || story.cwd : '', status: 'idle' as const }),
      ...(draft as Partial<Bot>),
      ...(validMexeriquinha(appearance) ? { avatarConfig: projetarAvatarLegado(appearance) } : {}),
      id: existing?.id ?? fixtureId ?? crypto.randomUUID(),
    })
    if (existing) Object.assign(existing, bot)
    const saved = existing ?? bot
    this.bots.set(saved.id, saved)
    this.emitBot(saved)
    return this.botJSON(saved)
  }

  private routingResponse(botId: string): RoutingView {
    if (!this.bots.has(botId)) throw fail('bot_not_found: bot não encontrado')
    const saved = this.routing.get(botId) ?? { revision: 0, policy: { mode: 'fixed' as const, guide: '', reserves: [] } }
    return structuredClone({ botId, hubId: this.hostId, spaceId: ROUTING_PERSONAL,
      policy: { schemaVersion: 1, revision: saved.revision, value: saved.policy },
      generalGuide: { schemaVersion: 1, ...this.routingGeneral }, effectiveGuide: saved.policy.guide || this.routingGeneral.value,
      auto: { available: false, reason: 'executor_and_scope_unproven' }, reservesExecution: 'unavailable', execution: 'existing_fixed',
      fixedCheck: { status: 'unavailable', reason: 'legacy_preserved' }, accounts: [
        { id: `ac_${'a'.repeat(64)}`, label: 'Conta de exemplo A', backend: 'codex', brand: 'OpenAI', billing: 'paid_api', state: 'connected', revision: 1, origin: 'managed', botIds: [], models: [{ id: 'modelo-exemplo-a', options: [{ id: 'reasoning_effort', values: ['high', 'medium'], default: 'high' }] }] },
        { id: `ac_${'b'.repeat(64)}`, label: 'Conta de exemplo B', backend: 'claude', brand: 'Anthropic', billing: 'subscription', state: 'connected', revision: 2, origin: 'managed', botIds: [], models: [{ id: 'modelo-exemplo-b', options: [] }] },
      ],
    })
  }

  private editorResponse(botId: string): BotEditor {
    const bot = this.bots.get(botId)
    if (!bot || bot.kind === 'group') throw fail('bot_not_found: bot não encontrado')
    const profile = { name: bot.name, description: bot.description, avatarConfig: bot.avatarConfig ?? null, ...(bot.mexeriquinha ? { mexeriquinha: bot.mexeriquinha } : {}), title: bot.title, backend: bot.backend, model: bot.model ?? null, modelOptions: bot.modelOptions ?? {}, forkEditor: bot.forkEditor, browserPreference: bot.browserPreference }
    return {
      botId, settingsVersion: JSON.stringify([profile, bot.backend, bot.command, bot.model, bot.modelOptions, bot.fastMode, bot.cwd, bot.permission, bot.connectors, bot.logins, bot.skills, bot.notify, bot.pinned, bot.position, bot.hidden, bot.computer, bot.browserPreference, bot.sharedMemory, bot.harnessMemory, this.primaryBotId]), profile,
      capabilities: { editableFields: ['name', 'description', 'avatarConfig', 'mexeriquinha', 'title', 'browserPreference', ...(falhaContaDemo() ? ['forkEditor' as const] : [])], unavailableSections: falhaContaDemo() ? ['autonomy', 'folder', 'tools', 'memory', 'voice', 'routines', 'export'] : ['engine', 'autonomy', 'folder', 'tools', 'memory', 'voice', 'routines', 'export'] },
      permissions: { canEdit: true, canDelete: true }, costInfo: null, speedStatus: null,
      engineAccounts: falhaContaDemo() ? contasFalhaDemo().accounts : [],
      engineRuntime: falhaContaDemo() && bot.forkEditor?.engineAccountId ? { engineAccountId: bot.forkEditor.engineAccountId, state: contasFalhaDemo().accounts.find((a) => a.id === bot.forkEditor?.engineAccountId)?.state ?? 'unverified', revision: 1 } : null,
      application: this.editorScenario === 'apply_pending' ? 'apply_pending' : 'next_turn',
      workspaceIssue: this.editorScenario === 'workspace' ? 'Revise a pasta. A pasta não existe ou não está acessível no Hub. Escolha outra pasta nas configurações do bot.' : null,
      message: 'Aplica no próximo turno.',
    }
  }
}


/** Paridade da demo com api/bots.rs e chat/uploads.rs; só bytes em memória, sem disco. */
class FakeSpaceUploads {
  private files = new Map<string, { name: string; data: Uint8Array; done: boolean }>()
  private key(space: string, bot: string, id: string) { return JSON.stringify([space, bot, id]) }

  append(space: string, bot: string, body: Record<string, unknown>): { attachment: Attachment | null } {
    const id = String(body.uploadId ?? '')
    const name = String(body.name ?? '')
    const offset = body.offset ?? 0
    if (!id || !name || /[\\/\u0000]/.test(name) || !Number.isSafeInteger(offset) || Number(offset) < 0) throw new Error('invalid_upload')
    const data = Uint8Array.from(atob(String(body.data ?? '')), c => c.charCodeAt(0))
    const key = this.key(space, bot, id)
    const previous = this.files.get(key)
    if (previous && previous.name !== name) throw new Error('upload_name_changed')
    const size = previous?.data.length ?? 0
    const end = Number(offset) + data.length
    if (end > 100 * 1024 * 1024) throw new Error('upload_too_large')
    if (offset !== size && end !== size) throw new Error('upload_out_of_order')
    let bytes = previous?.data ?? new Uint8Array()
    if (offset === size) {
      bytes = new Uint8Array(end)
      if (previous) bytes.set(previous.data)
      bytes.set(data, size)
    }
    const file = { name, data: bytes, done: body.done === true }
    this.files.set(key, file)
    return { attachment: file.done ? { id, name, size: bytes.length } : null }
  }

  attachment(space: string, bot: string, id: string): Attachment {
    const file = this.files.get(this.key(space, bot, id))
    if (!file?.done) throw new Error('unknown_upload')
    return { id, name: file.name, size: file.data.length }
  }

  read(space: string, bot: string, id: string, offset: number) {
    const file = this.files.get(this.key(space, bot, id))
    if (!file?.done) throw new Error('unknown_upload')
    if (!Number.isSafeInteger(offset) || offset < 0) throw new Error('invalid_offset')
    let text = ''
    for (const byte of file.data.subarray(offset, offset + 384 * 1024)) text += String.fromCharCode(byte)
    return { data: btoa(text), size: file.data.length }
  }
}

function fail(message: string): CallError {
  return { status: 400, message }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** I-023: exemplos sintéticos de cada desfecho, para a prova visual de Rotinas (lista, grade e painel do bot). */
function demoRoutines() {
  const day = new Date().setHours(0, 0, 0, 0)
  const at = (hours: number) => day + hours * 3_600_000
  const cron = (minute: number, hour: number) => [{ type: 'cron', expression: `${minute} ${hour} * * *`, timeZone: 'America/Sao_Paulo' }]
  const base = { enabled: true, timeoutSeconds: 1800, lastError: null, teamId: '', runsOn: null, createdAt: day, updatedAt: day }
  const cases: [string, string, number, string, object | null][] = [
    ['Caixa de entrada', 'sprint', 7, 'succeeded', { version: 1, kind: 'noNews', source: 'declared' }],
    ['Relatório de vendas', 'byte', 8, 'succeeded', { version: 1, kind: 'reported', source: 'declared', summary: 'Vendas subiram 4% na semana.', visibleReply: true }],
    ['Renovar certificado', 'crivo', 9, 'failed', { version: 1, kind: 'needsOwner', source: 'declared', summary: 'Falta aprovar o pagamento da renovação.' }],
    ['Backup das fotos', 'prosa', 10, 'failed', { version: 1, kind: 'failed', source: 'observed', reason: 'timeout' }],
    ['Resumo do dia', 'sprint', 11, 'succeeded', { version: 1, kind: 'unconfirmed', source: 'none', reason: 'noDeclaration' }],
    ['Conferir agenda', 'byte', 12, 'succeeded', null],
  ]
  const routines = cases.map(([name, botId, hour], i) => ({
    ...base, id: `demo-routine-${i}`, botId, name, instruction: name, triggers: cron(0, hour),
    summary: `Todo dia às ${hour}:00`, triggerDescriptions: [`Todo dia às ${hour}:00`], nextRunAt: at(24 + hour),
  }))
  const runs = cases.map(([, botId, hour, status, outcome], i) => ({
    id: `demo-run-${i}`, routineId: `demo-routine-${i}`, botId, status, createdAt: at(hour), startedAt: at(hour), finishedAt: at(hour) + 120_000,
    detail: null, ...(outcome ? { outcome } : {}),
  }))
  const occurrences = routines.map((r) => ({ routineId: r.id, botId: r.botId, at: r.nextRunAt, trigger: 0, enabled: true }))
  return { routines, runs, occurrences }
}
