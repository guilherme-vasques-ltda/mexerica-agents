import type { SpaceConversation, SpacesSnapshot } from '../../shared/fork-spaces.ts'
import { SpaceClient, type ConversationClient, type SpaceTransport } from '../../shared/fork-spaces-client.ts'
import { normalizeScreen, type Entry, type ScreenState } from '../../shared/models.ts'
import { entryIn, firstConversation, mergeHistory, principalConversation, readErrorText } from '../lib/fork-space-context.ts'
import { Observable } from '../lib/observable.ts'

// D169 (components/SpaceView; docs/reference/spaces.md): which space the window has open and what it shows of it, read
// from the Hub through `SpaceClient`/`ConversationClient` and the space's own event stream. Every request carries the
// Hub (hello.hostId), link, generation, space and conversation it left with; an answer or event for anything else is
// dropped before it paints (A→B→A, a late reply, another Hub behind the same computer). D543: somente uma intenção explícita envia;
// reconectar apenas relê. Trocar de Hub fecha a vista e invalida intenções antigas.

type StreamEvent = { type: string; id?: string; spaceId?: string; conversation?: SpaceConversation; conversations?: SpaceConversation[]; entry?: Entry; bot?: { id: string; name: string; spaceId?: string }; screen?: Partial<ScreenState> | null }

/** What the context needs of a computer's link: calls, and the event stream scoped to one space. */
export interface SpaceLink extends SpaceTransport {
  inSpace(spaceId: string): { events(since: number, client: string, onEvent: (e: StreamEvent) => void, onEnd: (error: unknown) => void): () => void }
}

/** What the context reads of a computer's store (BotStore): never written. */
export interface SpaceSource {
  readonly principal?: { readonly id: string | null }
  readonly hello: { hostId: string; rev: number; spacesContract?: number } | null
  readonly connectedClient: SpaceLink | null
  readonly principalEpoch: number
  readonly spaces: SpacesSnapshot | null
  subscribe(listener: () => void): () => void
}

/** Uma intenção explícita. Repetir conserva nonce e origem; não acompanha reconexões ou navegação. */
export interface SpaceMessage { readonly text: string; send(): Promise<void> }

type Failure = ReturnType<typeof readErrorText>
const idleHistory = () => ({ state: 'idle' as const, entries: [] as Entry[], hasMore: false, older: false, error: null })
interface Origin { source: SpaceSource; hostId: string; client: SpaceLink; epoch: number; spaceId: string; gen: number }

export class SpaceContext extends Observable {
  /** The open space; null while the window shows Pessoal. */
  spaceId: string | null = null
  list: { state: 'loading' | 'ready' | 'error'; conversations: SpaceConversation[]; principal: { id: string; name: string } | null; error: Failure | null } =
    { state: 'loading', conversations: [], principal: null, error: null }
  /** live; offline keeps the last data with `asOf`; paused when only this space's stream ended. */
  link: 'live' | 'offline' | 'paused' = 'live'
  asOf: number | null = null
  conversationId: string | null = null
  history: { state: 'idle' | 'loading' | 'ready' | 'error'; entries: Entry[]; hasMore: boolean; older: boolean; error: Failure | null } = idleHistory()
  /** Names of this space's own bots, from its stream; the principal's comes with the list. */
  bots = new Map<string, string>()
  /**
   * R1: who uses the computer, as the Hub projects it for this space (hello and `screen` of its own stream). Null
   * before that, and again offline, paused or on a new reading: never Ready, never the identity from before.
   */
  screen: ScreenState | null = null
  /**
   * BN-03: a conversa pedida exatamente (a fixa de um bot, `id === botId`). Ausente da lista do Hub, nada é escolhido
   * no lugar dela (nem a principal nem a primeira): `missing` fica true até Atualizar a encontrar ou a pessoa escolher outra.
   */
  exactId: string | null = null
  missing = false

