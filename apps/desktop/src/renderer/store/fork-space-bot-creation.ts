import type { Bot } from '../../shared/models.ts'
import type { SpaceSource } from './fork-space-context.ts'

/** A folha do + de um Space não pode escolher outro Hub nem reaproveitar autoridade após reconectar. */
export class SpaceBotCreation {
  private readonly client
  private readonly hostId
  private readonly epoch
  private readonly source: SpaceSource
  private created: Bot | null = null
  private pending: Promise<Bot> | null = null
  private readonly receive?: (bot: Bot, hubId: string) => void
  readonly spaceId: string
  constructor(source: SpaceSource, spaceId: string, receive?: (bot: Bot, hubId: string) => void) {
    this.receive = receive
    this.source = source
    this.spaceId = spaceId
    this.client = source.connectedClient
    this.hostId = source.hello?.hostId
    this.epoch = source.principalEpoch
    this.check()
  }
  check() {
    if (!this.client || !this.hostId || this.source.connectedClient !== this.client
      || this.source.hello?.hostId !== this.hostId || this.source.principalEpoch !== this.epoch
      || this.source.hello?.spacesContract !== 1
      || !this.source.spaces?.spaces.some((s) => s.id === this.spaceId && s.kind === 'space')) throw new Error('stale_space_context')
  }
  isCurrent() {
    try { this.check(); return true } catch { return false }
  }
  /** Uma instância é uma intenção. Recibo conhecido não vira falha nem outra criação após reconexão. */
  async create(body: Record<string, unknown>): Promise<Bot> {
    if (body.spaceId !== this.spaceId) throw new Error('cross_space_reference')
    if (this.created) return this.created
    if (this.pending) return this.pending
    this.check()
    this.pending = (async () => {
      const { bot } = await this.client!.call('createBot', { ...body, hubId: this.hostId, spaceId: this.spaceId }) as { bot: Bot }
      if (!bot?.id || bot.spaceId !== this.spaceId) throw new Error('cross_space_response')
      this.created = bot
      if (this.isCurrent()) this.receive?.(bot, this.hostId!)
      return bot
    })()
    try { return await this.pending } finally { this.pending = null }
  }
}
