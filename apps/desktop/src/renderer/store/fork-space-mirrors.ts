import type { Space } from '../../shared/fork-spaces'
import type { BotStore } from './bot-store'
import type { BotRef } from './app-model'

interface WindowSource {
  stores: Map<string, BotStore>
  selection: BotRef | null
  select(ref: BotRef | null): void
}

/** E2: uma loja/stream por Space conhecido. A raiz do Pessoal conserva descoberta e autoridade. */
export class SpaceMirrors {
  active: { computerId: string; hostId: string | null; spaceId: string; record: Space | null } | null = null
  /** Geração do Space ativo; seleção de conversa no mesmo contexto não a altera. */
  spaceEpoch = 0
  private activeIdentity: string | null = null
  private pools = new Map<string, Map<string, BotStore>>()
  private stops = new Map<string, (() => void)[]>()
  private selections = new Map<string, BotRef>()
  private mirrorsStops = new Map<BotStore, () => void>()
  private hosts = new Map<string, string>()
  private syncing = false

  constructor(private window: WindowSource, private notify: () => void) {}

  attach(root: BotStore) {
    const id = root.computer.id
    this.pools.set(id, new Map())
    this.stops.set(id, [root.subscribe(() => this.sync(root))])
    this.sync(root)
  }

  private sync(root: BotStore) {
    if (this.syncing) return
    const host = root.hello?.hostId
    const pool = this.pools.get(root.computer.id)
    if (!pool || !host) return
    this.syncing = true
    try {
      const before = this.hosts.get(root.computer.id)
      if (before && before !== host) {
        this.spaceEpoch++
        for (const store of pool.values()) this.retire(store)
        pool.clear()
        if (this.active?.computerId === root.computer.id) this.window.selection = null
      }
      this.hosts.set(root.computer.id, host)
      const spaces = root.hello?.spacesContract === 1 ? root.spaces?.spaces.filter(s => s.kind === 'space') ?? [] : []
      for (const [id, store] of pool) if (!spaces.some(s => s.id === id)) {
        this.retire(store); pool.delete(id)
        if (this.active?.computerId === root.computer.id && this.active.spaceId === id && root.hello?.spacesContract === 1) {
          this.spaceEpoch++
          this.active = null
          this.window.selection = null
        }
      }
      for (const space of spaces) if (!pool.has(space.id)) {
        const store = root.mirrorInSpace(space.id)
        pool.set(space.id, store)
        this.mirrorsStops.set(store, store.subscribe(() => {
          if (store.selection) {
            const botId = store.selection
            store.selection = null
            this.window.select({ computerId: root.computer.id, spaceId: space.id, botId })
          }
          this.notify()
        }))
        store.onRosterChanged = () => this.notify()
        store.start()
      }
    } finally { this.syncing = false }
    this.notify()
  }

  root(computerId: string) { return this.window.stores.get(computerId) ?? null }
  store(computerId: string, spaceId?: string) {
    const root = this.root(computerId)
    if (!spaceId || spaceId === root?.spaces?.personalSpaceId) return root
    return this.pools.get(computerId)?.get(spaceId) ?? null
  }
  current(computerId: string) {
    if (this.active?.computerId === computerId) return this.active.hostId === this.root(computerId)?.hello?.hostId ? this.store(computerId, this.active.spaceId) : null
    return this.root(computerId)
  }
  /** MX-217: a raiz e os spaces de um computador (a queda curta estaciona e retoma todos juntos). */
  doComputador(computerId: string) {
    const root = this.root(computerId)
    return [...(root ? [root] : []), ...(this.pools.get(computerId)?.values() ?? [])]
  }
  get all() { return [...this.window.stores.values(), ...[...this.pools.values()].flatMap(pool => [...pool.values()])] }

  activate(computerId: string, spaceId?: string) {
    const identity = this.spaceIdentity(computerId, spaceId)
    const previous = this.activeIdentity ?? (this.active
      ? this.spaceIdentity(this.active.computerId, this.active.spaceId)
      : this.spaceIdentity(this.window.selection?.computerId ?? computerId, this.window.selection?.spaceId))
    if (identity !== previous) this.spaceEpoch++
    this.activeIdentity = identity
    const root = this.root(computerId)
    const personal = !spaceId || spaceId === root?.spaces?.personalSpaceId
    const next = personal ? null : { computerId, hostId: root?.hello?.hostId ?? null, spaceId, record: root?.spaces?.spaces.find(s => s.id === spaceId) ?? null }
    if (this.active?.computerId !== next?.computerId || this.active?.spaceId !== next?.spaceId) {
      const previous = this.window.selection
      if (previous) this.selections.set(this.key(previous.computerId, previous.spaceId), previous)
      const saved = this.selections.get(this.key(computerId, personal ? undefined : spaceId))
      const store = this.store(computerId, personal ? undefined : spaceId)
      this.window.selection = saved && store?.bots.has(saved.botId) ? saved : null
    }
    this.active = next
    this.notify()
  }

  infer(ref: BotRef): BotRef {
    if (ref.hubId) return ref
    if (ref.spaceId) return { ...ref, hubId: this.root(ref.computerId)?.hello?.hostId }
    // UUID de bot é único no Hub, mas a intenção que sai daqui já fica explícita.
    const store = this.all.find(s => s.computer.id === ref.computerId && s.bots.has(ref.botId))
    return store ? { ...ref, ...(store.spaceId ? { spaceId: store.spaceId } : {}), hubId: store.hello?.hostId } : ref
  }

  resolve(ref: BotRef) {
    if (ref.hubId && this.root(ref.computerId)?.hello?.hostId !== ref.hubId) return null
    return this.store(ref.computerId, ref.spaceId)
  }

  // Pessoal é active=null por compatibilidade, mas sua identidade inclui computador e Hub.
  private spaceIdentity(computerId: string, spaceId?: string) {
    const root = this.root(computerId)
    return JSON.stringify([computerId, root?.hello?.hostId ?? null, spaceId || root?.spaces?.personalSpaceId || null])
  }

  private key(computerId: string, spaceId?: string) { return JSON.stringify([computerId, this.hosts.get(computerId) ?? null, spaceId ?? null]) }
  private retire(store: BotStore) {
    this.mirrorsStops.get(store)?.()
    this.mirrorsStops.delete(store)
    store.retire()
  }

  detach(computerId: string) {
    this.spaceEpoch++
    for (const stop of this.stops.get(computerId) ?? []) stop()
    for (const store of this.pools.get(computerId)?.values() ?? []) this.retire(store)
    this.stops.delete(computerId); this.pools.delete(computerId); this.hosts.delete(computerId)
    if (this.active?.computerId === computerId) this.active = null
  }
}