  get conversationKey() { return JSON.stringify([this.origin?.hostId, this.origin?.gen, this.spaceId, this.conversationId]) }
  /** D543 E1 (2c): principal do Pessoal e de cada Space, para rotular conversas que misturam Spaces. */
  get foreignLabels(): { personalId: string | null; spaces: ReadonlyMap<string, string> } {
    const snapshot = this.origin?.source.spaces
    const personalId = this.origin?.source.principal?.id ?? snapshot?.spaces.find((s) => s.id === snapshot.personalSpaceId)?.principalBotId ?? null
    const spaces = new Map<string, string>()
    for (const s of snapshot?.spaces ?? []) if (s.kind === 'space' && s.principalBotId) spaces.set(s.principalBotId, s.name)
    return { personalId, spaces }
  }
  get canSend() {
    return !!this.origin && this.current(this.origin) && this.link === 'live' && this.list.state === 'ready'
      && this.history.state === 'ready' && this.list.conversations.some((c) => c.id === this.conversationId && !c.closed)
  }

  prepareMessage(text: string, nonce: string = crypto.randomUUID()): SpaceMessage {
    const origin = this.origin
    const client = this.conversation
    const conversation = this.list.conversations.find((c) => c.id === this.conversationId)
    const trimmed = text.trim()
    if (!origin || !client || !conversation || !this.canSend || !trimmed) throw new Error('stale_message_context')
    const current = () => this.current(origin) && this.conversation === client && this.conversationId === conversation.id
    let busy = false
    return { text: trimmed, send: async () => {
      if (!current() || !this.canSend || busy) throw new Error('stale_message_context')
      busy = true
      try {
        // A fixa usa o caminho canônico do próprio bot; adicionais usam o binding/revisão da conversa.
        // Ambos passam por spaces::guard no Host. Nunca se omite Hub, Space ou conversa.
        const result = conversation.id === conversation.botId
          ? await client.space.call<{ entry: Entry }>('send', { conversationId: conversation.id, botId: conversation.botId, text: trimmed, clientNonce: nonce })
          : await client.send({ text: trimmed, clientNonce: nonce, expectedRevision: conversation.revision })
        if (!current()) throw new Error('stale_message_context')
        const e = result.entry
        if (e.botId !== conversation.id || (e.spaceId && e.spaceId !== origin.spaceId)) throw new Error('cross_space_response')
        const incoming = { ...e, spaceId: origin.spaceId, conversationId: conversation.id }
        this.history = { ...this.history, entries: mergeHistory(this.history.entries, [incoming]) }
        this.changed()
      } finally { busy = false }
    } }
  }

  private origin: Origin | null = null
  private gen = 0
  private space: SpaceClient | null = null
  private conversation: ConversationClient | null = null
  private stopStream: (() => void) | null = null
  private unwatch: (() => void) | null = null
  /** Entries of the open conversation that arrive while its latest page is being read: they join the page, not lost. */
  private live: Entry[] = []
  private principalAtOpen: string | null = null
  private reading = false
  private readGeneration = 0
  /** Lowest seq the Hub's pages reached: older pages start below it, and with more above the Hub an older stream entry waits for them. */
  private floor: number | null = null

