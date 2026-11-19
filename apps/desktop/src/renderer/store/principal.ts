import type { Bot } from '../../shared/models'
import { Observable } from '../lib/observable.ts'
import { principalTrigger, protectedPrincipal, type PrincipalIdentity, type PrincipalPreference } from '../lib/principal.ts'

interface Source extends Observable {
  bots: Map<string, Bot>
  connection: { kind: string }
  principalEpoch: number
  mismatch: unknown
  computer: { id: string }
  contextId: string
  spaceId?: string
  spaces?: import('../../shared/fork-spaces').SpacesSnapshot | null
}

interface IdentitySource {
  read(): Promise<{ primaryBotId: string | null }>
  hostId(): string | null
  storage: Pick<Storage, 'getItem' | 'setItem'>
}

/** Uma consulta por Hub em voo; gerações antigas nunca substituem a identidade atual. */
export class PrincipalModel extends Observable implements PrincipalIdentity {
  id: string | null = null
  phase: PrincipalIdentity['phase'] = 'unknown'
  private generation = 0
  private active = false
  private queued = false
  private retired = false
  private key = ''
  private off: () => void
  private source: Source
  private identity: IdentitySource
  private hostId: string | null
  private preferenceKey: string
  private savedIdentity: PrincipalPreference | null = null

  constructor(source: Source, identity: IdentitySource) {
    super()
    this.source = source
    this.identity = identity
    this.hostId = identity.hostId()
    this.preferenceKey = `donnabot-principal-v1-${JSON.stringify([source.contextId, source.computer.id, ...(source.spaceId ? [source.spaceId] : [])])}`
    try {
      const saved = JSON.parse(identity.storage.getItem(this.preferenceKey) ?? 'null') as PrincipalPreference | null
      if (saved && typeof saved.hostId === 'string' && (saved.primaryBotId === null || typeof saved.primaryBotId === 'string')) this.savedIdentity = saved
    } catch {}
    if (this.hostId && this.savedIdentity?.hostId !== this.hostId) this.remember(null)
    this.off = source.subscribe(() => this.sync())
  }

  get protectedId() {
    const hostId = this.identity.hostId()
    return hostId === this.hostId ? protectedPrincipal(this, this.savedIdentity, hostId) : null
  }

  private remember(settings: { primaryBotId: string | null } | null) {
    this.savedIdentity = settings && this.hostId ? { hostId: this.hostId, primaryBotId: settings.primaryBotId } : null
    try { this.identity.storage.setItem(this.preferenceKey, JSON.stringify(this.savedIdentity)) } catch {}
  }

  private get online() { return this.source.connection.kind === 'online' && !this.source.mismatch }

  private sync() {
    const hostId = this.identity.hostId()
    if (hostId !== this.hostId) {
      this.hostId = hostId
      this.id = null
      this.phase = 'unknown'
      if (this.savedIdentity?.hostId !== hostId) this.remember(null)
    }
    if (this.source.spaceId && this.source.spaceId !== this.source.spaces?.personalSpaceId) {
      const space = this.source.spaces?.spaces.find(s => s.id === this.source.spaceId)
      const id = space?.principalBotId ?? null
      const phase = this.online && space ? 'ready' : 'unknown'
      if (this.id !== id || this.phase !== phase) {
        this.generation++; this.queued = false; this.id = id; this.phase = phase
        this.remember(space ? { primaryBotId: id } : null); this.changed()
      }
      return
    }
    const key = JSON.stringify([hostId, this.online, this.source.principalEpoch, principalTrigger(this.source.bots.values())])
    if (this.retired || key === this.key) return
    this.key = key
    this.refresh()
  }

  refresh() {
    if (this.retired) return
    if (this.source.spaceId && this.source.spaceId !== this.source.spaces?.personalSpaceId) { this.sync(); return }
    this.generation++
    this.queued = this.online
    if (!this.online) {
      if (this.phase === 'loading') this.phase = 'unknown'
      this.changed()
    }
    void this.pump()
  }

  private async pump() {
    if (this.active || !this.queued || this.retired) return
    this.active = true
    this.queued = false
    const generation = this.generation
    this.phase = 'loading'
    this.changed()
    try {
      const settings = await this.identity.read()
      if (!this.retired && this.online && generation === this.generation) {
        this.id = settings.primaryBotId
        this.phase = 'ready'
        this.remember(settings)
      }
    } catch {
      if (!this.retired && generation === this.generation) this.phase = 'error'
    } finally {
      this.active = false
      if (!this.retired) { this.changed(); void this.pump() }
    }
  }

  /** O resultado da API de gravação também é autoritativo; invalida leituras antigas. */
  saved(settings: { primaryBotId: string | null }) {
    if (this.source.spaceId && this.source.spaceId !== this.source.spaces?.personalSpaceId) { this.sync(); return }
    this.generation++
    this.queued = false
    this.id = settings.primaryBotId
    this.phase = 'ready'
    this.remember(settings)
    this.changed()
  }

  retire() { this.retired = true; this.generation++; this.off() }
}
