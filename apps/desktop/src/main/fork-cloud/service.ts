import { cloudSubscription } from '../../shared/fork-cloud'
// Captura as funções nativas antes de registrar qualquer IPC pessoal.
import './personal-runtime'
import { randomBytes } from 'node:crypto'
import { app, BrowserWindow, ipcMain, powerMonitor, type IpcMainInvokeEvent, type IpcMainEvent } from 'electron'
import type { ForkCloudIntent, ForkCloudResult, ForkCloudState, ForkCloudKind, ForkCloudCallResult } from '../../shared/fork-cloud'
import type { Subscription } from './channel'
import type { FrameSink } from '../../shared/relay-protocol/channel.ts'
import { createFrameGate, STALE_AFTER_MS } from '../../shared/fork-browser-live'
import type { BrowserFrame } from '../../shared/relay-protocol/browser-frames.ts'
import type { CapturedSSHLocal } from '../../shared/fork-cloud-local'
import type { AccountService } from '../fork-account/service'
import type { registerForkCloud } from './register'
import { capturePrincipal, type Principal } from './principal'
import { AccountApi, type ClaimProof } from './account-api'
import { deviceSigner } from './device-keys'
import { b64url, computerIdOf, unb64url, verifyEd25519 } from './crypto'
import { Enrollment, type EnrollmentEffect } from './enroll'
import { Connection } from './connection'
import { reconcilePin } from './pins'
import type { BootstrapService } from './bootstrap-service'
import { managedAction } from './managed-connection'
import { ManagedPoll, type PollOutcome } from './managed-poll'

type Factory = NonNullable<ReturnType<typeof registerForkCloud>>
type LocalSource = { personalProfile?(id:string):import('./personal-ssh').PersonalSshProfile|null; captureLocal(profileId: string, current: () => boolean): CapturedSSHLocal | null }
type Directory = { computerId: string; signKey: string; name: string }
type Uncertainty = { kind: 'registration' | 'enrollment'; computerId?: string; operationId?: string; phase?: EnrollmentEffect['phase'] | 'claim' | 'claimRequest'; publicationId?: string; publicationHash?: string }
type ClaimConfirmation = { operationId: string; profileId: string; computerId: string; signKey: string; deviceKey: string }
type ActiveWrite = { principal: Principal; value: Uncertainty }
const identifier = (v: unknown, prefix: string) => typeof v === 'string' && new RegExp(`^${prefix}[A-Za-z0-9_-]{1,60}$`).test(v)
const text = (v: unknown, max: number): v is string => typeof v === 'string' && v.length > 0 && v.length <= max && !/[\x00-\x1f\x7f]/.test(v)
const freshId = () => randomBytes(16).toString('base64url')

/** Rejeita protótipos, getters, símbolos e chaves extras antes de qualquer efeito. */
function record(value: unknown, keys: string[]): Record<string, unknown> | null {
  try {
    if (!value || typeof value !== 'object' || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) return null
    const d = Object.getOwnPropertyDescriptors(value)
    if (Reflect.ownKeys(value).length !== keys.length || keys.some(k => !Object.prototype.hasOwnProperty.call(d, k) || !('value' in d[k]))) return null
    return Object.fromEntries(keys.map(k => [k, d[k].value]))
  } catch { return null }
}
function intentOf(value: unknown): ForkCloudIntent | null {
  try {
    if (!value || typeof value !== 'object' || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) return null
    const k = Object.getOwnPropertyDescriptor(value, 'kind')
    if (!k || !('value' in k)) return null
    switch (k.value) {
      case 'state': case 'discover': case 'disconnect': case 'reconcileRegistration': case 'reconcileClaim':
      case 'startBootstrap':
        return record(value, ['kind']) ? { kind: k.value } : null
      case 'personalLinkStatus': case 'personalLinkDisconnect':
      case 'personalLinkRoot': case 'personalLinkManagement': { const v=record(value,['kind','sourceId']);return v && text(v.sourceId,128) ? {kind:k.value,sourceId:v.sourceId}:null }
      case 'personalLinkConfirm': case 'personalLinkCancel': { const v=record(value,['kind','attemptId']);return v && typeof v.attemptId==='string' && !!unb64url(v.attemptId,16) ? {kind:k.value,attemptId:v.attemptId}:null }
      case 'personalStart': { const v=record(value,['kind','mode','sourceId']);return v && (v.mode==='command' || v.mode==='device') && text(v.sourceId,128) ? {kind:'personalStart',mode:v.mode,sourceId:v.sourceId}:null }
      case 'personalApprove':case 'personalCheck':case 'personalReconcile':case 'personalCancel': {const v=record(value,['kind','attemptId']);return v && typeof v.attemptId==='string' && /^[A-Za-z0-9_-]{22}$/.test(v.attemptId) ? {kind:k.value,attemptId:v.attemptId}:null}
      case 'registerDevice': { const v = record(value, ['kind', 'name']); return v && text(v.name, 100) ? { kind: 'registerDevice', name: v.name } : null }
      case 'startLocal': { const v = record(value, ['kind', 'profileId', 'claim']); return v && text(v.profileId, 128) && typeof v.claim === 'boolean' ? { kind: 'startLocal', profileId: v.profileId, claim: v.claim } : null }
      case 'startRemote': case 'connect': case 'reconcilePin': { const v = record(value, ['kind', 'computerId']); return v && unb64url(v.computerId, 16) && typeof v.computerId === 'string' ? { kind: k.value, computerId: v.computerId } : null }
      case 'check': case 'cancel': { const v = record(value, ['kind', 'attemptId']); return v && typeof v.attemptId === 'string' && !!unb64url(v.attemptId, 16) ? { kind: k.value, attemptId: v.attemptId } : null }
      case 'confirm': { const v = record(value, ['kind', 'attemptId', 'code']); return v && typeof v.attemptId === 'string' && !!unb64url(v.attemptId, 16) && typeof v.code === 'string' && /^[0-9]{6}$/.test(v.code) ? { kind: 'confirm', attemptId: v.attemptId, code: v.code } : null }
      case 'checkBootstrap': case 'proveBootstrap': case 'reconcileBootstrap': { const v = record(value, ['kind', 'bootstrapId']); return v && typeof v.bootstrapId === 'string' && !!unb64url(v.bootstrapId, 16) ? { kind: k.value, bootstrapId: v.bootstrapId } : null }
    }
  } catch { return null }
  return null
}
function localStatus(value: unknown, principal: Principal): ForkCloudKind | 'owned' | 'empty' {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return 'error'
  const v = value as { enabled?: unknown; url?: unknown; owner?: unknown }
  if (v.enabled !== true) return 'hubCloudOff'
  if (v.url !== principal.deployment.origin) return 'deploymentChanged'
  if (v.owner === null) return 'empty'
  if (!v.owner || typeof v.owner !== 'object' || Array.isArray(v.owner) || typeof (v.owner as { userId?: unknown }).userId !== 'string') return 'error'
  return (v.owner as { userId: string }).userId === principal.seal.userId ? 'owned' : 'accountMismatch'
}
function directory(value: unknown): Directory[] | null {
  if (!Array.isArray(value)) return null
  const entries: Directory[] = []
  const seen = new Set<string>()
  for (const raw of value) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) continue
    const row = raw as { computerId?: unknown; signKey?: unknown; name?: unknown }
    const key = unb64url(row.signKey, 32)
    if (typeof row.computerId !== 'string' || !unb64url(row.computerId, 16) || !key || computerIdOf(key) !== row.computerId || !text(row.name, 100) || typeof row.signKey !== 'string') continue
    if (seen.has(row.computerId)) return null
    seen.add(row.computerId); entries.push({ computerId: row.computerId, signKey: row.signKey, name: row.name })
  }
  return entries
}

