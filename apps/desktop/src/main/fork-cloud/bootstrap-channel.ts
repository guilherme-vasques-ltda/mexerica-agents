import { createHash } from 'node:crypto'
import { performance } from 'node:perf_hooks'
import type { AccountService, DeviceBinding } from '../fork-account/service'
import type { Principal } from './principal'
import { deviceSigner } from './device-keys'
import { b64url, unb64url, signatureHeader, type DeviceSigner } from './crypto'
import { Channel, Handshake, type CallResult } from './channel'
import { deviceRelayURL, nodeSocket, type SocketFactory, type SocketLike } from './relay-socket'
import { verifyBootstrapGrant, verifyBootstrapChallenge, verifyBootstrapChain, type BootstrapEnvelope, type BootstrapExpected, type BootstrapOriginals, type VerifiedGrant, type VerifiedChain } from './bootstrap-codec'

/** O callback deve persistir e sincronizar esses mesmos originais e a fase de envio incerto. */
export type BootstrapProofJournal = Readonly<{ grant: BootstrapEnvelope; challenge: BootstrapEnvelope; proof: BootstrapEnvelope }>
export type PersistBootstrapProof = (originals: BootstrapProofJournal) => Promise<void>
export type BootstrapChannelResult<T> = { kind: 'ok'; value: T } | { kind: 'notSent' | 'unknown' | 'unavailable' }
export type BootstrapReceipt = { readonly originals: BootstrapOriginals; readonly verified: VerifiedChain }
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
const sha = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex')
const sameBinding = (a: DeviceBinding | null, b: DeviceBinding) => !!a && (['version', 'sessionId', 'userId', 'origin', 'issuer', 'deploymentId', 'manifestHash', 'deviceId', 'deviceKey'] as const).every(k => a[k] === b[k])

/** Canal privado de uso único: não expõe socket, signer, RPC genérico, subs ou attachment. */
export class BootstrapChannel {
  private state: 'new' | 'opening' | 'open' | 'challenging' | 'challenged' | 'proving' | 'done' | 'closed' = 'new'
  private socket: SocketLike | null = null
  private handshake: Handshake | null = null
  private channel: Channel | null = null
  private timer: ReturnType<typeof setTimeout> | null = null
  private opening: ((value: BootstrapChannelResult<void>) => void) | null = null
  private challengeEnvelope: BootstrapEnvelope | null = null
  private grant: VerifiedGrant
  private readonly grantWire: BootstrapEnvelope
  private readonly deadlineMono: number
  private challengeDeadline = Infinity
  private challengeWall = Infinity
  private readonly epochListener = () => { if (!this.current()) this.close() }

  private constructor(private readonly account: AccountService, private readonly principal: Principal,
    private readonly binding: DeviceBinding, private readonly expected: BootstrapExpected, private readonly rootKey: string,
    original: BootstrapEnvelope, private readonly signer: DeviceSigner, private readonly persist: PersistBootstrapProof,
    private readonly socketFactory: SocketFactory) {
    this.grantWire = copy(original)
    this.grant = verifyBootstrapGrant(this.grantWire, expected, rootKey, Date.now())
    const expires = Math.min(this.grant.payload[19] as number, expected.expiresAt)
    this.deadlineMono = performance.now() + Math.max(0, expires - Date.now())
    account.on('epoch', this.epochListener)
  }

  /** Principal e binding são capturados pelo Main; rootKey vem exclusivamente de M admitido. */
  static async create(account: AccountService, principal: Principal, expected: BootstrapExpected, rootKey: string,
    grant: BootstrapEnvelope, persist: PersistBootstrapProof, socketFactory: SocketFactory = nodeSocket): Promise<BootstrapChannel> {
    if (!principal.current() || !account.isCloudSessionCurrent(principal.seal) || typeof persist !== 'function') throw new Error('Bootstrap unavailable.')
    const liveBinding = account.verifiedDeviceBinding(principal.seal)
    const binding = liveBinding ? Object.freeze({ ...liveBinding }) : null
    if (!binding || binding.userId !== principal.seal.userId || binding.sessionId !== principal.seal.sessionId || binding.origin !== principal.deployment.origin || binding.issuer !== principal.deployment.issuer || binding.deploymentId !== principal.deployment.deploymentId || binding.manifestHash !== principal.deployment.manifestHash || expected.ownerUserId !== principal.seal.userId || expected.deviceId !== binding.deviceId || expected.deviceKey !== binding.deviceKey || expected.deploymentId !== principal.deployment.deploymentId || expected.env !== principal.deployment.env || expected.issuer !== principal.deployment.issuer || expected.manifestHash !== principal.deployment.manifestHash) throw new Error('Different bootstrap context.')
    const immutable = Object.freeze(copy(expected))
    const original = copy(grant)
    verifyBootstrapGrant(original, immutable, rootKey, Date.now())
    const signer = await deviceSigner(principal, 'read')
    if (!principal.current() || !account.isCloudSessionCurrent(principal.seal) || !sameBinding(account.verifiedDeviceBinding(principal.seal), binding) || b64url(signer.publicKey) !== binding.deviceKey) throw new Error('Different bootstrap device.')
    verifyBootstrapGrant(original, immutable, rootKey, Date.now())
    return new BootstrapChannel(account, principal, Object.freeze({ ...binding }), immutable, rootKey, original, signer, persist, socketFactory)
  }

