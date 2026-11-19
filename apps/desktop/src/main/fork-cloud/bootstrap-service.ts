import type { AccountService, DeviceBinding } from '../fork-account/service'
import type { ForkBootstrapState } from '../../shared/fork-cloud'
import type { Principal } from './principal'
import { AccountApi } from './account-api'
import { bootstrapAuthority, type BootstrapAuthority } from './bootstrap-authority'
import { BootstrapChannel } from './bootstrap-channel'
import { verifyBootstrapActivation, verifyBootstrapChain, verifyBootstrapClosure, verifyBootstrapGrant, type BootstrapExpected, type BootstrapOriginals, type BootstrapEnvelope } from './bootstrap-codec'
import { advanceBootstrapJournal, createBootstrapJournal, readBootstrapJournal, type BootstrapJournal, type BootstrapJournalData, type BootstrapPhase } from './bootstrap-journal'
import { prepareBootstrapPin, publishBootstrapPin } from './bootstrap-pin'
import { computerIdOf, unb64url } from './crypto'

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
const string = (v: unknown): v is string => typeof v === 'string' && v.length > 0 && v.length <= 256
const time = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v > 0
type Action = 'startBootstrap' | 'checkBootstrap' | 'proveBootstrap' | 'reconcileBootstrap'
const PRE_PROOF: readonly BootstrapPhase[] = ['requested','openingUnknown','challengeUnknown','challenged']
const REMOTE_STATES = ['requested','attested','hubPending','published','hubActive','canceled','expired','refused']

