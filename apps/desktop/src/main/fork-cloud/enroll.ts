import { createHash, randomBytes } from 'node:crypto'
import { constants, promises as fs } from 'node:fs'
import { dirname } from 'node:path'
import { safeStorage } from 'electron'
import { b64url, computerIdOf, random, sasCode, sasCommit, unb64url } from './crypto'
import { deviceSigner } from './device-keys'
import { AccountApi } from './account-api'
import { pinMessage, pinPath, readPin, readPinBytes, securePinsAvailable, verifyPin, type PinReceipt, type StoredPin } from './pins'
import type { Principal } from './principal'
import type { ForkCloudState } from '../../shared/fork-cloud'
import type { CapturedSSHLocal } from '../../shared/fork-cloud-local'

/** No arbitrary renderer object may stand in for a Local capture. */
export const localEnrollmentUnavailable = () => ({ kind: 'unavailable' as const })
const grant = (value: unknown): value is string => typeof value === 'string' && /^grt_[A-Za-z0-9_-]{1,60}$/.test(value)
const record = (value: unknown): Record<string, unknown> | null => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null
export type EnrollmentEffect = { readonly attemptId: string; readonly computerId: string; readonly phase: 'request' | 'reveal' | 'approval' | 'publication'; readonly publicationHash?: string }

/** A request fixes the Hub key before its commit. Only this closed producer can
 * publish: a verified Local decision, or an explicit code comparison followed by
 * approval of the same request. Cloud status alone never issues a pin. */
export class Enrollment {
  readonly id = randomBytes(16).toString('base64url')
  private readonly epoch: number
  private readonly api: AccountApi
  private readonly hostKey: Uint8Array
  private readonly deviceNonce = random(32)
  private stopped = false
  private checking = false
  private phase: 'new' | 'starting' | 'waiting' | 'revealing' | 'revealed' | 'approvalConfirmed' | 'approvalUnknown' | 'publishing' | 'linked' = 'new'
  private requestId: string | null = null
  private deadline = 0
  private hostNonce: string | null = null
  private code: string | null = null
  private deviceKey: Uint8Array | null = null
  private comparisonAt: number | null = null
  private localTranscript: string | null = null
  private approvedGrant: string | null = null
  private publication: { value: StoredPin; bytes: Buffer } | null = null
  private effect: EnrollmentEffect['phase'] | null = null

