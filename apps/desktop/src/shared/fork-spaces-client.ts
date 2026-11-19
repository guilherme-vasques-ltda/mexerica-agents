import type { SpaceBirth, SpaceCreate, SpaceUpdate, SpacesSnapshot, SpaceConversation, SpaceReorder, OwnerPresentation, ConversationUpdate, ConversationHistory, ConversationSend, ConversationUpload, ContextAttachment, SpaceContext, SpacePrincipal, SpaceDeleteResult } from './fork-spaces.ts'
import type { BotEditor, BotEditorSave } from './fork-bot-editor.ts'

export interface SpaceTransport { call(method: string, body: unknown, timeoutMs?: number): Promise<unknown> }

/** Uma instância por montagem; descarte antes de selecionar outro Space/Hub. */
export class SpaceClient {
  private active = true
  readonly hostId: string
  readonly spaceId: string
  private readonly transport: SpaceTransport
  constructor(hostId: string, spaceId: string, transport: SpaceTransport) {
    if (!hostId || !spaceId) throw new Error('space_context_required')
    this.hostId = hostId
    this.spaceId = spaceId
    this.transport = transport
  }
  dispose() { this.active = false }
  key(conversationId = '') { return JSON.stringify([this.hostId, this.spaceId, conversationId]) }
  async call<T>(method: string, body: Record<string, unknown> = {}, timeoutMs?: number): Promise<T> {
    if (!this.active) throw new Error('stale_space_context')
    if (body.hubId !== undefined && body.hubId !== this.hostId) throw new Error('cross_hub_reference')
    if (body.spaceId !== undefined && body.spaceId !== this.spaceId) throw new Error('cross_space_reference')
    const value = await this.transport.call(method, { ...body, hubId: this.hostId, spaceId: this.spaceId }, timeoutMs)
    if (!this.active) throw new Error('stale_space_context')
    const object = value as { hubId?: string; spaceId?: string; context?: { spaceId?: string }; conversation?: { spaceId?: string }; conversations?: { spaceId?: string }[] } | null
    if ((method.startsWith('donnaSpace') || method.startsWith('donnaConversation') || method === 'donnaOwnerPresentation' || object?.hubId !== undefined) && object?.hubId !== this.hostId) throw new Error('cross_hub_response')
    if ((object?.spaceId !== undefined && object.spaceId !== this.spaceId)
      || (object?.context?.spaceId !== undefined && object.context.spaceId !== this.spaceId)) throw new Error('cross_space_response')
    if (object?.conversation && object.conversation.spaceId !== this.spaceId) throw new Error('cross_space_response')
    if (object?.conversations?.some((c) => c.spaceId !== this.spaceId)) throw new Error('cross_space_response')
    return value as T
  }
  conversation(id: string) { return new ConversationClient(this, id) }
  reorder(body: SpaceReorder) { return this.call<SpacesSnapshot>('donnaSpaceReorder', { ...body }) }
  owner(expectedRevision: number, owner: OwnerPresentation) { return this.call<SpacesSnapshot>('donnaOwnerPresentation', { expectedRevision, owner }) }
  spaces() { return this.call<SpacesSnapshot>('donnaSpaces') }
  birth(body: SpaceBirth) { return this.call<SpacesSnapshot>('donnaSpaceBirth', { ...body }) }
  /** D543 E1: ativa o Berga num Space legado; o cliente é o do Space alvo. */
  principal(body: Omit<SpacePrincipal, 'spaceId' | 'hubId'>) { return this.call<SpacesSnapshot>('donnaSpacePrincipal', { ...body }) }
  /** D543 E1: exclusão definitiva; resposta é a lista nova ou `{busy}` com os nomes. */
  remove(body: Omit<import('./fork-spaces.ts').SpaceDelete, 'spaceId' | 'hubId'>) { return this.call<SpaceDeleteResult>('donnaSpaceDelete', { ...body }) }
  create(body: SpaceCreate) { return this.call<SpacesSnapshot>('donnaSpaceCreate', { ...body }) }
  update(body: Omit<SpaceUpdate, 'spaceId' | 'hubId'>) { return this.call<SpacesSnapshot>('donnaSpaceUpdate', { ...body }) }
  /** D543 E1: leitura do perfil do bot principal para aparência/nome; o Hub ignora spaceId nesses métodos. */
  botEditor(botId: string) { return this.call<BotEditor>('donnaBotEditor', { botId }) }
  botEditorSave(request: BotEditorSave) { return this.call<BotEditor>('donnaBotEditorSave', { ...request }) }
  conversations() { return this.call<{ spaceId: string; conversations: SpaceConversation[] }>('donnaConversations') }
  createConversation(botId: string, title?: string) {
    return this.call<{ conversation: SpaceConversation }>('donnaConversationCreate', { botId, ...(title === undefined ? {} : { title }) })
  }
}