  /** Opens `spaceId` of this computer's Hub (or reads it again after a new link), from what the store says now. */
  open(source: SpaceSource, spaceId: string, conversationId: string | null = null, exact = false) {
    const hostId = source.hello?.hostId
    const client = source.connectedClient
    this.release()
    this.principalAtOpen = source.spaces?.spaces.find((s) => s.id === spaceId)?.principalBotId ?? null
    const gen = ++this.gen
    const reopening = this.spaceId === spaceId && this.origin?.hostId === hostId
    this.spaceId = spaceId
    this.screen = null
    if (!reopening) this.reset(conversationId)
    if (exact && conversationId) {
      // BN-03 R1-01: outra conversa no MESMO space (A → biblioteca → B). Nada de A pode aparecer sob B.
      if (conversationId !== this.conversationId) {
        this.forgetHistory()
        // Até a página de B chegar: reserva de carregamento, nunca “nenhuma mensagem” nem o conteúdo de A.
        this.history = { ...idleHistory(), state: 'loading' }
      }
      this.exactId = conversationId
      this.missing = false
      this.conversationId = conversationId
    }
    this.unwatch = source.subscribe(this.watch)
    if (!hostId || !client || source.hello?.spacesContract !== 1) {
      this.origin = { source, hostId: hostId ?? '', client: client as SpaceLink, epoch: source.principalEpoch, spaceId, gen }
      this.link = 'offline'
      if (this.list.state === 'loading') this.list = { ...this.list, state: 'error', error: client ? readErrorText(new Error('space_context_unavailable')) : readErrorText(Object.assign(new Error('offline'), { status: 0 })) }
      this.changed()
      return
    }
    const origin: Origin = { source, hostId, client, epoch: source.principalEpoch, spaceId, gen }
    this.origin = origin
    this.link = 'live'
    this.space = new SpaceClient(hostId, spaceId, client)
    this.changed()
    void this.readList(origin)
    // R2: from 0, so the Hub's catch-up brings every bot of this space (its names), not only those changed since hello.
    // R4: and it counts only until its own end, even on a link that would still deliver a line after it.
    let ended = false
    this.stopStream = client.inSpace(spaceId).events(0, 'desktop', (e) => { if (!ended) this.onEvent(origin, e) }, (_error) => {
      if (ended) return
      ended = true
      if (!this.current(origin)) return
      this.link = 'paused'
      this.screen = null
      this.asOf = Date.now()
      this.changed()
    })
  }

  /** Shows `conversationId` of the open space. */
  select(conversationId: string) {
    if (!this.origin || !this.space || conversationId === this.conversationId && this.history.state !== 'error') return
    this.exactId = null
    this.missing = false
    this.conversationId = conversationId
    this.conversation?.dispose()
    this.conversation = null
    this.history = idleHistory()
    this.floor = null
    this.changed()
    void this.readHistory(this.origin, false)
  }

  /** Older messages of the open conversation, one page before the oldest shown. */
  loadOlder() {
    if (!this.origin || this.history.state !== 'ready' || !this.history.hasMore || this.history.older || this.floor === null) return
    this.history = { ...this.history, older: true }
    this.changed()
    void this.readHistory(this.origin, true)
  }

  /** Reads the open space again (after a paused stream or a failed read); read only. */
  retry() {
    const origin = this.origin
    if (origin) this.open(origin.source, origin.spaceId, this.conversationId)
  }

  /** Back to Pessoal: every client, stream and datum of the space goes. */
  close() {
    this.release()
    this.live = []
    this.reading = false
    this.gen++
    this.origin = null
    this.spaceId = null
    this.reset(null)
    this.link = 'live'
    this.asOf = null
    this.changed()
  }

  /**
   * A identidade da conversa mostrada mudou: histórico, piso, paginação, buffer ao vivo e leituras em curso da anterior
   * deixam de valer antes de a nova aparecer. Uma página atrasada da anterior falha em `currentRead` (geração nova).
   * Reler a MESMA conversa não passa por aqui e conserva o que já mostrava.
   */
  private forgetHistory() {
    this.conversation?.dispose()
    this.conversation = null
    this.history = idleHistory()
    this.floor = null
    this.live = []
    this.reading = false
    this.readGeneration++
  }

  private reset(conversationId: string | null) {
    this.exactId = null
    this.missing = false
    this.list = { state: 'loading', conversations: [], principal: null, error: null }
    this.history = idleHistory()
    this.floor = null
    this.bots = new Map()
    this.screen = null
    this.conversationId = conversationId
  }

  private release() {
    this.stopStream?.()
    this.stopStream = null
    this.unwatch?.()
    this.unwatch = null
    this.conversation?.dispose()
    this.conversation = null
    this.space?.dispose()
    this.space = null
  }