export class ForkCloudService {
  private generation = 0
  private busy = false
  private readonly claimConfirmations = new Map<string, ClaimConfirmation>()
  private readonly uncertainties = new Map<string, Uncertainty>()
  private attempt: Enrollment | null = null
  private attemptPrincipal: Principal | null = null
  private activeWrite: ActiveWrite | null = null
  private connection: Connection | null = null
  private bootstrap: BootstrapService | null = null
  private personal:import('./personal-service').PersonalService|null=null
  private personalLinkChannel:import('./personal-link-channel').PersonalLinkChannel|null=null
  private personalLinkChannelOwner:{principal:Principal;sourceId:string}|null=null
  private personalLink:import('./personal-link-initial').PersonalLinkInitial|null=null
  private managedBusy = false
  private managedAt = 0
  private managedPaused = false
  private found: Directory[] = []
  private foundEpoch = -1
  private foundBinding: string | null = null
  private state: ForkCloudState
  /** Quando cada quadro chegou do Hub (relógio monotônico deste processo). Só o recebimento: não autoriza entrada. */
  private readonly frameArrivedAt = new WeakMap<BrowserFrame, number>()
  /** Quadros realmente despachados ao app pela assinatura atual, por conexão, com a hora da CHEGADA (só os que ainda estão em 6 s; no máximo 2 conexões). */
  private readonly frameArrivals = new Map<string, Map<string, number>>()
  private readonly epochChanged = () => { this.managedPaused=false; this.invalidate(); this.set('signedOut') }

