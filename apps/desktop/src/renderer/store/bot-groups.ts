import type { BotGroupsEdit, BotGroupsEditResult, BotGroupsSnapshot } from '../../shared/bot-groups.ts'
import { Observable } from '../lib/observable.ts'

export interface GroupsClient { call<T>(method: string, body?: unknown): Promise<T> }
type Event = { type: 'hello'; hostId?: string; spaceId?: string; rev?: number; botGroups?: BotGroupsSnapshot } | { type: 'botGroups'; spaceId: string; snapshot: BotGroupsSnapshot } | { type: string }

/** Um modelo pertence a um único Space; o Hub antigo não anuncia o campo no hello. */
export class BotGroupsModel extends Observable {
  snapshot: BotGroupsSnapshot | null = null
  error: { code?: string; message: string } | null = null
  private queue: Promise<unknown> = Promise.resolve()
  private hostId: string | undefined
  private spaceId: string | undefined
  private generation = 0
  private readonly client: () => Promise<GroupsClient>
  private readonly space: () => string | undefined
  private readonly origin: () => string | undefined
  constructor(client: () => Promise<GroupsClient>, space: () => string | undefined, origin: () => string | undefined = () => undefined) {
    super()
    this.client = client
    this.space = space
    this.origin = origin
  }

  receive(event: Event) {
    if (event.type === 'hello') {
      const hello = event as Extract<Event, { type: 'hello' }>
      if (this.space() && hello.spaceId !== this.space()) return
      const changedOrigin = (this.hostId !== undefined && hello.hostId !== this.hostId) || (this.spaceId !== undefined && hello.spaceId !== this.spaceId)
      const rewound = typeof hello.rev === 'number' && !!this.snapshot && hello.rev < this.snapshot.rev
      if (changedOrigin || rewound || !hello.botGroups) { this.snapshot = null; this.generation++ }
      this.hostId = hello.hostId
      this.spaceId = hello.spaceId
      if (this.error) { this.error = null; this.changed() }
      if (!hello.botGroups) { this.changed(); return }
      this.accept(hello.botGroups)
    } else if (event.type === 'botGroups') {
      const update = event as Extract<Event, { type: 'botGroups' }>
      if (!this.space() || update.spaceId !== this.space()) return
      this.accept(update.snapshot)
    }
  }

  matchesOrigin(hostId: string, spaceId: string) { return this.hostId === hostId && this.spaceId === spaceId }

  private accept(snapshot: BotGroupsSnapshot) {
    if (this.snapshot && snapshot.rev < this.snapshot.rev) return
    this.snapshot = snapshot
    this.changed()
  }

  edit(op: BotGroupsEdit, suppressNameTaken = false): Promise<BotGroupsEditResult> {
    const origin = this.origin(), space = this.space(), generation = this.generation
    const run = this.queue.then(async () => {
      try {
        const client = await this.client()
        if (this.origin() !== origin || this.space() !== space || this.generation !== generation) throw new Error('stale_space_link')
        const result = await client.call<BotGroupsEditResult>('botGroupsEdit', op)
        if (this.origin() !== origin || this.space() !== space || this.generation !== generation) throw new Error('stale_space_link')
        this.accept(result.snapshot)
        this.error = null
        this.changed()
        return result
      } catch (error) {
        const e = error as { code?: string; message?: string }
        if (this.origin() === origin && this.space() === space && this.generation === generation && !(suppressNameTaken && e.code === 'name_taken')) {
          this.error = { code: e.code, message: e.message ?? '' }
          this.changed()
        }
        throw error
      }
    })
    this.queue = run.catch(() => {})
    return run
  }

  async addGenerated(base: string, botId?: string): Promise<BotGroupsEditResult> {
    for (let n = 1; ; n++) {
      const suffix = n === 1 ? '' : ` (${n})`
      const name = [...base].slice(0, 40 - [...suffix].length).join('') + suffix
      try { return await this.edit({ op: 'add', name, ...(botId ? { botId } : {}) }, true) }
      catch (error) { if ((error as { code?: string }).code !== 'name_taken') throw error }
    }
  }
}