  private current(): boolean {
    return this.state !== 'closed' && this.state !== 'done' && this.principal.current() && this.account.isCloudSessionCurrent(this.principal.seal) && sameBinding(this.account.verifiedDeviceBinding(this.principal.seal), this.binding) && performance.now() < this.deadlineMono && performance.now() < this.challengeDeadline && Date.now() < (this.grant.payload[19] as number) && Date.now() < this.expected.expiresAt && Date.now() < this.challengeWall
  }
  private armDeadline() {
    if (this.timer) clearTimeout(this.timer)
    const remaining = Math.min(this.deadlineMono, this.challengeDeadline) - performance.now()
    if (remaining <= 0) { this.close(); return }
    this.timer = setTimeout(() => this.close(), Math.ceil(remaining))
  }
  private wireSend(message: object) {
    if (!this.current() || !this.socket) throw new Error('Bootstrap superseded.')
    const raw = JSON.stringify(message)
    // O limite de 16 KiB vale para o plaintext; os quadros cifrados são maiores.
    if (Buffer.byteLength(raw, 'utf8') > 24_000) throw new Error('Bootstrap frame too large.')
    this.socket.send(raw)
  }
  private finishOpening(result: BootstrapChannelResult<void>) {
    const settle = this.opening; this.opening = null; settle?.(result)
  }
  close() {
    if (this.state === 'closed' || this.state === 'done') return
    this.state = 'closed'
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.account.off('epoch', this.epochListener)
    this.handshake?.abandon(); this.handshake = null
    const channel = this.channel; this.channel = null; channel?.close()
    const socket = this.socket; this.socket = null
    try { socket?.close(1000) } catch {}
    this.finishOpening({ kind: 'unavailable' })
  }

  /** Um socket e um handshake; presença repetida nunca inicia outro desafio. */
  async start(): Promise<BootstrapChannelResult<void>> {
    if (this.state !== 'new' || !this.current() || this.principal.deployment.origin.startsWith('http:')) return { kind: 'notSent' }
    this.state = 'opening'; this.armDeadline()
    if (!this.current()) return { kind: 'notSent' }
    try {
      const url = deviceRelayURL(this.principal.deployment.origin, this.expected.computerId)
      url.searchParams.set('purpose', 'bootstrap'); url.searchParams.set('bootstrapId', this.expected.bootstrapId)
      if (!this.current()) return { kind: 'notSent' }
      // A assinatura do upgrade usa a MESMA chave do binding e cobre a query exata.
      const header = signatureHeader(this.signer, 'GET', url)
      if (!this.current()) return { kind: 'notSent' }
      const socket = this.socketFactory(url.toString(), { 'Codync-Sig': header })
      if (!this.current()) { socket.close(1000); return { kind: 'notSent' } }
      this.socket = socket
      return await new Promise<BootstrapChannelResult<void>>((resolve) => {
        this.opening = resolve
        socket.addEventListener('message', event => {
          if (!this.current() || this.socket !== socket) { this.close(); return }
          let message: { t?: unknown; online?: unknown; c?: unknown; d?: unknown; ek?: unknown; sig?: unknown; code?: unknown }
          try { if (typeof event.data !== 'string' || Buffer.byteLength(event.data, 'utf8') > 24_000) throw new Error('Invalid frame'); message = JSON.parse(event.data) } catch { this.close(); return }
          if (!message || typeof message !== 'object' || Array.isArray(message)) { this.close(); return }
          if (message.t === 'presence') {
            if (message.online !== true || this.state !== 'opening' || this.handshake) { this.close(); return }
            const host = unb64url(this.expected.signKey, 32)
            if (!host || !this.current()) { this.close(); return }
            try {
              this.handshake = new Handshake(this.signer, host, { send: frame => this.wireSend(frame), sendRaw: hello => this.wireSend({ ...hello, pair: true, purpose: 'bootstrap', bootstrapId: this.expected.bootstrapId }) })
            } catch { this.close() }
            return
          }
          if (message.t === 'welcome' || message.t === 'reject') {
            const hs = this.handshake
            if (!hs || !hs.receive(message) || !hs.result) { this.close(); return }
            this.handshake = null
            if (hs.result.kind !== 'ready' || !this.current()) { this.close(); return }
            this.channel = hs.result.channel; this.state = 'open'; this.finishOpening({ kind: 'ok', value: undefined }); return
          }
          if (message.t === 'f' && this.channel) {
            try { this.channel.receive(message) } catch { this.close() }
            return
          }
          if (message.t !== 'pong') this.close()
        })
        socket.addEventListener('error', () => this.close())
        socket.addEventListener('close', () => this.close())
      })
    } catch { this.close(); return { kind: 'unavailable' } }
  }

