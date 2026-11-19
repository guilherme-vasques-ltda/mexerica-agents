import { signatureHeader, unb64url, type DeviceSigner } from './crypto'
import { deviceSigner } from './device-keys'
import { matchesDeployment } from './pins'
import type { Principal } from './principal'

type Platform = 'ios' | 'macos' | 'linux' | 'windows' | 'android'
export type ClaimProof = { computerId: string; signKey: string; boxKey: string; sig: string; name: string; platform: 'macos' | 'linux' | 'windows'; device?: string | null; version: string }
export type AccountIntent =
  | { kind: 'health' }
  | { kind: 'managedOffer' }
  | { kind: 'registerDevice'; name: string; platform: Platform }
  | { kind: 'listDevices' }
  | { kind: 'revokeDevice'; deviceId: string }
  | { kind: 'listComputers' }
  | { kind: 'createClaim' }
  | { kind: 'completeClaim'; claimId: string; proof: ClaimProof }
  | { kind: 'requestAccess'; computerId: string; commit: string }
  | { kind: 'getAccess'; requestId: string }
  | { kind: 'cancelAccess'; requestId: string }
  | { kind: 'revealAccess'; requestId: string; nonce: string }
  | { kind: 'bootstrapCreate'; bootstrapId: string; nonceApp: string }
  | { kind: 'bootstrapRead'; bootstrapId: string }
  | { kind: 'bootstrapReconcile'; bootstrapId: string }
export type AccountResult = { kind: 'ok'; value: Record<string, unknown> } | { kind: 'notSent' | 'unknown' | 'unavailable' } | { kind: 'refused'; status: number; code: string }

type Route = { method: 'GET' | 'POST' | 'DELETE'; path: string; body?: object; signed?: 'sign' | 'register' }
const id = (value: unknown, prefix: string) => typeof value === 'string' && new RegExp(`^${prefix}[A-Za-z0-9_-]{1,60}$`).test(value)
const computer = (value: unknown) => typeof value === 'string' && unb64url(value, 16) !== null
const bytes = (value: unknown, count: number) => unb64url(value, count) !== null
const text = (value: unknown, limit: number) => typeof value === 'string' && value.length > 0 && value.length <= limit
const keys = (value: unknown, allowed: string[]) => !!value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).every((key) => allowed.includes(key))

/** A tabela é fechada e construída aqui; nenhum chamador fornece método ou caminho. */
function route(intent: AccountIntent): Route | null {
  switch (intent.kind) {
    case 'managedOffer': return keys(intent, ['kind']) ? { method:'GET', path:'/v1/bootstrap/managed-offer' } : null
    case 'health': return keys(intent, ['kind']) ? { method: 'GET', path: '/v1/health' } : null
    case 'registerDevice':
      return keys(intent, ['kind', 'name', 'platform']) && text(intent.name, 100) && ['ios', 'macos', 'linux', 'windows', 'android'].includes(intent.platform)
        ? { method: 'POST', path: '/v1/devices', body: { name: intent.name, platform: intent.platform }, signed: 'register' } : null
    case 'listDevices': return keys(intent, ['kind']) ? { method: 'GET', path: '/v1/devices' } : null
    case 'revokeDevice': return keys(intent, ['kind', 'deviceId']) && id(intent.deviceId, 'dev_') ? { method: 'DELETE', path: `/v1/devices/${intent.deviceId}` } : null
    case 'listComputers': return keys(intent, ['kind']) ? { method: 'GET', path: '/v1/computers', signed: 'sign' } : null
    case 'createClaim': return keys(intent, ['kind']) ? { method: 'POST', path: '/v1/claims', body: {} } : null
    case 'completeClaim': {
      const p = intent.proof
      if (!keys(intent, ['kind', 'claimId', 'proof']) || !id(intent.claimId, 'clm_') || !keys(p, ['computerId', 'signKey', 'boxKey', 'sig', 'name', 'platform', 'device', 'version']) || !computer(p.computerId) || !bytes(p.signKey, 32) || !bytes(p.boxKey, 32) || !bytes(p.sig, 64) || !text(p.name, 100) || !['macos', 'linux', 'windows'].includes(p.platform) || !text(p.version, 32) || (p.device != null && !text(p.device, 32))) return null
      return { method: 'POST', path: `/v1/claims/${intent.claimId}/complete`, body: { ...p } }
    }
    case 'requestAccess': return keys(intent, ['kind', 'computerId', 'commit']) && computer(intent.computerId) && bytes(intent.commit, 32) ? { method: 'POST', path: `/v1/computers/${intent.computerId}/access-requests`, body: { commit: intent.commit }, signed: 'sign' } : null
    case 'getAccess': return keys(intent, ['kind', 'requestId']) && id(intent.requestId, 'req_') ? { method: 'GET', path: `/v1/access-requests/${intent.requestId}` } : null
    case 'cancelAccess': return keys(intent, ['kind', 'requestId']) && id(intent.requestId, 'req_') ? { method: 'DELETE', path: `/v1/access-requests/${intent.requestId}` } : null
    case 'revealAccess': return keys(intent, ['kind', 'requestId', 'nonce']) && id(intent.requestId, 'req_') && bytes(intent.nonce, 32) ? { method: 'POST', path: `/v1/access-requests/${intent.requestId}/reveal`, body: { nonce: intent.nonce }, signed: 'sign' } : null
    case 'bootstrapCreate':
      return keys(intent, ['kind', 'bootstrapId', 'nonceApp']) && typeof intent.bootstrapId === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(intent.bootstrapId) && bytes(intent.nonceApp, 32)
        ? { method: 'POST', path: '/v1/bootstrap/intents', body: { bootstrapId: intent.bootstrapId, nonceApp: intent.nonceApp }, signed: 'sign' } : null
    case 'bootstrapRead':
      return keys(intent, ['kind', 'bootstrapId']) && typeof intent.bootstrapId === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(intent.bootstrapId)
        ? { method: 'GET', path: `/v1/bootstrap/intents/${intent.bootstrapId}`, signed: 'sign' } : null
    case 'bootstrapReconcile':
      return keys(intent, ['kind', 'bootstrapId']) && typeof intent.bootstrapId === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(intent.bootstrapId)
        ? { method: 'POST', path: `/v1/bootstrap/intents/${intent.bootstrapId}/reconcile`, signed: 'sign' } : null
    default: return null
  }
}