/** One captured account/device/frame. Durable journal is restored locally; no automatic network. */
export class BootstrapService {
  private journal: BootstrapJournal | null = null
  private busy = false
  private stopped = false
  private readUnavailable = false
  private channel: BootstrapChannel | null = null
  private readonly binding: DeviceBinding
  private readonly root: BootstrapAuthority
  private readonly onEpoch = () => this.stop()
  private constructor(private readonly account: AccountService, private readonly p: Principal, binding: DeviceBinding, root: BootstrapAuthority) {
    this.binding = Object.freeze({ ...binding }); this.root = root
    account.on('epoch', this.onEpoch)
  }
  static async restore(account: AccountService, p: Principal): Promise<BootstrapService | null> {
    const root = bootstrapAuthority(p), binding = account.verifiedDeviceBinding(p.seal)
    if (!root || !binding || !p.current()) return null
    const service = new BootstrapService(account, p, binding, root)
    try {
      service.journal = await readBootstrapJournal(p, service.binding)
      const j = service.journal
      if (j?.phase === 'closed') verifyBootstrapClosure(j.data.terminal!, j.data.originals as BootstrapOriginals, j.data.expected!, root.issuerPublicKey)
      service.check(); return service
    }
    catch (error) { service.stop(); throw error }
  }
  current(): boolean {
    return !this.stopped && this.p.current() && this.account.isCloudSessionCurrent(this.p.seal) &&
      same(this.account.verifiedDeviceBinding(this.p.seal), this.binding) && same(bootstrapAuthority(this.p), this.root)
  }
  private check() { if (!this.current()) throw new Error('Bootstrap superseded.') }
  stop() { this.stopped = true; this.channel?.close(); this.channel = null; this.account.off('epoch', this.onEpoch) }
  private get deadline(): number | undefined {
    const d = this.journal?.data
    if (!d?.expected) return undefined
    const grant = d.originals.grant?.[0][19]
    const challenge = d.originals.challenge?.[0][8]
    return Math.min(d.expected.expiresAt, typeof grant === 'number' ? grant : d.expected.expiresAt,
      typeof challenge === 'number' ? challenge : d.expected.expiresAt)
  }
  get blocksOtherOperations(): boolean {
    const j = this.journal
    return this.current() && j !== null && j.phase !== 'complete' && j.phase !== 'closed' && j.phase !== 'prepared' &&
      !(PRE_PROOF.includes(j.phase) && !j.data.originals.proof && !j.data.originals.receipt &&
        this.deadline !== undefined && Date.now() >= this.deadline)
  }
  snapshot(): ForkBootstrapState | null {
    if (!this.current()) return null
    const j = this.journal
    if (!j) return { bootstrapId: '', phase: 'available', blocksOtherOperations:false, canStart: !this.busy, canCheck: false, canProve: false, canReconcile: false }
    return { bootstrapId: j.bootstrapId, phase: j.phase, blocksOtherOperations:this.blocksOtherOperations, canStart: !this.busy && j.phase === 'prepared', canCheck: !this.busy && j.phase !== 'prepared' && j.phase !== 'closed',
      canProve: !this.busy && j.phase === 'requested' && !!j.data.originals.grant && this.deadline !== undefined && Date.now() < this.deadline,
      canReconcile: !this.busy && ['receipt','reconcileUnknown','published','pinUnknown'].includes(j.phase),
      ...(j.data.expected ? { computerId: j.data.expected.computerId, deadline: this.deadline } : {}),
      ...(j.data.terminal ? { terminalReason: j.data.terminal.reason } : {}),
      ...(this.readUnavailable ? { readUnavailable: true as const } : {}) }
  }
  private async advance(phase: BootstrapPhase, data: BootstrapJournalData = this.journal!.data) {
    this.check()
    if (!this.journal) throw new Error('Bootstrap missing.')
    const next = await advanceBootstrapJournal(this.p, this.binding, this.journal, phase, data)
    this.check(); this.journal = next
  }
  private expected(v: Record<string, unknown>, j: BootstrapJournal): BootstrapExpected {
    const host = unb64url(v.hubSignKey, 32)
    if (v.bootstrapId !== j.bootstrapId || v.nonceApp !== j.nonceApp || v.ownerUserId !== j.ownerUserId ||
        v.deviceId !== j.deviceId || v.deviceKey !== j.deviceKey || v.deploymentId !== j.deploymentId ||
        v.env !== j.env || v.issuer !== j.issuer || v.manifestHash !== j.manifestHash ||
        !string(v.provisioningIntentId) || !string(v.resourceDigest) || !/^[a-f0-9]{64}$/.test(v.resourceDigest) ||
        !string(v.hubGeneration) || !unb64url(v.hubGeneration, 32) || !host || !string(v.hubSignKey) ||
        !string(v.hubComputerId) || computerIdOf(host) !== v.hubComputerId || !string(v.hubBoxKey) || !unb64url(v.hubBoxKey, 32) ||
        !time(v.createdAt) || v.createdAt > Date.now() + 30_000 || !time(v.expiresAt) || v.expiresAt <= v.createdAt) throw new Error('Different bootstrap intention.')
    return { bootstrapId: j.bootstrapId, provisioningIntentId: v.provisioningIntentId, resourceDigest: v.resourceDigest,
      hubGeneration: v.hubGeneration, issuerKeyId: this.root.issuerKeyId, issuer: j.issuer, deploymentId: j.deploymentId,
      env: j.env, manifestHash: j.manifestHash, ownerUserId: j.ownerUserId, deviceId: j.deviceId, deviceKey: j.deviceKey,
      nonceApp: j.nonceApp, computerId: v.hubComputerId, signKey: v.hubSignKey, boxKey: v.hubBoxKey, createdAt: v.createdAt, expiresAt: v.expiresAt }
  }
  private async api(): Promise<AccountApi> {
    this.check()
    const api = new AccountApi(this.p), health = await api.execute({ kind: 'health' })
    this.check()
    if (health.kind !== 'ok') throw new Error('Bootstrap deployment unavailable.')
    return api
  }
  private async adopt(v: Record<string, unknown>) {
    const j = this.journal!
    const expected = this.expected(v, j)
    if (j.data.expected && !same(j.data.expected, expected)) throw new Error('Bootstrap generation changed.')
    let originals = { ...j.data.originals }
    if (v.grant !== undefined && v.grant !== null) {
      // Only an uncommitted attempt requires a currently live Grant. Recovery
      // verifies the original deadline at the Hub's signed committedAt instead.
      if (!v.receipt && !originals.receipt) verifyBootstrapGrant(v.grant, expected, this.root.issuerPublicKey, Date.now())
      if (originals.grant && !same(originals.grant, v.grant)) throw new Error('Bootstrap grant changed.')
      originals.grant = v.grant as BootstrapEnvelope
    }
    if (v.receipt !== undefined && v.receipt !== null) {
      const r = v.receipt as Record<string, unknown>
      const candidate = { grant: r.grant, challenge: r.challenge, proof: r.proof, receipt: r.receipt } as BootstrapOriginals
      verifyBootstrapChain(candidate, expected, this.root.issuerPublicKey, Date.now())
      for (const key of ['grant','challenge','proof','receipt'] as const) if (originals[key] && !same(originals[key], candidate[key])) throw new Error('Bootstrap originals changed.')
      originals = candidate
    }
    const data = { ...j.data, expected, originals }
    // A retained pin may already have a pending file. Keep its exact local
    // finalization available; the historical pin never replaces current admission.
    if ((v.state === 'expired' || v.state === 'canceled') && originals.receipt && j.phase !== 'pinUnknown' && j.phase !== 'complete') {
      const chain = verifyBootstrapChain(originals as BootstrapOriginals, expected, this.root.issuerPublicKey, Date.now())
      let revocation: BootstrapEnvelope | null = null
      if (v.state === 'canceled') {
        const r = v.revocation as Record<string, unknown> | null
        if (!r || r.status !== 'confirmed') return
        if (r.bootstrapId !== j.bootstrapId || r.grantId !== chain.grantId || r.hashHubReceipt !== chain.hashReceipt) throw new Error('Different terminal bootstrap receipt.')
        revocation = r.revocation as BootstrapEnvelope
      }
      const terminal = verifyBootstrapClosure({
        reason: v.state, revision: v.revision as number, observedAt: Date.now(),
        grantId: chain.grantId, hashHubReceipt: chain.hashReceipt, revocation,
      }, originals as BootstrapOriginals, expected, this.root.issuerPublicKey)
      // Store and sync the historical closure before releasing other operations.
      await this.advance('closed', { ...data, terminal })
      return
    }
    if (v.state === 'published' || v.state === 'hubActive') {
      if (!originals.receipt) throw new Error('Missing committed bootstrap receipt.')
      verifyBootstrapChain(originals as BootstrapOriginals, expected, this.root.issuerPublicKey, Date.now())
      if (v.state === 'hubActive' && v.activation) data.activation = verifyBootstrapActivation(v.activation, originals as BootstrapOriginals, expected, this.root.issuerPublicKey, Date.now())
      // A phase is never rolled backwards, and a stored pin's exact bytes remain frozen.
      if (j.phase !== 'pinUnknown' && j.phase !== 'complete') await this.advance('published', data)
    } else if (originals.receipt) {
      if (j.phase !== 'reconcileUnknown' && j.phase !== 'published' && j.phase !== 'pinUnknown' && j.phase !== 'complete') await this.advance('receipt', data)
    } else if (['prepared','createUnknown','requested'].includes(j.phase)) {
      await this.advance('requested', data)
    }
  }
  private async read(api: AccountApi): Promise<string | null> {
    const j = this.journal!
    const result = await api.execute({ kind: 'bootstrapRead', bootstrapId: j.bootstrapId })
    this.check()
    if (result.kind !== 'ok') { this.readUnavailable = true; return null }
    if (typeof result.value.state !== 'string' || !REMOTE_STATES.includes(result.value.state)) throw new Error('Invalid bootstrap state.')
    await this.adopt(result.value); this.check(); this.readUnavailable = false
    return result.value.state
  }
  private async publishPin() {
    const j = this.journal!, d = j.data
    if (j.phase === 'published') {
      if (!d.expected || !d.originals.receipt || !d.activation) return
      const retained = await prepareBootstrapPin(this.p, d.expected, d.originals as BootstrapOriginals, d.activation)
      this.check(); await this.advance('pinUnknown', { ...d, ...retained })
    }
    const current = this.journal!
    if (current.phase !== 'pinUnknown' || !current.data.expected || !current.data.sealedPin || !current.data.pinHash) return
    await publishBootstrapPin(this.p, current.data.expected.computerId, current.data.sealedPin, current.data.pinHash)
    this.check(); await this.advance('complete')
  }
  async invoke(action: Action, bootstrapId?: string): Promise<void> {
    this.check()
    if (this.journal?.phase === 'closed') return
    if (this.busy || action !== 'startBootstrap' && (!this.journal || this.journal.bootstrapId !== bootstrapId)) return
    this.busy = true
    try {
      if (action === 'startBootstrap') {
        if (this.journal && this.journal.phase !== 'prepared') return
        const api = await this.api()
        if (!this.journal) this.journal = await createBootstrapJournal(this.p, this.binding)
        this.check()
        await this.advance('createUnknown')
        const result = await api.execute({ kind: 'bootstrapCreate', bootstrapId: this.journal.bootstrapId, nonceApp: this.journal.nonceApp })
        this.check()
        if (result.kind === 'ok') await this.adopt(result.value)
        else {
          if (result.kind === 'notSent' || result.kind === 'refused' && ['badRequest','badSignature','unauthenticated','forbidden','bootstrapUnavailable'].includes(result.code)) await this.advance('prepared')
          this.readUnavailable = true
        }
      } else if (action === 'checkBootstrap') {
        // Restore/read is never a proof or POST, including unknown after restart.
        const api = await this.api(); await this.read(api)
      } else if (action === 'proveBootstrap') {
        const j = this.journal!
        if (j.phase !== 'requested' || !j.data.expected || !j.data.originals.grant) return
        const channel = await BootstrapChannel.create(this.account, this.p, j.data.expected, this.root.issuerPublicKey, j.data.originals.grant, async originals => {
          this.check(); await this.advance('proofUnknown', { ...this.journal!.data, originals: { ...originals } })
        })
        this.check(); this.channel = channel
        try {
          await this.advance('openingUnknown')
          if ((await channel.start()).kind !== 'ok') return
          this.check(); await this.advance('challengeUnknown')
          if ((await channel.challenge()).kind !== 'ok') return
          this.check(); await this.advance('challenged')
          const result = await channel.prove(); this.check()
          if (result.kind === 'ok') await this.advance('receipt', { ...this.journal!.data, originals: result.value.originals })
        } finally { channel.close(); if (this.channel === channel) this.channel = null }
      } else if (action === 'reconcileBootstrap') {
        if (this.journal!.phase === 'complete') return
        // Local pin sync only: these bytes were validated and retained before any write.
        if (this.journal!.phase === 'pinUnknown') { await this.publishPin(); return }
        const api = await this.api()
        const observed = await this.read(api)
        if (!observed) return
        if (observed === 'hubPending' && ['receipt','reconcileUnknown'].includes(this.journal!.phase)) {
          // This explicit action reconciles one immutable receipt. The Worker CAS
          // returns its existing publication; it never signs/proves/creates again.
          if (this.journal!.phase === 'receipt') await this.advance('reconcileUnknown')
          const result = await api.execute({ kind: 'bootstrapReconcile', bootstrapId: this.journal!.bootstrapId })
          this.check()
          if (result.kind === 'notSent' || result.kind === 'refused' &&
              ['badRequest','badSignature','unauthenticated','forbidden'].includes(result.code)) await this.advance('receipt')
          // The mutation's 200 is not proof. Read the same intent and signed activation.
          if (!(await this.read(api))) return
        }
        await this.publishPin()
      }
    } finally { this.busy = false }
  }
}