  constructor(private readonly account: AccountService, private readonly factory: Factory, private readonly local: LocalSource, private readonly onChange: (value: ForkCloudResult) => void = () => {}) {
    this.state = { kind: 'signedOut', epoch: account.sessionEpoch, attemptId: '' }
    account.on('epoch', this.epochChanged)
  }
  dispose() { this.invalidate(); this.account.off('epoch', this.epochChanged) }
  private invalidate() {
    if (this.attempt && this.attemptPrincipal) this.rememberAttempt(this.attemptPrincipal, this.attempt)
    if (this.activeWrite) this.remember(this.activeWrite.principal, this.activeWrite.value)
    this.generation++; this.attempt?.cancel(); this.attempt = null; this.connection?.stop(); this.connection = null
    this.personal?.stop(); this.personal=null
    this.personalLink?.stop(); this.personalLink=null
    this.personalLinkChannel?.dispose(); this.personalLinkChannel=null; this.personalLinkChannelOwner=null
    this.bootstrap?.stop(); this.bootstrap = null
    this.attemptPrincipal = null
    this.found = []; this.foundEpoch = -1; this.foundBinding = null
  }
  private markUncertain(p: Principal, kind: 'registration' | 'enrollment' = 'enrollment', computerId?: string) {
    this.remember(p, { kind, computerId })
  }
  private remember(p: Principal, value: Uncertainty) {
    const prior = this.uncertainties.get(p.namespace)
    if (!prior || value.operationId && prior.operationId === value.operationId) this.uncertainties.set(p.namespace, { ...value })
  }
  private rememberAttempt(p: Principal, attempt: Enrollment) {
    const effect = attempt.pendingEffect
    if (effect) this.remember(p, { kind: 'enrollment', computerId: effect.computerId, operationId: effect.attemptId, phase: effect.phase, ...(effect.publicationHash ? { publicationId: effect.attemptId, publicationHash: effect.publicationHash } : {}) })
    else if (this.uncertainties.get(p.namespace)?.operationId === attempt.id) this.uncertainties.delete(p.namespace)
  }
  private beginWrite(p: Principal, value: Uncertainty): ActiveWrite {
    const write = { principal: p, value }
    this.activeWrite = write
    return write
  }
  private finishWrite(write: ActiveWrite, uncertain: boolean) {
    if (uncertain) this.remember(write.principal, write.value)
    else if (write.value.operationId && this.uncertainties.get(write.principal.namespace)?.operationId === write.value.operationId) this.uncertainties.delete(write.principal.namespace)
    if (!uncertain && write.value.phase === 'claim' && this.claimConfirmations.get(write.principal.namespace)?.operationId === write.value.operationId) this.claimConfirmations.delete(write.principal.namespace)
    if (this.activeWrite === write) this.activeWrite = null
  }
  private emit() { this.onChange(this.output()) }
  private set(kind: ForkCloudKind, attemptId = '', computerId?: string) {
    this.state = { kind, epoch: this.account.sessionEpoch, attemptId, ...(computerId ? { computerId } : {}) }
    this.emit()
  }
  private output(): ForkCloudResult {
    const attachment = this.connection?.getAttachment()
    const captured = capturePrincipal(this.account)
    const principal = captured?.current() ? captured : null
    const registration = principal ? this.account.verifiedDeviceBinding(principal.seal) ? 'registered' : 'unregistered' : 'unavailable'
    const recovery = principal ? this.uncertainties.get(principal.namespace) : undefined
    const state = recovery ? { ...(this.attempt && this.attempt.id === recovery.operationId ? this.state : {}), kind: 'unknown' as const, epoch: this.account.sessionEpoch, attemptId: this.attempt?.id ?? '', ...(recovery.computerId ? { computerId: recovery.computerId } : {}) } : { ...this.state }
    const bootstrap = this.bootstrap?.snapshot()
    const ownStatus=this.personalLinkChannel?.snapshot()
    return { ...(ownStatus ? {personalLinkOwn:ownStatus}:{}), registration, recovery: recovery?.kind ?? null, recoveryPhase: recovery?.phase ?? null, state, computers: this.foundEpoch === this.account.sessionEpoch ? this.found.map(({ computerId, name }) => ({ computerId, name })) : [], ...(attachment ? { attachment } : {}), ...(bootstrap ? { bootstrap } : {}), ...(this.personal?.snapshot() ? {personalAttempt:this.personal.snapshot()!}:{}), ...(this.personalLink?.snapshot() ? {personalLink:this.personalLink.snapshot()!}:{}), ...(principal?.personalBootstrapStatus ? { personalBootstrap: principal.personalBootstrapStatus } : {}) }
  }
  private reject(): ForkCloudResult { return { ...this.output(), state: { kind: 'notSent', epoch: this.account.sessionEpoch, attemptId: '' } } }
  private async restoreBootstrap(p: Principal) {
    if (this.bootstrap?.current()) return
    this.bootstrap?.stop(); this.bootstrap = null
    if (!p.bootstrapAvailable) return
    const { BootstrapService } = await import('./bootstrap-service')
    if (!p.current()) return
    const service = await BootstrapService.restore(this.account, p)
    if (!p.current()) { service?.stop(); return }
    this.bootstrap = service
  }
  /** 'active' só quando o fluxo gerenciado deu um passo (ritmo curto em managed-poll). */
  private async advanceManaged(p: Principal): Promise<PollOutcome> {
    if (this.managedBusy || Date.now()-this.managedAt<2000) return 'active'
    if (this.personal || this.managedPaused || this.busy || this.attempt || this.connection) return 'idle'
    this.managedBusy=true; this.managedAt=Date.now()
    try {
      const api=new AccountApi(p)
      const health=await api.execute({kind:'health'}); if (!p.current()) return 'idle'; if (health.kind!=='ok') return 'error'
      const offer=await api.execute({kind:'managedOffer'}); if (!p.current()) return 'idle'; if (offer.kind!=='ok') return 'error'
      if (offer.value.available!==true || typeof offer.value.computerId!=='string' || !unb64url(offer.value.computerId,16)) return 'idle'
      const action=managedAction(this.output(),offer.value.computerId,offer.value.canStart===true)
      if (!action || !p.current() || this.managedPaused) return 'idle'
      await this.invoke(action,()=>p.current())
      return 'active'
    } catch { /* Keep the existing durable unknown; next tick can read again. */ return 'error' }
    finally { this.managedBusy=false }
  }
  /** Leitura de fundo (managed-poll); a mesma do pedido de estado do renderer. */
  async pollManaged(callerCurrent: () => boolean = () => true): Promise<PollOutcome> {
    const p = this.captured(callerCurrent)
    if(p?.personalBootstrapStatus?.reason==='helperRequired' && !this.busy && !this.personal?.current()){
      this.busy=true;try{this.personal?.stop();const {PersonalService}=await import('./personal-service');if(p.current())this.personal=await PersonalService.restore(this.account,p,id=>this.local.personalProfile?.(id)??null)}catch{if(p.current())this.set('storageError')}finally{this.busy=false}
    }
    if (p?.bootstrapAvailable && !this.personal && !this.busy && !this.bootstrap?.current()) {
      this.busy = true
      try { await this.restoreBootstrap(p) } catch { if (p.current()) this.set('storageError') }
      finally { this.busy = false }
    }
    return p?.bootstrapAvailable && p.current() ? this.advanceManaged(p) : 'idle'
  }
  private captured(callerCurrent: () => boolean = () => true): Principal | null {
    const raw = capturePrincipal(this.account)
    if (!raw) return null
    const generation = this.generation
    const initialBinding = this.account.verifiedDeviceBinding(raw.seal)
    const fingerprint = initialBinding ? JSON.stringify(initialBinding) : null
    const current = () => {
      if (generation !== this.generation || !raw.current() || !callerCurrent()) return false
      return fingerprint === null || JSON.stringify(this.account.verifiedDeviceBinding(raw.seal)) === fingerprint
    }
    return Object.freeze({ ...raw, current, async token() { if (!current()) throw new Error('Superseded'); const token = await raw.token(); if (!current()) throw new Error('Superseded'); return token } })
  }
  private async bound(p: Principal): Promise<boolean> {
    const binding = this.account.verifiedDeviceBinding(p.seal)
    if (!binding || !p.current()) return false
    try {
      const signer = await deviceSigner(p, 'read')
      const now = this.account.verifiedDeviceBinding(p.seal)
      return p.current() && !!now && now.deviceId === binding.deviceId && now.deviceKey === binding.deviceKey && b64url(signer.publicKey) === binding.deviceKey
    } catch { return false }
  }
  private takeState(p: Principal, attempt: Enrollment, value: ForkCloudState) {
    if (!p.current() || this.attempt !== attempt || value.epoch !== p.seal.epoch || value.attemptId !== attempt.id) return
    const safe: ForkCloudState = { kind: value.kind, epoch: value.epoch, attemptId: value.attemptId,
      ...(value.computerId ? { computerId: value.computerId } : {}), ...(value.code ? { code: value.code } : {}), ...(value.deadline ? { deadline: value.deadline } : {}), ...(value.readUnavailable ? { readUnavailable: true as const } : {}) }
    this.rememberAttempt(p, attempt)
    this.state = safe
    if (attempt.ended || safe.kind === 'linked') { this.attempt = null; this.attemptPrincipal = null }
    this.emit()
  }
  private async list(p: Principal, api: AccountApi): Promise<Directory[] | null> {
    const result = await api.execute({ kind: 'listComputers' })
    return p.current() && result.kind === 'ok' ? directory(result.value.computers) : null
  }
  private async createAttempt(p: Principal, target: Directory, local?: CapturedSSHLocal) {
    if (!p.current() || (local && !local.current())) return
    const attempt = new Enrollment(p, target.computerId, target.signKey, local)
    this.attempt = attempt
    this.attemptPrincipal = p
    const result = await attempt.start()
    this.rememberAttempt(p, attempt)
    if (!p.current() || this.attempt !== attempt) return
    this.takeState(p, attempt, result)
  }
  private async startLocal(p: Principal, profileId: string, claim: boolean) {
    const op = freshId()
    const local = this.local.captureLocal(profileId, () => p.current())
    if (!local || !p.current()) { this.set('notSent', op); return }
    const current = () => p.current() && local.current()
    const scoped: Principal = Object.freeze({ ...p, current, async token() { if (!current()) throw new Error('Superseded'); const token = await p.token(); if (!current()) throw new Error('Superseded'); return token } })
    this.set('linking', op, local.computerId)
    const api = new AccountApi(scoped)
    let write: ActiveWrite | null = null
    const finish = (uncertain: boolean) => { if (write) { this.finishWrite(write, uncertain); write = null } }
    try {
      if (!(await this.bound(scoped)) || !current()) { if (current()) this.set('needsLink', op, local.computerId); return }
      const hello = await local.execute({ kind: 'hello' })
      if (!current()) return
      if (!hello || typeof hello !== 'object' || (hello as { computerId?: unknown }).computerId !== local.computerId || (hello as { signKey?: unknown }).signKey !== local.signKey) { this.set('hubUnverified', op, local.computerId); return }
      const health = await api.execute({ kind: 'health' })
      if (!current()) return
      if (health.kind !== 'ok') { this.set('deploymentChanged', op, local.computerId); return }
      const status = localStatus(await local.execute({ kind: 'cloudStatus' }), p)
      if (!current()) return
      if (status !== 'owned' && status !== 'empty') { this.set(status, op, local.computerId); return }
      if (status === 'empty') {
        if (!claim) { this.set('needsLink', op, local.computerId); return }
        write = this.beginWrite(p, { kind: 'enrollment', computerId: local.computerId, operationId: op, phase: 'claimRequest' })
        const challenge = await api.execute({ kind: 'createClaim' })
        if (challenge.kind !== 'ok') { finish(challenge.kind === 'unknown'); if (current()) this.set(challenge.kind === 'unknown' ? 'unknown' : challenge.kind === 'notSent' ? 'notSent' : 'error', op, local.computerId); return }
        if (!current()) { finish(true); return }
        const { claimId, nonce, expiresAt } = challenge.value
        if (!identifier(claimId, 'clm_') || typeof claimId !== 'string' || typeof nonce !== 'string' || !unb64url(nonce, 16) || typeof expiresAt !== 'number' || !Number.isSafeInteger(expiresAt) || expiresAt <= Date.now()) { finish(true); this.set('unknown', op, local.computerId); return }
        finish(false)
        const signed = await local.execute({ kind: 'claimSign', claimId, nonce, userId: p.seal.userId })
        if (!current()) return
        if (!signed || typeof signed !== 'object' || Array.isArray(signed)) { this.set('hubUnverified', op, local.computerId); return }
        const raw = signed as Record<string, unknown>
        const host = unb64url(local.signKey, 32), sig = unb64url(raw.sig, 64)
        if (raw.computerId !== local.computerId || raw.signKey !== local.signKey || !host || !sig || !unb64url(raw.boxKey, 32) || typeof raw.boxKey !== 'string'
          || !text(raw.name, 100) || !['macos', 'linux', 'windows'].includes(String(raw.platform)) || !text(raw.version, 32)
          || !(raw.device == null || text(raw.device, 32)) || Date.now() >= expiresAt) { this.set('hubUnverified', op, local.computerId); return }
        const canonical = ['codync/claim/v1', claimId, nonce, p.seal.userId, local.computerId, raw.boxKey].join('\n')
        if (!verifyEd25519(host, sig, new TextEncoder().encode(canonical)) || !current() || Date.now() >= expiresAt) { this.set('hubUnverified', op, local.computerId); return }
        const proof: ClaimProof = { computerId: local.computerId, signKey: local.signKey, boxKey: raw.boxKey, sig: raw.sig as string,
          name: raw.name, platform: raw.platform as ClaimProof['platform'], device: raw.device as string | null | undefined, version: raw.version }
        const beforeClaim = localStatus(await local.execute({ kind: 'cloudStatus' }), p)
        if (!current()) return
        if (beforeClaim !== 'empty' || Date.now() >= expiresAt) { this.set(beforeClaim === 'owned' ? 'needsLink' : beforeClaim === 'empty' ? 'notSent' : beforeClaim, op, local.computerId); return }
        if (!(await this.bound(scoped)) || !current() || Date.now() >= expiresAt) { if (current()) this.set('notSent', op, local.computerId); return }
        const deviceKey = this.account.verifiedDeviceBinding(p.seal)?.deviceKey
        if (!deviceKey || !current()) return
        this.claimConfirmations.set(p.namespace, { operationId: op, profileId, computerId: local.computerId, signKey: local.signKey, deviceKey })
        write = this.beginWrite(p, { kind: 'enrollment', computerId: local.computerId, operationId: op, phase: 'claim' })
        const completed = await api.execute({ kind: 'completeClaim', claimId, proof })
        if (completed.kind !== 'ok') { finish(completed.kind === 'unknown'); if (current()) this.set(completed.kind === 'unknown' ? 'unknown' : completed.kind === 'notSent' ? 'notSent' : 'error', op, local.computerId); return }
        if (!current()) { finish(true); return }
        const verified = localStatus(await local.execute({ kind: 'cloudStatus' }), p)
        if (!current()) { finish(true); return }
        if (verified !== 'owned') { finish(true); this.set('unknown', op, local.computerId); return }
        finish(false)
      }
      const entries = await this.list(p, api)
      if (!current()) return
      const selected = entries?.filter(row => row.computerId === local.computerId && row.signKey === local.signKey) ?? []
      if (selected.length !== 1) { this.set('hubUnverified', op, local.computerId); return }
      if (!(await this.bound(scoped)) || !current()) return
      await this.createAttempt(scoped, selected[0], local)
    } catch (error) {
      const uncertain = write !== null
      finish(uncertain)
      if (current()) this.set(uncertain ? 'unknown' : local.wasNotSent(error) ? 'notSent' : 'error', op, local.computerId)
    }
  }