  async challenge(): Promise<BootstrapChannelResult<void>> {
    if (this.state !== 'open' || !this.current() || !this.channel) return { kind: 'notSent' }
    this.state = 'challenging'
    let result: CallResult
    try { result = await this.channel.call('bootstrap.challenge', {}, Math.max(1, Math.min(20_000, Math.floor(this.deadlineMono - performance.now())))) }
    catch { this.close(); return { kind: 'unknown' } }
    if (!this.current() || this.state !== 'challenging') { this.close(); return { kind: 'unknown' } }
    if (result.kind !== 'ok') { this.close(); return { kind: result.kind === 'notSent' ? 'notSent' : 'unknown' } }
    try {
      this.grant = verifyBootstrapGrant(this.grantWire, this.expected, this.rootKey, Date.now())
      const checked = verifyBootstrapChallenge(result.value, this.expected, this.grant, Date.now())
      this.challengeEnvelope = copy(checked.original)
      const expires = checked.payload[8] as number
      this.challengeWall = expires
      this.challengeDeadline = performance.now() + Math.max(0, expires - Date.now())
      this.state = 'challenged'; this.armDeadline()
      if (!this.current()) { this.close(); return { kind: 'unknown' } }
      return { kind: 'ok', value: undefined }
    } catch { this.close(); return { kind: 'unknown' } }
  }

  /** Apenas clique explícito do consumidor; callback persiste originais antes do envio. */
  async prove(): Promise<BootstrapChannelResult<BootstrapReceipt>> {
    const challenge = this.challengeEnvelope, channel = this.channel
    if (this.state !== 'challenged' || !challenge || !channel || !this.current()) return { kind: 'notSent' }
    this.state = 'proving'
    let proof: BootstrapEnvelope
    try {
      const grant = verifyBootstrapGrant(this.grantWire, this.expected, this.rootKey, Date.now())
      const verified = verifyBootstrapChallenge(challenge, this.expected, grant, Date.now())
      if (!this.current() || this.channel !== channel) { this.close(); return { kind: 'notSent' } }
      const payload = ['donna/bootstrap-proof/v1', this.expected.bootstrapId, this.expected.hubGeneration, grant.hash, verified.hash]
      const signature = this.signer.sign(Buffer.from(JSON.stringify(payload)))
      if (!this.current() || this.channel !== channel) { this.close(); return { kind: 'notSent' } }
      proof = [payload, b64url(signature)]
      const journal = { grant: copy(this.grantWire), challenge: copy(challenge), proof: copy(proof) }
      // Persistir a fase de prova incerta ANTES de a mensagem poder sair.
      await this.persist(journal)
      if (!this.current() || this.channel !== channel || this.state !== 'proving') { this.close(); return { kind: 'notSent' } }
      const freshGrant = verifyBootstrapGrant(this.grantWire, this.expected, this.rootKey, Date.now())
      const freshChallenge = verifyBootstrapChallenge(challenge, this.expected, freshGrant, Date.now())
      if (payload[3] !== freshGrant.hash || payload[4] !== freshChallenge.hash || sha(journal) !== sha({ grant: this.grantWire, challenge, proof })) { this.close(); return { kind: 'notSent' } }
    } catch { this.close(); return { kind: 'notSent' } }
    let result: CallResult
    try { result = await channel.call('bootstrap.prove', proof, Math.max(1, Math.min(20_000, Math.floor(Math.min(this.deadlineMono, this.challengeDeadline) - performance.now())))) }
    catch { this.close(); return { kind: 'unknown' } }
    if (!this.current() || this.channel !== channel || this.state !== 'proving') { this.close(); return { kind: 'unknown' } }
    if (result.kind !== 'ok') { this.close(); return { kind: result.kind === 'notSent' ? 'notSent' : 'unknown' } }
    try {
      const originals: BootstrapOriginals = { grant: copy(this.grantWire), challenge: copy(challenge), proof: copy(proof), receipt: copy(result.value) as BootstrapEnvelope }
      const verified = verifyBootstrapChain(originals, this.expected, this.rootKey, Date.now())
      if (!this.current() || this.channel !== channel) { this.close(); return { kind: 'unknown' } }
      this.close()
      return { kind: 'ok', value: { originals, verified } }
    } catch { this.close(); return { kind: 'unknown' } }
  }
}
