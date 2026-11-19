import type { Engine, ManagedAccount } from './fork-engine-accounts.ts'
import type { SpaceBirth, SpacesSnapshot } from './fork-spaces.ts'
import { SpaceClient, type SpaceTransport } from './fork-spaces-client.ts'

/** Só referências comprovadas pelo Registry do Hub capturado; nenhuma credencial atravessa a folha. */
export function birthAccounts(accounts: ManagedAccount[], backend: Engine) {
  return accounts.filter((a) => a.backend === backend && a.state === 'connected')
}

export function birthDefault(accounts: ManagedAccount[], backend: Engine): string {
  const defaults = birthAccounts(accounts, backend).filter((a) => a.origin === 'legacy')
  return defaults.length === 1 ? defaults[0]!.id : ''
}

export interface BirthSource {
  hello: { hostId: string; spaceBirthContract?: number } | null
  connectedClient: SpaceTransport | null
  spaces?: { revision: number } | null
  principalEpoch: number
}

/** Um nonce por intenção, inclusive após resposta perdida. Nunca rebaseia o pedido no Hub/revisão novos. */
export class SpaceBirthIntent {
  private request: SpaceBirth | null = null
  private inflight = false
  private source: BirthSource
  private hostId: string
  private client: SpaceTransport
  private epoch: number
  private personalId: string
  private revision: number
  private nonce: string
  constructor(source: BirthSource, hostId: string, client: SpaceTransport, epoch: number, personalId: string, revision: number, nonce: string) {
    this.source = source
    this.hostId = hostId
    this.client = client
    this.epoch = epoch
    this.personalId = personalId
    this.revision = revision
    this.nonce = nonce
  }
  get fixed() { return this.request !== null }
  private current() {
    if (this.source.hello?.hostId !== this.hostId || this.source.connectedClient !== this.client || this.source.principalEpoch !== this.epoch) throw new Error('space_birth_context_changed')
    if (this.source.hello.spaceBirthContract !== 1) throw new Error('space_birth_unsupported')
  }
  async send(draft: Omit<SpaceBirth, 'expectedRevision' | 'clientNonce'>): Promise<SpacesSnapshot> {
    this.current()
    if (this.inflight) throw new Error('space_birth_inflight')
    if (!this.request && this.source.spaces && this.source.spaces.revision !== this.revision) throw new Error('space_birth_revision_changed')
    if (!this.request) this.request = structuredClone({ ...draft, expectedRevision: this.revision, clientNonce: this.nonce })
    this.inflight = true
    const scoped = new SpaceClient(this.hostId, this.personalId, this.client)
    try {
      const result = await scoped.birth(this.request)
      this.current()
      return result
    } finally {
      scoped.dispose()
      this.inflight = false
    }
  }
}

/** Ativação: conserva o corpo exato após resposta perdida, como o Birth. */
export class SpacePrincipalIntent {
  private request: Omit<import('./fork-spaces.ts').SpacePrincipal, 'hubId' | 'spaceId'> | null = null
  private inflight = false
  private source: Omit<BirthSource, 'hello'> & { hello: { hostId: string; spaceLifecycleContract?: number } | null }
  private hostId: string
  private client: SpaceTransport
  private epoch: number
  private spaceId: string
  private revision: number
  constructor(source: SpacePrincipalIntent['source'], hostId: string, client: SpaceTransport, epoch: number, spaceId: string, revision: number) {
    this.source = source
    this.hostId = hostId
    this.client = client
    this.epoch = epoch
    this.spaceId = spaceId
    this.revision = revision
  }
  get fixed() { return this.request !== null }
  async send(draft: Omit<import('./fork-spaces.ts').SpacePrincipal, 'expectedRevision' | 'hubId' | 'spaceId'>): Promise<SpacesSnapshot> {
    if (this.source.hello?.hostId !== this.hostId || this.source.connectedClient !== this.client || this.source.principalEpoch !== this.epoch) throw new Error('hub_context_mismatch')
    if (this.source.hello.spaceLifecycleContract !== 1) throw new Error('spaces_unavailable')
    if (this.inflight) throw new Error('space_principal_inflight')
    if (!this.request && this.source.spaces?.revision !== this.revision) throw new Error('spaces_revision_conflict')
    if (!this.request) this.request = structuredClone({ ...draft, expectedRevision: this.revision })
    this.inflight = true
    const scoped = new SpaceClient(this.hostId, this.spaceId, this.client)
    try {
      const result = await scoped.principal(this.request)
      if (this.source.hello?.hostId !== this.hostId || this.source.connectedClient !== this.client || this.source.principalEpoch !== this.epoch) throw new Error('hub_context_mismatch')
      return result
    } finally { scoped.dispose(); this.inflight = false }
  }
}