  /** Read-only confirmation after the existing claim; never signs or sends it again. */
  private async reconcileClaim(p: Principal) {
    const pending = this.claimConfirmations.get(p.namespace)
    const uncertainty = this.uncertainties.get(p.namespace)
    if (!pending || uncertainty?.phase !== 'claim' || uncertainty.operationId !== pending.operationId) return
    const local = this.local.captureLocal(pending.profileId, () => p.current())
    if (!local || local.computerId !== pending.computerId || local.signKey !== pending.signKey || !local.current() || !p.current()) return
    const current = () => p.current() && local.current()
    const scoped: Principal = Object.freeze({ ...p, current, async token() { if (!current()) throw new Error('Superseded'); const token = await p.token(); if (!current()) throw new Error('Superseded'); return token } })
    try {
      if (this.account.verifiedDeviceBinding(p.seal)?.deviceKey !== pending.deviceKey || !(await this.bound(scoped)) || !current()) return
      const hello = await local.execute({ kind: 'hello' })
      if (!current() || !hello || typeof hello !== 'object' || (hello as { computerId?: unknown }).computerId !== pending.computerId || (hello as { signKey?: unknown }).signKey !== pending.signKey) return
      const api = new AccountApi(scoped)
      const health = await api.execute({ kind: 'health' })
      if (!current() || health.kind !== 'ok') return
      const owner = localStatus(await local.execute({ kind: 'cloudStatus' }), scoped)
      if (!current() || owner !== 'owned') return
      const rows = await this.list(scoped, api)
      if (!current() || rows?.filter(row => row.computerId === pending.computerId && row.signKey === pending.signKey).length !== 1) return
      if (this.account.verifiedDeviceBinding(p.seal)?.deviceKey !== pending.deviceKey || !(await this.bound(scoped)) || !current()) return
      if (this.claimConfirmations.get(p.namespace) !== pending || this.uncertainties.get(p.namespace)?.operationId !== pending.operationId) return
      this.claimConfirmations.delete(p.namespace); this.uncertainties.delete(p.namespace)
      // Confirmation is read-only. Linking remains a separate explicit click.
      this.set('needsLink', '', pending.computerId)
    } catch { /* Retain this claim and its fence for another explicit read. */ }
  }