  /** The store moved: another Hub closes the view; a new link or generation on the same Hub reads again; none keeps the last data. */
  private watch = () => {
    const origin = this.origin
    if (!origin) return
    const { source } = origin
    // Opened before the Hub said who it is: its hello reads the space then (the window closes it if the Hub's list lacks it).
    if (!origin.hostId) return void (source.hello?.hostId && this.open(source, origin.spaceId, this.conversationId))
    if (source.hello?.hostId !== origin.hostId) return this.close()
    if (!source.connectedClient) {
      if (this.link !== 'offline') {
        this.stopStream?.()
        this.stopStream = null
        this.link = 'offline'
        this.screen = null
        this.asOf = Date.now()
        this.changed()
      }
      return
    }
    const principal = source.spaces?.spaces.find((s) => s.id === origin.spaceId)?.principalBotId ?? null
    if (principal !== this.principalAtOpen) return this.open(source, origin.spaceId, this.conversationId)
    if (source.connectedClient !== origin.client || source.principalEpoch !== origin.epoch || this.link === 'offline') this.open(source, origin.spaceId, this.conversationId)
  }

  private current(origin: Origin) {
    const { source } = origin
    return this.origin === origin && this.gen === origin.gen && source.hello?.hostId === origin.hostId && source.connectedClient === origin.client && source.principalEpoch === origin.epoch
  }

  private async readList(origin: Origin) {
    try {
      const answer = await this.space!.conversations() as { conversations: SpaceConversation[]; principal?: { id: string; name: string } | null }
      if (!this.current(origin)) return
      const conversations = answer.conversations.filter((c) => c.spaceId === origin.spaceId)
      this.list = { state: 'ready', conversations, principal: answer.principal ? { id: answer.principal.id, name: answer.principal.name } : null, error: null }
      this.asOf = Date.now()
      const exact = this.exactId
      this.missing = !!exact && !conversations.some((c) => c.id === exact)
      const keep = exact ? (this.missing ? null : exact)
        : this.conversationId && conversations.some((c) => c.id === this.conversationId) ? this.conversationId : principalConversation(conversations, answer.principal?.id) ?? firstConversation(conversations)
      if (keep !== this.conversationId) this.forgetHistory()
      this.changed()
      if (keep) {
        this.conversationId = keep
        await this.readHistory(origin, false)
      } else this.conversationId = null
    } catch (e) {
      if (!this.current(origin)) return
      this.list = { ...this.list, state: 'error', error: readErrorText(e) }
      this.changed()
    }
  }

  private async readHistory(origin: Origin, older: boolean) {
    const conversationId = this.conversationId
    if (!conversationId || !this.space) return
    if (!this.conversation || this.conversation.conversationId !== conversationId) {
      this.conversation?.dispose()
      this.conversation = this.space.conversation(conversationId)
    }
    const client = this.conversation
    const generation = ++this.readGeneration
    const currentRead = () => this.current(origin) && this.conversationId === conversationId && this.conversation === client && this.readGeneration === generation
    if (!older) {
      // The latest page replaces what was shown (after a new link there may be a gap); what arrives meanwhile joins it.
      this.live = []
      this.reading = true
      this.history = { ...this.history, state: this.history.entries.length ? this.history.state : 'loading', error: null }
      this.changed()
    }
    try {
      const page = await client.history(older && this.floor !== null ? { beforeSeq: this.floor, limit: 100 } : { limit: 100 })
      if (!currentRead()) return
      if (!older) this.reading = false
      const incoming = page.entries.filter((e) => entryIn(e, origin.spaceId, conversationId))
      const lowest = incoming.reduce((low, e) => Math.min(low, e.seq), older ? this.floor ?? Infinity : Infinity)
      this.floor = Number.isFinite(lowest) ? lowest : older ? this.floor : null
      // A stream entry older than the page would leave a hole above it: it comes back with the older pages.
      const live = this.live.filter((e) => !page.hasMore || this.floor === null || e.seq >= this.floor)
      this.history = {
        state: 'ready',
        entries: older ? mergeHistory(this.history.entries, incoming) : mergeHistory(live, incoming),
        hasMore: page.hasMore,
        older: false,
        error: null,
      }
      this.changed()
    } catch (e) {
      if (!currentRead()) return
      if (!older) this.reading = false
      this.history = { ...this.history, state: older ? 'ready' : 'error', older: false, error: readErrorText(e) }
      this.changed()
    }
  }