/** Uma instância por seleção de conversa; descartar também ao trocar Hub/Space. */
export class ConversationClient {
  private active = true
  readonly space: SpaceClient
  readonly conversationId: string
  constructor(space: SpaceClient, conversationId: string) {
    this.space = space
    this.conversationId = conversationId
    if (!conversationId) throw new Error('conversation_context_required')
  }
  dispose() { this.active = false }
  key() { return this.space.key(this.conversationId) }
  async call<T>(method: string, body: Record<string, unknown> = {}): Promise<T> {
    if (!this.active) throw new Error('stale_conversation_context')
    if (body.conversationId !== undefined && body.conversationId !== this.conversationId) throw new Error('cross_conversation_reference')
    const value = await this.space.call<T>(method, { ...body, conversationId: this.conversationId })
    if (!this.active) throw new Error('stale_conversation_context')
    const response = value as { context?: SpaceContext; conversation?: SpaceConversation; entry?: { conversationId?: string } }
    if ((response.context && response.context.conversationId !== this.conversationId)
      || (response.conversation && response.conversation.id !== this.conversationId)
      || (response.entry && response.entry.conversationId !== this.conversationId)) throw new Error('cross_conversation_response')
    return value
  }
  history(body: { beforeSeq?: number; limit?: number; query?: string } = {}) { return this.call<ConversationHistory>('donnaConversationHistory', body) }
  update(body: ConversationUpdate) { return this.call<{ conversation: SpaceConversation }>('donnaConversationUpdate', { ...body }) }
  read() { return this.call<{ conversation: SpaceConversation }>('donnaConversationRead') }
  send(body: Omit<ConversationSend, 'hubId' | 'spaceId' | 'conversationId'>) { return this.call<{ context: SpaceContext; entry: import('./models').Entry; replayed: boolean; queued?: true }>('donnaConversationSend', { ...body }) }
  /** G1b: resposta do dono ao cartão de escolha (`entry.data.genuiChoice`); o Hub faz o CAS e devolve recibo. */
  answerChoice(body: { expectedRevision: number; questionId: string; revision: number; answer: unknown }) {
    return this.call<{ context: SpaceContext; question: unknown; entry: import('./models').Entry; replayed: boolean }>('donnaConversationChoiceAnswer', { ...body })
  }
  upload(body: ConversationUpload) { return this.call<{ context: SpaceContext; attachment: ContextAttachment | null; revision?: number; replayed?: boolean }>('donnaConversationUpload', { ...body }) }
  readFile(entryId: string, fileId: string, offset = 0) { return this.call<{ context: SpaceContext; data: string; size: number }>('donnaConversationReadFile', { entryId, fileId, offset }) }
  readUpload(uploadId: string, offset = 0) { return this.call<{ context: SpaceContext; attachment: ContextAttachment; data: string; size: number }>('donnaConversationReadUpload', { uploadId, offset }) }
  memory<T>(body: { method: string; expectedRevision?: number; [key: string]: unknown }) { return this.call<T & { context: SpaceContext }>('donnaConversationMemory', body) }
  /** entryId da mensagem original do usuário; o ator captura o cartão/espera real. */
  retry(expectedRevision: number, entryId: string) { return this.call<{ context: SpaceContext; retryRequested: true }>('donnaConversationRetry', { expectedRevision, entryId }) }
  stop(expectedRevision: number, entryId: string) { return this.call<{ context: SpaceContext; stopRequested: boolean; entryIds: string[] }>('donnaConversationStop', { expectedRevision, entryId }) }
  newSession(expectedRevision: number) { return this.call<{ context: SpaceContext; conversation: SpaceConversation; reset: true }>('donnaConversationNewSession', { expectedRevision }) }
}