  dataConnection(id: unknown): Connection | null {
    if (typeof id !== 'string' || !unb64url(id, 16)) return null
    const connection = this.connection
    return connection && connection.getAttachment()?.connectionId === id ? connection : null
  }
  async call(id: unknown, method: unknown, body: unknown, timeout: unknown): Promise<ForkCloudCallResult> {
    if (typeof method !== 'string' || !/^[A-Za-z][A-Za-z0-9]{0,63}$/.test(method) || typeof timeout !== 'number' || !Number.isInteger(timeout) || timeout < 1000 || timeout > 120_000) return { kind: 'notSent', reason: 'notReady' }
    try { const serialized = JSON.stringify(body); if (serialized === undefined) return { kind: 'notSent', reason: 'notReady' }; if (Buffer.byteLength(serialized, 'utf8') > 1024 * 1024) return { kind: 'notSent', reason: 'tooLarge' } } catch { return { kind: 'notSent', reason: 'notReady' } }
    const connection = this.dataConnection(id)
    if (!connection) return { kind: 'notSent', reason: 'notReady' }
    if (method === 'browserStreamInput' && !this.inputFrameFresh(id as string, body)) return { kind: 'notSent', reason: 'notReady' }
    try { return await connection.call(method, body, timeout) } catch { return { kind: 'unknown', reason: 'closed' } }
  }
  subscribe(id: unknown, body: unknown, onEvent: (value: unknown) => void, onEnd: (error: { status: number; message: string } | null) => void): Subscription | null {
    const request = cloudSubscription(body)
    if (!request) return null
    const connection = this.dataConnection(id)
    if (!connection) return null
    return connection.subscribe(request, onEvent, onEnd)
  }
  /** Quadros do navegador do bot (B2): só com a conexão atual e a capacidade aceita pelo Hub. */
  subscribeFrames(id: unknown, sink: FrameSink): Subscription | null {
    const connection = this.dataConnection(id)
    if (!connection?.framesNegotiated || typeof id !== 'string') return null
    const subscription = connection.subscribeFrames({
      onFrame: (frame) => { this.frameArrivedAt.set(frame, performance.now()); sink.onFrame(frame) },
      onEnd: (error) => { this.frameArrivals.delete(id); sink.onEnd(error) },
    })
    return subscription && { ...subscription, cancel: () => { this.frameArrivals.delete(id); subscription.cancel() } }
  }
  /** Identidade estrutural do quadro: uma lista JSON, sem separador que os identificadores possam conter. */
  private frameKey(frame: { session: string; target: string; targetGeneration: number; sequence: number }) { return JSON.stringify([frame.session, frame.target, frame.targetGeneration, frame.sequence]) }
  /**
   * Chamado só depois de o quadro ter saído pelo IPC para a assinatura atual: um quadro recebido que a fila descartou ou
   * ainda retém não chega aqui e nunca autoriza entrada. A idade segue contando desde a chegada, não desde o despacho.
   */
  frameDispatched(connectionId: string, frame: BrowserFrame) {
    const arrival = this.frameArrivedAt.get(frame)
    if (arrival === undefined) return
    let arrivals = this.frameArrivals.get(connectionId)
    if (!arrivals) {
      if (this.frameArrivals.size >= 2) this.frameArrivals.delete(this.frameArrivals.keys().next().value as string)
      this.frameArrivals.set(connectionId, arrivals = new Map())
    }
    arrivals.set(this.frameKey(frame), arrival)
    // Despacho em ordem de chegada: o que passou do prazo está no começo e nunca mais autoriza nada.
    for (const [key, at] of arrivals) { if (performance.now() - at <= STALE_AFTER_MS) break; arrivals.delete(key) }
  }
  /**
   * O processo principal é a autoridade final do prazo do quadro: o clique só sai ao Hub se o quadro que ele cita (sessão,
   * aba, geração e sequência) foi mesmo despachado ao app por esta conexão e chegou há no máximo 6 s, contados aqui, neste relógio,
   * imediatamente antes do envio. A idade calculada pelo renderer não autoriza nada (ela não vê o IPC nos dois sentidos).
   */
  private inputFrameFresh(connectionId: string, body: unknown): boolean {
    if (!body || typeof body !== 'object' || Array.isArray(body)) return false
    const { session, target, targetGeneration, frameSequence } = body as Record<string, unknown>
    if (typeof session !== 'string' || typeof target !== 'string' || typeof targetGeneration !== 'number' || typeof frameSequence !== 'number') return false
    const arrival = this.frameArrivals.get(connectionId)?.get(this.frameKey({ session, target, targetGeneration, sequence: frameSequence }))
    return arrival !== undefined && performance.now() - arrival <= STALE_AFTER_MS
  }