  private onEvent(origin: Origin, e: StreamEvent) {
    if (!this.current(origin)) return
    if ((e.type === 'hello' || e.type === 'conversation' || e.type === 'screen') && e.spaceId !== origin.spaceId) return
    if (e.type === 'hello') {
      // The hello's screen is this space's projection too; a hello without it leaves no stale identity behind.
      this.screen = normalizeScreen(e.screen)
      if (!e.conversations || this.list.state !== 'ready') return this.changed()
      this.list = { ...this.list, conversations: newest(this.list.conversations, e.conversations.filter((c) => c.spaceId === origin.spaceId), true) }
    } else if (e.type === 'screen') {
      this.screen = normalizeScreen(e.screen)
    } else if (e.type === 'conversation' && e.conversation?.spaceId === origin.spaceId && this.list.state === 'ready') {
      this.list = { ...this.list, conversations: newest(this.list.conversations, [e.conversation], false) }
    } else if (e.type === 'entry' && e.entry && this.conversationId && entryIn(e.entry, origin.spaceId, this.conversationId)) {
      if (this.reading) this.live.push(e.entry)
      if (this.history.state !== 'ready' || this.history.hasMore && this.floor !== null && e.entry.seq < this.floor) return
      this.history = { ...this.history, entries: mergeHistory(this.history.entries, [e.entry]) }
    } else if (e.type === 'bot' && e.bot?.id && e.bot.name && (e.bot.spaceId ?? e.spaceId) === origin.spaceId) {
      // R2: a name only from this space's own roster (the bot's canonical space, or this stream's), never another's.
      this.bots = new Map(this.bots).set(e.bot.id, e.bot.name)
    } else if (e.type === 'botDeleted' && e.id && this.bots.has(e.id)) {
      const bots = new Map(this.bots)
      bots.delete(e.id)
      this.bots = bots
    } else if (e.type === 'resync') {
      return this.retry()
    } else return
    this.link = 'live'
    this.asOf = Date.now()
    // BN-03: a conversa exata pedida chegou pelo stream; só ela é aberta, nunca outra no lugar.
    const exact = this.exactId
    if (this.missing && exact && this.list.conversations.some((c) => c.id === exact)) {
      this.missing = false
      this.forgetHistory()
      this.conversationId = exact
      void this.readHistory(origin, false)
    }
    this.changed()
  }
}

/**
 * Each conversation at its newest state revision. A whole list from the Hub (`full`, its hello) sets the order and
 * drops what it no longer lists; a single update keeps the order and adds a new one at the end.
 */
export function newest(current: readonly SpaceConversation[], incoming: readonly SpaceConversation[], full: boolean) {
  const byId = new Map(current.map((c) => [c.id, c]))
  for (const c of incoming) {
    const known = byId.get(c.id)
    if (!known || c.stateRevision >= known.stateRevision) byId.set(c.id, c)
  }
  const order = full ? incoming.map((c) => c.id) : [...current.map((c) => c.id), ...incoming.map((c) => c.id)]
  return [...new Set(order)].map((id) => byId.get(id)!)
}

/**
 * Como a janela abre um space (use-space-view): com `conversationId`, exatamente essa conversa (BN-03); sem ele, a
 * escolha normal do Hub. Um só caminho para o gancho e para os testes da navegação da biblioteca.
 */
export function openSpaceAt(context: SpaceContext, source: SpaceSource, spaceId: string, conversationId?: string) {
  context.open(source, spaceId, conversationId ?? null, !!conversationId)
}
