import { randomBytes } from 'node:crypto'
import type { AccountService } from '../fork-account/service'
import type { ForkCloudAttachment, ForkCloudState } from '../../shared/fork-cloud'
import { AccountApi } from './account-api'
import { deviceSigner } from './device-keys'
import { unb64url } from './crypto'
import { readPin } from './pins'
import type { Principal } from './principal'
import { RelayLink, type LinkState, type SocketFactory } from './relay-socket'
import type { Channel, CallResult, Subscription } from './channel'
import type { FrameSink } from '../../shared/relay-protocol/channel.ts'

/** Uma conexão pertence a uma única época e não reabre ao retornar à mesma conta. */
export class Connection {
  private readonly incarnation = randomBytes(16).toString('base64url')
  private readonly epoch: number
  private readonly api: AccountApi
  private link: RelayLink | null = null
  private channel: Channel | null = null
  private pendingChannel: Channel | null = null
  private pinnedKey = ''
  private attachment: ForkCloudAttachment | null = null
  private readonly subscriptions = new Set<Subscription>()
  private stopped = false
  private started = false
  private pinPending = false
  get hasPendingPin(): boolean { return this.pinPending }
  private diagnosing = false
  private readonly onEpoch = () => { if (!this.current()) this.stop() }

  constructor(private readonly account: AccountService, private readonly principal: Principal, private readonly computerId: string, private readonly onState: (state: ForkCloudState) => void, private readonly socket?: SocketFactory) {
    this.epoch = principal.seal.epoch
    this.api = new AccountApi(principal)
  }
  private current() { return !this.stopped && this.principal.current() && this.account.isCloudSessionCurrent(this.principal.seal) && this.principal.seal.epoch === this.epoch }
  private publish(kind: ForkCloudState['kind']) {
    if (this.current()) this.onState({ kind, epoch: this.epoch, attemptId: this.incarnation, computerId: this.computerId })
  }
  getAttachment(): ForkCloudAttachment | null { return this.current() && this.channel?.isOpen ? this.attachment : null }
  private clearChannel() {
    this.attachment = null
    this.channel = null
    this.pendingChannel = null
    for (const sub of this.subscriptions) { try { sub.cancel() } catch {} }
    this.subscriptions.clear()
  }
  private async qualify(channel: Channel) {
    this.clearChannel()
    this.pendingChannel = channel
    if (!this.current()) { channel.close(); return }
    const result = await channel.call('hello', {}, 20_000).catch(() => ({ kind: 'unknown' as const, reason: 'closed' as const }))
    if (!this.current() || this.pendingChannel !== channel || !channel.isOpen) return
    const hello = result.kind === 'ok' && result.value && typeof result.value === 'object' && !Array.isArray(result.value) ? result.value as { computerId?: unknown; signKey?: unknown; hostId?: unknown; name?: unknown } : null
    if (!hello || hello.computerId !== this.computerId || hello.signKey !== this.pinnedKey || typeof hello.hostId !== 'string' || !hello.hostId.trim() || hello.hostId.length > 256 || /[\x00-\x1f\x7f]/.test(hello.hostId) || typeof hello.name !== 'string' || !hello.name.trim() || hello.name.length > 100 || /[\x00-\x1f\x7f]/.test(hello.name)) {
      this.clearChannel(); this.publish('hubUnverified'); this.stop(); return
    }
    this.pendingChannel = null; this.channel = channel
    this.attachment = { connectionId: randomBytes(16).toString('base64url'), computerId: this.computerId, contextId: this.principal.namespace,
      hostId: hello.hostId, computer: { id: this.computerId, name: hello.name, signKey: this.pinnedKey, urls: [] } }
    this.publish('ready')
  }
  stop() {
    if (this.stopped) return
    this.stopped = true
    this.account.off('epoch', this.onEpoch)
    this.clearChannel()
    this.link?.stop()
  }
  async start(): Promise<boolean> {
    if (this.started || !this.current()) return false
    this.started = true
    this.account.on('epoch', this.onEpoch)
    try {
      const pin = await readPin(this.principal, this.computerId)
      if (!this.current() || !pin) { this.stop(); return false }
      const health = await this.api.execute({ kind: 'health' })
      if (!this.current() || health.kind !== 'ok') { this.stop(); return false }
      const signer = await deviceSigner(this.principal, 'read')
      if (!this.current()) { this.stop(); return false }
      const binding = this.account.verifiedDeviceBinding(this.principal.seal)
      if (!binding || binding.deviceKey !== Buffer.from(signer.publicKey).toString('base64url')) { this.stop(); return false }
      this.pinnedKey = pin.signKey
      const host = unb64url(pin.signKey, 32)
      if (!host || !this.current()) { this.stop(); return false }
      this.link = new RelayLink({ cloudURL: this.principal.deployment.origin, computerId: this.computerId, hostSignPub: host, signer, socket: this.socket,
        isAuthorized: () => this.current(), browserFrames: true,
        onChannel: (channel) => { if (!channel) { this.clearChannel(); if (this.current()) this.publish('reconnecting') } else if (this.current()) void this.qualify(channel); else channel.close() },
        onState: (state) => this.changed(state),
      })
      if (!this.current()) { this.stop(); return false }
      this.link.start()
      return this.current()
    } catch (error) {
      this.pinPending = !!error && typeof error === 'object' && (error as { code?: unknown }).code === 'PIN_PUBLICATION_PENDING'
      this.stop(); return false
    }
  }
  private changed(state: LinkState) {
    if (!this.current()) return
    if (state.kind === 'ready') { this.publish('connecting'); return }
    if (state.kind === 'retrying' && state.connectFailures >= 2 && !this.diagnosing) {
      this.diagnosing = true
      this.link?.stop()
      void this.classify()
      return
    }
    if (state.kind === 'stopped') {
      if (this.diagnosing) return
      const kind = state.reason === 'hubUnverified' ? 'hubUnverified' : state.reason === 'handshakeRefused' ? 'handshakeRefused' : state.reason === 'updateRequired' ? 'updateRequired' : 'accessEnded'
      this.publish(kind)
      return
    }
    this.publish(state.kind === 'retrying' ? 'reconnecting' : state.kind === 'handshaking' ? 'connecting' : state.kind)
  }
  private async classify() {
    const health = await this.api.execute({ kind: 'health' })
    if (!this.current()) return
    if (health.kind !== 'ok') { this.publish('deploymentChanged'); return }
    const list = await this.api.execute({ kind: 'listComputers' })
    if (!this.current()) return
    if (list.kind !== 'ok' || !Array.isArray(list.value.computers)) { this.publish('error'); return }
    const entries = list.value.computers.filter((row: { computerId?: unknown }) => row?.computerId === this.computerId)
    if (entries.length !== 1) this.publish('hubMissing')
    else if (entries[0]?.access === 'none') this.publish('needsLink')
    else this.publish('hubOffline')
  }
  /** `authorize` roda depois de a mensagem estar selada e imediatamente antes de ela ir ao socket; se recusar, nada sai. */
  async call(method: string, body: unknown, timeoutMs = 20_000, authorize?: () => boolean): Promise<CallResult> {
    const channel = this.channel
    if (!this.current() || !channel) return { kind: 'notSent', reason: 'notReady' }
    const result = await channel.call(method, body, timeoutMs, authorize)
    if (!this.current() || this.channel !== channel) return result.kind === 'notSent' ? result : { kind: 'unknown', reason: 'closed' }
    return result
  }
  /** O Hub aceitou `browserFramesV1` neste canal. */
  get framesNegotiated(): boolean { return this.current() && this.channel?.framesNegotiated === true }
  /** Quadros binários do navegador do bot: só com o canal atual e a capacidade aceita pelo Hub. */
  subscribeFrames(sink: FrameSink): Subscription | null {
    const channel = this.channel
    if (!this.current() || !channel || !this.attachment) return null
    let raw: Subscription | null = null
    let sub: Subscription | null = null
    let ended = false
    const guard = () => !ended && this.current() && this.channel === channel
    try {
      raw = channel.subscribeFrames({
        onFrame: (frame) => { if (guard()) sink.onFrame(frame) },
        onEnd: (error) => { if (ended) return; ended = true; if (sub) this.subscriptions.delete(sub); if (this.current() && this.channel === channel) sink.onEnd(error) },
      })
    } catch { return null }
    if (!raw) return null
    const inner = raw
    const tracked: Subscription = { id: inner.id, cancel: () => { if (ended) return; ended = true; this.subscriptions.delete(tracked); inner.cancel() } }
    sub = tracked
    this.subscriptions.add(tracked)
    return tracked
  }
  subscribe(body: unknown, onEvent: (value: unknown) => void, onEnd: (error: { status: number; message: string } | null) => void): Subscription | null {
    const channel = this.channel
    if (!this.current() || !channel || !this.attachment) return null
    let sub: Subscription | null = null
    let ended = false
    const end = (error: { status: number; message: string } | null) => {
      if (ended) return
      ended = true
      if (sub) this.subscriptions.delete(sub)
      if (this.current() && this.channel === channel) onEnd(error)
    }
    try {
      const terminal = !!body && typeof body === 'object' && (body as { kind?: unknown }).kind === 'term'
      const payload = terminal ? { term: (body as { term: string }).term } : body
      const raw = channel.subscribe(terminal ? 'term' : 'events', payload, value => { if (!ended && this.current() && this.channel === channel) onEvent(value) }, end)
      if (!raw || ended) { raw?.cancel(); return null }
      sub = { id: raw.id, cancel: () => { if (ended) return; ended = true; if (sub) this.subscriptions.delete(sub); raw.cancel() } }
      this.subscriptions.add(sub)
      return sub
    } catch { return null }
  }

}