export class AccountApi {
  private verified = false
  private signer: { mode: 'existing' | 'register'; value: Promise<DeviceSigner> } | null = null
  constructor(private readonly principal: Principal) {}

  private async signerFor(mode: 'sign' | 'register'): Promise<DeviceSigner> {
    if (!this.principal.current()) throw new Error('Account session changed.')
    let captured = this.signer
    if (!captured) {
      captured = { mode: mode === 'register' ? 'register' : 'existing', value: deviceSigner(this.principal, mode) }
      this.signer = captured
    }
    if (mode === 'register' && captured.mode !== 'register') throw new Error('Registration requires its own explicit capture.')
    const signer = await captured.value
    if (!this.principal.current()) throw new Error('Account session changed.')
    return signer
  }

  /** Explicit registration fixes its key before POST; reconciliation never calls this. */
  async prepareRegistrationKey(): Promise<Uint8Array> {
    return new Uint8Array((await this.signerFor('register')).publicKey)
  }

  /** The attempt's commit and subsequent signatures use this same internal capture. */
  async captureDeviceKey(): Promise<Uint8Array> {
    return new Uint8Array((await this.signerFor('sign')).publicKey)
  }

  async execute(intent: AccountIntent): Promise<AccountResult> {
    const route = intent && typeof intent === 'object' ? routeFor(intent) : null
    if (!route || !this.principal.current()) return { kind: 'notSent' }
    if (intent.kind === 'health') this.verified = false
    if (intent.kind !== 'health' && !this.verified) return { kind: 'unavailable' }
    if (route.signed === 'register' && this.signer?.mode === 'existing') return { kind: 'notSent' }
    let sent = false
    try {
      const url = new URL(route.path, this.principal.deployment.origin)
      if (url.origin !== this.principal.deployment.origin || url.protocol !== 'https:') return { kind: 'notSent' }
      const payload = route.body === undefined ? undefined : Buffer.from(JSON.stringify(route.body))
      const headers: Record<string, string> = {}
      if (intent.kind !== 'health') {
        const token = await this.principal.token()
        if (!this.principal.current()) return { kind: 'notSent' }
        headers.Authorization = `Bearer ${token}`
      }
      if (payload) headers['Content-Type'] = 'application/json'
      if (route.signed) {
        const signer = await this.signerFor(route.signed)
        if (!this.principal.current()) return { kind: 'notSent' }
        headers['Codync-Sig'] = signatureHeader(signer, route.method, url, payload)
      }
      if (!this.principal.current()) return { kind: 'notSent' }
      sent = true
      const response = await fetch(url, { method: route.method, headers, body: payload, redirect: 'error', signal: AbortSignal.timeout(20_000) })
      if (!this.principal.current()) return { kind: 'unknown' }
      const value: unknown = await response.json()
      if (!this.principal.current()) return { kind: 'unknown' }
      if (!value || typeof value !== 'object' || Array.isArray(value)) return { kind: 'unknown' }
      if (!response.ok) {
        const error = (value as { error?: { code?: unknown } }).error
        // Both producers of this code in bootstrapCreate are before INSERT.
        // Other 5xx and all ambiguous post-commit conflicts remain unknown.
        if (intent.kind === 'bootstrapCreate' && response.status === 503 && error?.code === 'bootstrapUnavailable') return { kind: 'refused', status: response.status, code: error.code }
        if (response.status >= 500) return { kind: 'unknown' }
        return typeof error?.code === 'string' && /^[A-Za-z0-9_]{1,64}$/.test(error.code) ? { kind: 'refused', status: response.status, code: error.code } : { kind: 'unknown' }
      }
      if (intent.kind === 'health') {
        this.verified = matchesDeployment(this.principal.deployment, value)
        if (!this.verified) return { kind: 'unavailable' }
      }
      return { kind: 'ok', value: value as Record<string, unknown> }
    } catch { return { kind: sent ? 'unknown' : 'notSent' } }
  }
}

// Kept separate from the variable in execute to avoid accepting dynamic paths.
const routeFor = route