  async invoke(input: unknown, callerCurrent: () => boolean = () => true): Promise<ForkCloudResult> {
    const intent = intentOf(input)
    if (!intent) return this.reject()
    if (intent.kind==='personalLinkDisconnect') {
      const p=this.captured(callerCurrent);if(!p?.current())return this.reject()
      const channel=this.personalLinkChannel, owner=this.personalLinkChannelOwner
      if(!channel)return this.output()
      const original=owner?.principal
      if(!original?.current() || owner?.sourceId!==intent.sourceId || original.namespace!==p.namespace
        || original.seal.userId!==p.seal.userId || original.seal.sessionId!==p.seal.sessionId || original.seal.epoch!==p.seal.epoch)return this.reject()
      // Fechamento local cancela a query existente; só ela libera busy no próprio finally.
      channel.stop()
      return this.output()
    }
    if (intent.kind==='personalLinkStatus') {
      if(this.busy || this.attempt || this.connection || this.personal || this.bootstrap?.current())return this.reject()
      const p=this.captured(callerCurrent);if(!p)return this.reject();this.busy=true
      try {
        const {PersonalLinkChannel}=await import('./personal-link-channel')
        if(!p.current())return this.reject()
        const channel=this.personalLinkChannel??=new PersonalLinkChannel()
        this.personalLinkChannelOwner={principal:p,sourceId:intent.sourceId}
        const personalLinkOwn=await channel.query(p,intent.sourceId,this.local.personalProfile?.(intent.sourceId)??null)
        if(!p.current())return this.reject()
        return {...this.output(),personalLinkOwn}
      } catch {return this.reject()} finally {this.busy=false}
    }
    // D430 PR-A: intenções fechadas; o Main monta e assina só os objetos do procedimento ancorado.
    if (intent.kind==='personalLinkRoot' || intent.kind==='personalLinkManagement' || intent.kind==='personalLinkConfirm' || intent.kind==='personalLinkCancel') {
      if(this.busy || this.attempt || this.connection || this.personal || this.bootstrap?.current())return this.reject()
      const p=this.captured(callerCurrent);if(!p)return this.reject();this.busy=true
      try {
        const {PersonalLinkInitial}=await import('./personal-link-initial')
        if(!p.current())return this.reject()
        const link=this.personalLink??=new PersonalLinkInitial()
        if(intent.kind==='personalLinkRoot')await link.startRoot(p,intent.sourceId,this.local.personalProfile?.(intent.sourceId)??null)
        else if(intent.kind==='personalLinkManagement')await link.startManagement(p,intent.sourceId,this.local.personalProfile?.(intent.sourceId)??null)
        else if(intent.kind==='personalLinkConfirm')await link.confirm(p,intent.attemptId)
        else if(intent.kind==='personalLinkCancel')link.cancel(intent.attemptId)
        return this.output()
      } catch { return this.reject() } finally {this.busy=false}
    }
    if (intent.kind==='personalStart' || intent.kind==='personalApprove' || intent.kind==='personalCheck' || intent.kind==='personalReconcile' || intent.kind==='personalCancel') {
      if(this.busy || this.attempt || this.connection || this.bootstrap?.current())return this.reject()
      const p=this.captured(callerCurrent);if(!p)return this.reject();this.busy=true
      try {
        if(intent.kind==='personalStart') {
          if(this.personal)return this.reject()
          const {PersonalService}=await import('./personal-service')
          if(!p.current())return this.reject()
          this.personal=await PersonalService.create(this.account,p,id=>this.local.personalProfile?.(id)??null)
          await this.personal.start(intent.mode,intent.sourceId)
        } else {
          if(!this.personal || !this.personal.current()){this.personal?.stop();const {PersonalService}=await import('./personal-service');this.personal=await PersonalService.restore(this.account,p,id=>this.local.personalProfile?.(id)??null)}
          if(!this.personal)return this.reject()
          if(intent.kind==='personalApprove')await this.personal.approve(intent.attemptId)
          else if(intent.kind==='personalReconcile')await this.personal.reconcile(intent.attemptId)
          else if(intent.kind==='personalCancel')await this.personal.cancel(intent.attemptId)
          else await this.personal.check(intent.attemptId)
        }
        return this.output()
      } catch { return this.reject() } finally {this.busy=false}
    }
    if (intent.kind === 'state') { await this.pollManaged(callerCurrent); return this.output() }
    if (intent.kind === 'cancel') {
      if (!this.attempt || this.attempt.id !== intent.attemptId) return this.reject()
      this.invalidate(); this.set('notSent'); return this.output()
    }
    if (intent.kind === 'disconnect') { this.managedPaused=true; this.invalidate(); this.set('notSent'); return this.output() }
    if(this.personal)return this.reject()
    // A check may reconcile this exact publication; Enrollment never replays a lost approval.
    if (this.busy) return this.reject()
    const p = this.captured(callerCurrent)
    if (!p) { this.set('signedOut'); return this.output() }
    if (!p.current()) return this.reject()
    const uncertainty = this.uncertainties.get(p.namespace)
    if (uncertainty && !(intent.kind === 'check' && this.attempt?.id === intent.attemptId) && intent.kind !== 'reconcilePin' && !(intent.kind === 'reconcileClaim' && uncertainty.phase === 'claim') && !(intent.kind === 'reconcileRegistration' && uncertainty.kind === 'registration')) return this.reject()
    this.busy = true
    try {
      // Enforce a retained unknown even if the renderer did not request state
      // first after restart. Restoring this journal never starts network work.
      if (p.bootstrapAvailable) {
        await this.restoreBootstrap(p)
        if (!p.current()) return this.output()
      }
      if (intent.kind === 'startBootstrap' || intent.kind === 'checkBootstrap' || intent.kind === 'proveBootstrap' || intent.kind === 'reconcileBootstrap') {
        if (this.attempt || this.connection || !(await this.bound(p)) || !p.current()) return this.reject()
        if (!this.bootstrap || !p.current()) return this.reject()
        await this.bootstrap.invoke(intent.kind, 'bootstrapId' in intent ? intent.bootstrapId : undefined)
        if (!p.current()) return this.output()
        const state = this.bootstrap.snapshot()
        if (state?.phase === 'complete') this.set('linked', state.bootstrapId, state.computerId)
        else this.set('needsLink', state?.bootstrapId ?? '', state?.computerId)
      } else if (this.bootstrap?.blocksOtherOperations) {
        return this.reject()
      } else if (intent.kind === 'registerDevice') {
        if (this.attempt || this.connection || this.account.verifiedDeviceBinding(p.seal)) return this.reject()
        const platform = process.platform === 'darwin' ? 'macos' : process.platform === 'linux' ? 'linux' : process.platform === 'win32' ? 'windows' : null
        if (!platform) { this.set('notSent'); return this.output() }
        const write = this.beginWrite(p, { kind: 'registration', operationId: freshId() })
        const result = await this.factory.registerDevice(intent.name, platform)
        this.finishWrite(write, result.kind === 'unknown')
        if (!p.current()) return this.output()
        if (result.kind === 'registered' && p.bootstrapAvailable) await this.restoreBootstrap(p)
        if (!p.current()) return this.output()
        this.set(result.kind === 'registered' ? 'needsLink' : result.kind === 'unavailable' ? 'error' : result.kind)
      } else if (intent.kind === 'reconcileRegistration') {
        if (this.attempt || this.connection) return this.reject()
        const result = await this.factory.reconcileRegistration()
        if (!p.current()) return this.output()
        if (result.kind === 'registered') {
          this.uncertainties.delete(p.namespace)
          if (p.bootstrapAvailable) await this.restoreBootstrap(p)
          if (p.current()) this.set('needsLink')
        }
        else if (result.kind === 'notSent') return this.reject()
        else { this.markUncertain(p, 'registration'); this.set('unknown') }
      } else if (intent.kind === 'reconcileClaim') {
        if (this.attempt || this.connection || uncertainty?.phase !== 'claim') return this.reject()
        await this.reconcileClaim(p)
      } else if (intent.kind === 'discover') {
        if (this.attempt || this.connection) return this.reject()
        this.found = []; this.foundEpoch = -1; this.foundBinding = null
        if (!(await this.bound(p)) || !p.current()) { if (p.current()) this.set('needsLink'); return this.output() }
        this.set('discovering')
        const api = new AccountApi(p), health = await api.execute({ kind: 'health' })
        if (!p.current()) return this.output()
        if (health.kind !== 'ok') { this.set('deploymentChanged'); return this.output() }
        const entries = await this.list(p, api)
        if (!p.current()) return this.output()
        if (!entries) { this.set('error'); return this.output() }
        this.found = entries; this.foundEpoch = p.seal.epoch; this.foundBinding = JSON.stringify(this.account.verifiedDeviceBinding(p.seal)); this.set(entries.length ? 'needsLink' : 'noHub')
      } else if (intent.kind === 'startLocal' || intent.kind === 'startRemote') {
        if (this.attempt || this.connection) return this.reject()
        if (intent.kind === 'startLocal') await this.startLocal(p, intent.profileId, intent.claim)
        else {
          if (!(await this.bound(p)) || !p.current()) { if (p.current()) this.set('needsLink'); return this.output() }
          const entries = this.foundEpoch === p.seal.epoch && this.foundBinding === JSON.stringify(this.account.verifiedDeviceBinding(p.seal)) ? this.found.filter(row => row.computerId === intent.computerId) : []
          if (entries.length !== 1) { this.set('needsLink'); return this.output() }
          await this.createAttempt(p, entries[0])
        }
      } else if (intent.kind === 'check' || intent.kind === 'confirm') {
        const attempt = this.attempt
        if (!attempt || attempt.id !== intent.attemptId || !(await this.bound(p)) || !p.current() || this.attempt !== attempt) return this.reject()
        const result = intent.kind === 'check' ? await attempt.check() : await attempt.confirmCode(intent.code)
        this.rememberAttempt(p, attempt)
        if (!p.current() || this.attempt !== attempt) return this.output()
        this.takeState(p, attempt, result)
      } else if (intent.kind === 'reconcilePin') {
        if (this.connection || uncertainty?.kind === 'registration' || uncertainty && uncertainty.phase !== 'publication' || uncertainty?.computerId && uncertainty.computerId !== intent.computerId || !(await this.bound(p)) || !p.current()) return this.reject()
        if (this.attempt) {
          const attempt = this.attempt
          if (attempt.computerId !== intent.computerId || attempt.pendingEffect?.phase !== 'publication') return this.reject()
          const result = await attempt.check()
          this.rememberAttempt(p, attempt)
          if (p.current() && this.attempt === attempt) this.takeState(p, attempt, result)
          return this.output()
        }
        const write = this.beginWrite(p, { kind: 'enrollment', computerId: intent.computerId, operationId: uncertainty?.operationId ?? freshId(), phase: 'publication', publicationId: uncertainty?.publicationId, publicationHash: uncertainty?.publicationHash })
        const expected = write.value.publicationId && write.value.publicationHash ? { attemptId: write.value.publicationId, sha256: write.value.publicationHash } : undefined
        const pin = await reconcilePin(p, intent.computerId, expected, (identity) => {
          write.value.publicationId = identity.attemptId; write.value.publicationHash = identity.sha256
        })
        if (!p.current()) { this.finishWrite(write, true); return this.output() }
        if (!pin || uncertainty && !pin.reconciled) { this.finishWrite(write, !!uncertainty); return this.reject() }
        this.finishWrite(write, false)
        this.uncertainties.delete(p.namespace)
        this.set('linked', '', intent.computerId)
      } else if (intent.kind === 'connect') {
        if (this.connection || this.attempt && this.state.kind !== 'linked') return this.reject()
        if (!(await this.bound(p)) || !p.current()) return this.reject()
        const connection = new Connection(this.account, p, intent.computerId, (state) => {
          if (p.current() && this.connection === connection && state.epoch === p.seal.epoch) {
            this.state = { ...state }
            if (!['connecting', 'ready', 'reconnecting', 'hubOffline'].includes(state.kind)) { this.connection = null; connection.stop() }
            this.emit()
          }
        })
        this.connection = connection; this.set('connecting', '', intent.computerId)
        const started = await connection.start()
        if (!p.current() || this.connection !== connection) return this.output()
        if (!started) {
          this.connection = null
          if (connection.hasPendingPin) this.remember(p, { kind: 'enrollment', phase: 'publication', computerId: intent.computerId, operationId: freshId() })
          this.set(connection.hasPendingPin ? 'unknown' : 'error', '', intent.computerId)
        }
      }
    } catch {
      if (this.attempt && this.attemptPrincipal) this.rememberAttempt(this.attemptPrincipal, this.attempt)
      if (this.activeWrite?.principal === p) this.finishWrite(this.activeWrite, true)
      if (p.current()) this.set(this.uncertainties.has(p.namespace) ? 'unknown' : 'error', this.state.attemptId, this.state.computerId)
    } finally { this.busy = false }
    return this.output()
  }
}