  constructor(private readonly principal: Principal, readonly computerId: string, signKey: string, private readonly local?: CapturedSSHLocal) {
    const key = unb64url(signKey, 32)
    if (!key || computerIdOf(key) !== computerId || !principal.current() || (local && (!local.current() || local.computerId !== computerId || local.signKey !== signKey))) throw new Error('Invalid directory identity.')
    this.hostKey = new Uint8Array(key)
    this.api = new AccountApi(principal)
    this.epoch = principal.seal.epoch
  }
  private current() { return !this.stopped && this.principal.current() && this.principal.seal.epoch === this.epoch && (!this.local || this.local.current()) }
  private requireCurrent() { if (!this.current()) throw new Error('Enrollment superseded.') }
  private async sameDevice(): Promise<boolean> {
    if (!this.current() || !this.deviceKey) return false
    try { const signer = await deviceSigner(this.principal, 'read'); return this.current() && b64url(signer.publicKey) === b64url(this.deviceKey) } catch { return false }
  }
  private state(kind: ForkCloudState['kind'], extras: Partial<ForkCloudState> = {}): ForkCloudState {
    return { kind, epoch: this.epoch, attemptId: this.id, computerId: this.computerId, ...extras }
  }
  cancel() { this.stopped = true }
  /** Remains observable after cancellation; only a result of this effect clears it. */
  get pendingEffect(): EnrollmentEffect | null {
    return this.effect ? { attemptId: this.id, computerId: this.computerId, phase: this.effect, ...(this.effect === 'publication' && this.publication ? { publicationHash: createHash('sha256').update(this.publication.bytes).digest('hex') } : {}) } : null
  }
  get ended(): boolean { return this.stopped && this.effect === null }
  private readUnavailable(): ForkCloudState {
    return this.state(this.phase === 'revealed' ? 'waitingOwnerApproval' : 'linking', {
      ...(this.phase === 'revealed' && this.code ? { code: this.code } : {}),
      ...(this.deadline ? { deadline: this.deadline } : {}), readUnavailable: true,
    })
  }
  async start(): Promise<ForkCloudState> {
    if (!this.current() || this.phase !== 'new') return this.state('error')
    this.phase = 'starting'
    try {
      const health = await this.api.execute({ kind: 'health' })
      if (!this.current() || health.kind !== 'ok') { this.cancel(); return this.state('deploymentChanged') }
      const deviceKey = await this.api.captureDeviceKey()
      if (!this.current()) return this.state('error')
      this.deviceKey = deviceKey
      this.effect = 'request'
      const response = await this.api.execute({ kind: 'requestAccess', computerId: this.computerId, commit: b64url(sasCommit(deviceKey, this.deviceNonce)) })
      if (response.kind === 'notSent' || response.kind === 'unavailable' || response.kind === 'refused') this.effect = null
      if (!this.current() || response.kind !== 'ok' || !(await this.sameDevice())) { this.cancel(); return this.state(this.effect ? 'unknown' : response.kind === 'notSent' ? 'notSent' : 'error') }
      const { requestId, expiresAt } = response.value
      if (typeof requestId !== 'string' || !/^req_[A-Za-z0-9_-]{1,60}$/.test(requestId) || typeof expiresAt !== 'number' || !Number.isSafeInteger(expiresAt) || expiresAt <= Date.now()) { this.cancel(); return this.state('unknown') }
      this.effect = null
      this.requestId = requestId; this.deadline = Math.min(expiresAt, Date.now() + 10 * 60_000); this.phase = 'waiting'
      return this.state('linking', { deadline: this.deadline })
    } catch { this.cancel(); return this.state(this.effect ? 'unknown' : 'error') }
  }
  /** The UI must ask the person to compare against their owner channel and type
   * its code. No boolean, directory status or default confirmation is accepted. */
  async confirmCode(code: string): Promise<ForkCloudState> {
    if (this.checking) return this.state('notSent')
    if (!this.current() || this.local || this.phase !== 'revealed' || !this.code || Date.now() > this.deadline || !/^[0-9]{6}$/.test(code) || code !== this.code) {
      this.cancel(); return this.state('hubUnverified')
    }
    this.comparisonAt = Date.now()
    return this.check()
  }
  async check(): Promise<ForkCloudState> {
    if (this.checking) return this.state('notSent')
    this.checking = true
    try {
      if (!this.current()) return this.state('error')
      if (this.phase === 'linked') return this.state('linked')
      if (this.phase === 'approvalUnknown') return this.state('unknown')
      if (this.phase === 'publishing') return await this.reconcilePublication()
      if (this.effect) return this.state('unknown')
      if (!this.requestId || !this.deviceKey || !['waiting', 'revealed', 'approvalConfirmed'].includes(this.phase) || Date.now() > this.deadline) { this.cancel(); return this.state('error') }
      if (!(await this.sameDevice())) { this.cancel(); return this.state('error') }
      const list = await this.api.execute({ kind: 'listComputers' })
      if (!this.current()) return this.state('error')
      if (list.kind !== 'ok' || !Array.isArray(list.value.computers)) return this.readUnavailable()
      const found = list.value.computers.filter((entry: { computerId?: unknown }) => entry?.computerId === this.computerId)
      if (found.length !== 1 || found[0]?.signKey !== b64url(this.hostKey)) { this.cancel(); return this.state('hubUnverified') }
      const reply = await this.api.execute({ kind: 'getAccess', requestId: this.requestId })
      if (!this.current()) return this.state('error')
      if (reply.kind !== 'ok') return this.readUnavailable()
      if (reply.value.requestId !== this.requestId || reply.value.computerId !== this.computerId || typeof reply.value.expiresAt !== 'number' || !Number.isSafeInteger(reply.value.expiresAt)) { this.cancel(); return this.state('error') }
      this.deadline = Math.min(this.deadline, reply.value.expiresAt)
      if (Date.now() > this.deadline) { this.cancel(); return this.state('accessEnded') }
      const nonce = reply.value.hostNonce
      if (nonce === undefined && this.hostNonce === null && reply.value.status === 'pending') return this.state('linking', { deadline: this.deadline })
      if (typeof nonce !== 'string' || !unb64url(nonce, 32) || (this.hostNonce !== null && this.hostNonce !== nonce)) { this.cancel(); return this.state('hubUnverified') }
      if (this.hostNonce === null) {
        if (reply.value.status !== 'pending' || !(await this.sameDevice())) { this.cancel(); return this.state('error') }
        this.hostNonce = nonce
        this.code = sasCode(this.hostKey, this.deviceKey, this.deviceNonce, unb64url(nonce, 32)!)
        this.phase = 'revealing'
        this.effect = 'reveal'
        const revealed = await this.api.execute({ kind: 'revealAccess', requestId: this.requestId, nonce: b64url(this.deviceNonce) })
        if (revealed.kind === 'notSent' || revealed.kind === 'unavailable' || revealed.kind === 'refused') this.effect = null
        if (!this.current() || revealed.kind !== 'ok' || !(await this.sameDevice()) || Date.now() > this.deadline) { this.cancel(); return this.state(this.effect ? 'unknown' : 'error') }
        this.effect = null
        this.phase = 'revealed'
        // A response read before reveal cannot approve the new transcript.
        return this.state('waitingOwnerApproval', { code: this.code, deadline: this.deadline })
      }
      if (reply.value.status !== 'pending' && reply.value.status !== 'approved') { this.cancel(); return this.state('accessEnded') }
      if (this.phase === 'approvalConfirmed') {
        if (reply.value.status !== 'approved' || reply.value.grantId !== this.approvedGrant) return this.readUnavailable()
        return await this.publish()
      }
      if (this.local) return await this.approveLocal()
      if (this.comparisonAt === null || reply.value.status !== 'approved') return this.state('waitingOwnerApproval', { code: this.code!, deadline: this.deadline })
      if (!grant(reply.value.grantId)) { this.cancel(); return this.state('hubUnverified') }
      this.approvedGrant = reply.value.grantId
      this.phase = 'approvalConfirmed'
      return await this.publish()
    } catch (error) {
      if (this.effect === 'approval' && this.local?.wasNotSent(error)) { this.effect = null; this.phase = 'revealed' }
      return this.effect ? this.state('unknown') : !this.current() ? this.state('error') : this.readUnavailable()
    } finally { this.checking = false }
  }
  private async approveLocal(): Promise<ForkCloudState> {
    const local = this.local!
    const status = record(await local.execute({ kind: 'cloudStatus' }))
    this.requireCurrent()
    if (status?.enabled !== true || status.url !== this.principal.deployment.origin || record(status.owner)?.userId !== this.principal.seal.userId) { this.cancel(); return this.state('accountMismatch') }
    const response = record(await local.execute({ kind: 'accessRequests' }))
    this.requireCurrent()
    const rows = Array.isArray(response?.requests) ? response.requests.filter((row: unknown) => record(row)?.requestId === this.requestId) : []
    const request = record(rows[0])
    if (rows.length !== 1 || request?.deviceKey !== b64url(this.deviceKey!) || request.code !== this.code || !unb64url(request.transcript, 32) || !Number.isSafeInteger(request.deadline) || request.deadline !== request.expiresAt || (request.deadline as number) < Date.now() || Date.now() > this.deadline || !(await this.sameDevice())) {
      this.cancel(); return this.state('hubUnverified')
    }
    this.requireCurrent()
    // The incarnation is private to the Hub. Preserve its Local transcript; do
    // not pretend it can be recomputed using an app attemptId or cloud status.
    this.localTranscript = request.transcript as string
    this.comparisonAt = Date.now()
    this.phase = 'approvalUnknown' // Set before send; never auto-replay a lost decision.
    this.effect = 'approval'
    const decision = record(await local.execute({ kind: 'decideAccessRequest', requestId: this.requestId!, expect: { deviceKey: b64url(this.deviceKey!), code: this.code!, transcript: this.localTranscript } }))
    this.requireCurrent()
    if (decision?.result === 'refused' || decision?.result === 'denied') { this.effect = null; this.cancel(); return this.state('accessEnded') }
    if (decision?.result !== 'approved' || decision.deviceKey !== b64url(this.deviceKey!) || !grant(decision.grantId)) return this.state('unknown')
    this.approvedGrant = decision.grantId
    this.effect = null
    this.phase = 'approvalConfirmed'
    // Reconcile the exact confirmed grant on the next explicit check; the Local
    // ACL acknowledgement is retained and must not be sent again.
    return this.state('linking', { deadline: this.deadline })
  }
  private async publish(): Promise<ForkCloudState> {
    this.requireCurrent()
    if (!this.deviceKey || !this.hostNonce || !this.code || !this.requestId || !this.approvedGrant || !this.comparisonAt || Date.now() > this.deadline || !(await this.sameDevice()) || !securePinsAvailable()) return this.readUnavailable()
    this.requireCurrent()
    const existing = await readPin(this.principal, this.computerId)
    this.requireCurrent()
    if (existing) {
      if (existing.signKey !== b64url(this.hostKey)) throw new Error('Existing pin differs.')
      this.phase = 'linked'; this.effect = null; return this.state('linked')
    }
    const receipt: PinReceipt = Object.freeze({ version: 2, namespace: this.principal.namespace, computerId: this.computerId, signKey: b64url(this.hostKey), source: this.local ? 'A1' : 'A2', deviceKey: b64url(this.deviceKey), requestId: this.requestId, grantId: this.approvedGrant, attemptId: this.id, deviceNonce: b64url(this.deviceNonce), hostNonce: this.hostNonce, commit: b64url(sasCommit(this.deviceKey, this.deviceNonce)), code: this.code, deadline: this.deadline, confirmedAt: this.comparisonAt, localTranscript: this.localTranscript })
    const signer = await deviceSigner(this.principal, 'read')
    this.requireCurrent()
    if (b64url(signer.publicKey) !== receipt.deviceKey) throw new Error('Device changed.')
    const value: StoredPin = { receipt, signature: b64url(signer.sign(pinMessage(receipt))) }
    await verifyPin(this.principal, this.computerId, value); this.requireCurrent()
    const bytes = safeStorage.encryptString(JSON.stringify(value))
    this.publication = { value, bytes }
    this.phase = 'publishing'
    this.effect = 'publication'
    const path = pinPath(this.principal, this.computerId)
    // Exclusive pending file is both the full encrypted receipt and the fence.
    // Never replace an existing pin or a different pending intention.
    const out = await fs.open(`${path}.pending`, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    try { this.requireCurrent(); await out.writeFile(bytes); await out.sync() } finally { await out.close() }
    this.requireCurrent()
    return await this.reconcilePublication()
  }
  private async reconcilePublication(): Promise<ForkCloudState> {
    const publication = this.publication
    if (!publication || !this.current() || !(await this.sameDevice())) return this.state('unknown')
    const path = pinPath(this.principal, this.computerId)
    const pending = `${path}.pending`
    try {
      let raw: Buffer
      try { raw = await readPinBytes(pending) }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
        // The fence may already have been removed before directory sync failed.
        // Reconcile only our retained exact bytes; never recreate a missing file.
        raw = await readPinBytes(path); this.requireCurrent()
        if (!raw.equals(publication.bytes)) throw new Error('Different stored pin.')
        const file = await fs.open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
        try { await file.sync() } finally { await file.close() }
        const dir = await fs.open(dirname(path), 'r')
        try { await dir.sync(); this.requireCurrent() } finally { await dir.close() }
        this.phase = 'linked'; this.effect = null; return this.state('linked')
      }
      this.requireCurrent()
      if (!raw.equals(publication.bytes)) throw new Error('Different pending publication.')
      await verifyPin(this.principal, this.computerId, publication.value); this.requireCurrent()
      // Same intention only: EEXIST is reconciled by byte equality, not replaced.
      try { await fs.link(pending, path) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
      this.requireCurrent()
      if (!(await readPinBytes(path)).equals(publication.bytes)) throw new Error('Different stored pin.')
      this.requireCurrent()
      const file = await fs.open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
      try { await file.sync() } finally { await file.close() }
      const dir = await fs.open(dirname(path), 'r')
      try {
        await dir.sync(); this.requireCurrent()
        await fs.unlink(pending)
        await dir.sync(); this.requireCurrent()
      } finally { await dir.close() }
      this.phase = 'linked'
      this.effect = null
      return this.state('linked')
    } catch { return this.state('unknown') }
  }
}