function trusted(event: IpcMainInvokeEvent | IpcMainEvent, windows: BrowserWindow[], urls: string[]): boolean {
  const frame = event.senderFrame
  const allowed = urls.filter(url => { try { const u = new URL(url); return u.protocol === 'file:' && u.host === '' || ['http:', 'https:'].includes(u.protocol) && ['localhost', '127.0.0.1', '[::1]'].includes(u.hostname) } catch { return false } })
  return !!frame && windows.some(w => !w.isDestroyed() && w.webContents === event.sender && w.webContents.mainFrame === frame)
    && allowed.some(url => frame.url.split('#')[0] === url.split('#')[0])
}
/** Ocioso por 5 min: sem leitura de rede; a volta é percebida por checagem local. */
const AWAY_IDLE_S = 300
export function registerForkCloudService(account: AccountService, factory: Factory, local: LocalSource, windows: () => BrowserWindow[], urls: string[], visible: () => boolean = () => true) {
  type Entry = { event: IpcMainEvent; connectionId: string; sub: Subscription | null; cleanup: () => void }
  const subscriptions = new Map<string, Entry>()
  const key = (event: IpcMainEvent, id: string) => `${event.sender.id}:${id}`
  const live = (event: IpcMainEvent | IpcMainInvokeEvent) => trusted(event, windows(), urls) && !event.sender.isDestroyed()
  const end = (id: string, error: { status: number; message: string } | null, notify: boolean) => {
    const entry = subscriptions.get(id)
    if (!entry) return
    subscriptions.delete(id); entry.cleanup(); try { entry.sub?.cancel() } catch {}
    if (notify && live(entry.event)) entry.event.sender.send('forkCloud:events:end', id.slice(id.indexOf(':') + 1), error)
  }
  type FrameEntry = { event: IpcMainEvent; connectionId: string; sub: Subscription | null; gate: ReturnType<typeof createFrameGate<BrowserFrame>>; cleanup: () => void }
  const frames = new Map<string, FrameEntry>()
  const transportError = (status: number, message: string) => ({ status, message })
  const endFrames = (id: string, error: { status: number; message: string } | null, notify: boolean) => {
    const entry = frames.get(id)
    if (!entry) return
    frames.delete(id); entry.cleanup(); entry.gate.clear(); try { entry.sub?.cancel() } catch {}
    if (notify && live(entry.event)) entry.event.sender.send('forkCloud:frames:end', id.slice(id.indexOf(':') + 1), error)
  }
  let poll: ManagedPoll | null = null, attached = false
  const service = new ForkCloudService(account, factory, local, value => {
    // A dropped connection is retried by the next read, not after a long backoff.
    if (attached && !value.attachment) poll?.now()
    attached = !!value.attachment
    for (const [id, entry] of subscriptions) if (entry.connectionId !== value.attachment?.connectionId || !live(entry.event)) end(id, { status: 0, message: 'Conexão encerrada.' }, true)
    for (const [id, entry] of frames) if (entry.connectionId !== value.attachment?.connectionId || !live(entry.event)) endFrames(id, transportError(0, 'Conexão encerrada.'), true) // textos-ok: diagnóstico do transporte
    for (const window of windows()) {
      if (window.isDestroyed() || window.webContents.isDestroyed()) continue
      const frame = window.webContents.mainFrame
      const allowed = urls.some(url => { try { const u = new URL(url); return (u.protocol === 'file:' && u.host === '' || ['http:', 'https:'].includes(u.protocol) && ['localhost', '127.0.0.1', '[::1]'].includes(u.hostname)) && frame.url.split('#')[0] === url.split('#')[0] } catch { return false } })
      if (allowed) window.webContents.send('forkCloud:change', value)
    }
  })
  // Runs in Electron main even when the customer has not opened the Computers
  // screen. A session/offer/device fence is rechecked before every action.
  // Pace and pauses: managed-poll.ts (DON-863).
  poll=new ManagedPoll(()=>service.pollManaged(), ()=>powerMonitor.getSystemIdleTime()>=AWAY_IDLE_S)
  const read=poll
  const seen=()=>visible() ? read.resume() : read.pause()
  const watch=(w: BrowserWindow)=>{ for (const e of ['show','hide','minimize','restore'] as const) w.on(e as 'show',()=>setImmediate(seen)); w.once('closed',()=>setImmediate(seen)) }
  BrowserWindow.getAllWindows().forEach(watch)
  const created=(_: unknown, w: BrowserWindow)=>watch(w)
  const focused=()=>{ if (!visible()) read.pause(); else if (!read.resume()) read.now() }
  const away=()=>read.pause(), back=()=>{ if (visible() && !read.resume()) read.now() }
  const signedIn=()=>read.now()
  app.on('browser-window-created',created); app.on('browser-window-focus',focused)
  for (const e of ['lock-screen','suspend'] as const) powerMonitor.on(e as 'suspend',away)
  for (const e of ['unlock-screen','resume'] as const) powerMonitor.on(e as 'resume',back)
  account.on('epoch',signedIn)
  if (visible()) read.now(); else read.pause()
  ipcMain.handle('forkCloud:invoke', async (event, ...args: unknown[]) => {
    const denied = () => ({ registration: 'unavailable' as const, recovery: null, state: { kind: 'notSent', epoch: account.sessionEpoch, attemptId: '' }, computers: [] })
    if (!live(event) || args.length !== 1) return denied()
    const frame = event.senderFrame, epoch = account.sessionEpoch
    const result = await service.invoke(args[0], () => live(event) && event.senderFrame === frame && account.sessionEpoch === epoch)
    if (!(args[0] && typeof args[0] === 'object' && (args[0] as { kind?: unknown }).kind === 'state')) read.now()
    return live(event) && event.senderFrame === frame && account.sessionEpoch === epoch ? result : denied()
  })
  ipcMain.handle('forkCloud:call', async (event, ...args: unknown[]) => {
    if (!live(event) || args.length !== 4) return { kind: 'notSent', reason: 'notReady' }
    const frame = event.senderFrame
    const result = await service.call(args[0], args[1], args[2], args[3])
    if (!live(event) || event.senderFrame !== frame) return result.kind === 'notSent' ? result : { kind: 'unknown', reason: 'closed' }
    return result
  })
  ipcMain.on('forkCloud:events:open', (event, ...args: unknown[]) => {
    if (args.length !== 3) return
    const [id, connectionId, body] = args
    if (!live(event) || typeof id !== 'string' || !/^[a-f0-9]{32}$/.test(id)) return
    if (typeof connectionId !== 'string') { event.sender.send('forkCloud:events:end', id, { status: 400, message: 'Assinatura inválida.' }); return }
    const mapKey = key(event, id)
    if (subscriptions.has(mapKey)) return
    const frame = event.senderFrame
    const changed = () => end(mapKey, null, false)
    event.sender.on('did-start-navigation', changed); event.sender.once('destroyed', changed)
    const cleanup = () => { event.sender.removeListener('did-start-navigation', changed); event.sender.removeListener('destroyed', changed) }
    const entry: Entry = { event, connectionId, sub: null, cleanup }
    subscriptions.set(mapKey, entry)
    entry.sub = service.subscribe(connectionId, body, value => {
      if (!live(event) || event.senderFrame !== frame || !service.dataConnection(connectionId)) { end(mapKey, null, false); return }
      event.sender.send('forkCloud:events:data', id, value)
    }, error => { if (event.senderFrame === frame) end(mapKey, error, true); else end(mapKey, null, false) })
    if (!entry.sub) end(mapKey, { status: 0, message: 'Assinatura indisponível.' }, true)
  })
  ipcMain.on('forkCloud:events:cancel', (event, ...args: unknown[]) => {
    if (args.length !== 1) return
    const [id] = args
    if (!live(event) || typeof id !== 'string') return
    const mapKey = key(event, id), entry = subscriptions.get(mapKey)
    if (entry && entry.event.sender === event.sender && entry.event.senderFrame === event.senderFrame) end(mapKey, null, false)
  })
  // B2: quadros binários do navegador do bot. Um quadro por vez até a tela confirmar; fila de no máximo um.
  ipcMain.on('forkCloud:frames:open', (event, ...args: unknown[]) => {
    if (args.length !== 2) return
    const [id, connectionId] = args
    if (!live(event) || typeof id !== 'string' || !/^[a-f0-9]{32}$/.test(id)) return
    if (typeof connectionId !== 'string') {
      event.sender.send('forkCloud:frames:end', id, transportError(400, 'Assinatura inválida.')) // textos-ok: diagnóstico do transporte
      return
    }
    const mapKey = key(event, id)
    if (frames.has(mapKey)) return
    const frame = event.senderFrame
    const current = () => live(event) && event.senderFrame === frame && !!service.dataConnection(connectionId)
    const changed = () => endFrames(mapKey, null, false)
    // Só a troca de documento da janela principal encerra: âncora (mesmo documento) e subquadro não mexem na tela aberta.
    const navigated = (details?: { isMainFrame?: boolean; isSameDocument?: boolean }) => { if (details?.isMainFrame !== false && details?.isSameDocument !== true) changed() }
    event.sender.on('did-start-navigation', navigated); event.sender.once('destroyed', changed)
    const cleanup = () => { event.sender.removeListener('did-start-navigation', navigated); event.sender.removeListener('destroyed', changed) }
    const gate = createFrameGate<BrowserFrame>((value, waited) => { if (!current() || !frames.has(mapKey)) return endFrames(mapKey, null, false); event.sender.send('forkCloud:frames:data', id, value, waited); service.frameDispatched(connectionId, value) })
    const entry: FrameEntry = { event, connectionId, sub: null, gate, cleanup }
    frames.set(mapKey, entry)
    entry.sub = service.subscribeFrames(connectionId, { onFrame: value => gate.offer(value), onEnd: error => endFrames(mapKey, error, true) })
    if (entry.sub) { if (live(event)) event.sender.send('forkCloud:frames:opened', id); return }
    const connection = service.dataConnection(connectionId)
    // 0: conexão encerrada; 501: capacidade não negociada; 409: negociada, mas o consumidor único de quadros já está em uso.
    endFrames(mapKey, transportError(!connection ? 0 : connection.framesNegotiated ? 409 : 501, 'Assinatura indisponível.'), true) // textos-ok: diagnóstico do transporte
  })
  ipcMain.on('forkCloud:frames:consumed', (event, ...args: unknown[]) => {
    if (args.length !== 1) return
    const [id] = args
    if (!live(event) || typeof id !== 'string') return
    const entry = frames.get(key(event, id))
    if (entry && entry.event.sender === event.sender && entry.event.senderFrame === event.senderFrame) entry.gate.consumed()
  })
  ipcMain.on('forkCloud:frames:cancel', (event, ...args: unknown[]) => {
    if (args.length !== 1) return
    const [id] = args
    if (!live(event) || typeof id !== 'string') return
    const mapKey = key(event, id), entry = frames.get(mapKey)
    if (entry && entry.event.sender === event.sender && entry.event.senderFrame === event.senderFrame) endFrames(mapKey, null, false)
  })
  app.once('will-quit', () => {
    read.stop(); account.off('epoch',signedIn)
    app.off('browser-window-created',created); app.off('browser-window-focus',focused)
    for (const e of ['lock-screen','suspend'] as const) powerMonitor.off(e as 'suspend',away)
    for (const e of ['unlock-screen','resume'] as const) powerMonitor.off(e as 'resume',back)
    for (const id of subscriptions.keys()) end(id, null, false)
    for (const id of frames.keys()) endFrames(id, null, false)
    ipcMain.removeHandler('forkCloud:invoke'); ipcMain.removeHandler('forkCloud:call')
    ipcMain.removeAllListeners('forkCloud:events:open'); ipcMain.removeAllListeners('forkCloud:events:cancel'); service.dispose()
    ipcMain.removeAllListeners('forkCloud:frames:open'); ipcMain.removeAllListeners('forkCloud:frames:consumed'); ipcMain.removeAllListeners('forkCloud:frames:cancel')
  })
  return service
}
