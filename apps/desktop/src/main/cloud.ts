// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { forkFeatures } from '../shared/fork-features'
import { createHash, randomBytes, sign } from 'node:crypto'
import { ipcMain } from 'electron'
import type { AccountService, CloudSeal, DeviceRevocation } from './account'
import { openAccountRelay, type AccountRelay } from './fork-cloud/account-relay'
import type { RelayLinkOptions } from './fork-cloud/relay-socket'
import { deviceIdentity, retireDeviceIdentity, discardDeviceIdentity, verifyDeviceGeneration, type DeviceIdentity as Identity, type DeviceScope } from './fork-account/device-key'

/**
 * The Codync cloud's user-facing `/v1` API (§8.4) with this install's device key (§3.2): an
 * Ed25519 key per account, kept encrypted with the OS keychain, that signs `Codync-Sig` (§5).
 */

const b64url = (b: Buffer) => b.toString('base64url')

/** `Codync-Sig: v=1,kid,ts,nonce,sig` over method, authority, path and the body's hash. */
function signatureHeader(id: Identity, method: string, url: URL, body: Buffer) {
  const ts = Date.now()
  const nonce = b64url(randomBytes(16))
  const input = ['codync-sig-v1', method.toUpperCase(), url.host.toLowerCase(), `${url.pathname || '/'}${url.search}`, String(ts), nonce, b64url(createHash('sha256').update(body).digest())].join('\n')
  const sig = sign(null, Buffer.from(input), id.key)
  return `v=1,kid=${id.publicKey},ts=${ts},nonce=${nonce},sig=${b64url(sig)}`
}

export interface CloudFailure {
  status: number
  code: string
  message: string | null
}

interface RegisteredDevice { seal: CloudSeal; origin: string; id: string; identity: Identity }

export function registerCloud(account: AccountService) {
  if (!forkFeatures.cloud || forkFeatures.cloudAccess) return
  const scope = (): DeviceScope => { const value = account.deviceIdentityScope(); if (!value) throw new Error('Account configuration unavailable.'); return value }
  account.setReconciliationKeyReader?.(async (userId) => {
    try { return (await deviceIdentity(userId, () => true, 'read', scope())).publicKey } catch { return null }
  })
  account.setReconciliationRegistrar?.(async (userId, expectedKey, jwt, origin, current) => {
    try {
      const existing = await deviceIdentity(userId, current, 'read', scope())
      if (!current() || existing.publicKey !== expectedKey || !['darwin', 'linux', 'win32'].includes(process.platform)) return null
      const url = new URL('/v1/devices', origin)
      const body = Buffer.from(JSON.stringify({ name: 'Donnabot', platform: ({ darwin: 'macos', linux: 'linux', win32: 'windows' } as Record<string, string>)[process.platform] }))
      const headers = { Authorization: `Bearer ${jwt}`, 'Content-Type': 'application/json', 'Codync-Sig': signatureHeader(existing, 'POST', url, body) }
      if (!current()) return null
      const response = await fetch(url, { method: 'POST', headers, body, redirect: 'error', signal: AbortSignal.timeout(20_000) })
      if (!current() || !response.ok) return null
      const result = await response.json() as { device?: { deviceId?: unknown; deviceKey?: unknown } }
      if (!current() || result.device?.deviceKey !== expectedKey || typeof result.device.deviceId !== 'string' || !/^dev_[A-Za-z0-9_-]+$/.test(result.device.deviceId)) return null
      return result.device.deviceId
    } catch { return null }
  })
  account.setReconciliationKeyVerifier(async (userId, expectedKey) => {
    try {
      return await verifyDeviceGeneration(userId, expectedKey, scope())
    } catch { return false }
  })
  account.setDeviceIdentityResetter?.(async (binding, current) => {
    await discardDeviceIdentity(binding.userId, current)
  })
  account.setDeviceIdentityRetirer?.(async (binding, current) => {
    try { await retireDeviceIdentity(binding, current); return current() } catch { return false }
  })
  let registered: RegisteredDevice | null = null
  account.setDeviceRevoker((seal, binding) => {
    const captured = registered
    if (!binding || !captured || binding.userId !== seal.userId || binding.sessionId !== seal.sessionId || binding.deviceId !== captured.id || binding.deviceKey !== captured.identity.publicKey || binding.origin !== captured.origin || captured.seal.epoch !== seal.epoch || captured.seal.sessionId !== seal.sessionId || captured.seal.userId !== seal.userId) return null
    registered = null
    return async (jwt, current): Promise<DeviceRevocation> => {
      const url = new URL(`/v1/devices/${encodeURIComponent(captured.id)}`, captured.origin)
      try {
        if (!current()) return { kind: 'unknown' }
        const response = await fetch(url, { method: 'DELETE', headers: { Authorization: `Bearer ${jwt}` }, redirect: 'error', signal: AbortSignal.timeout(20_000) })
        if (!response.ok || !current()) return { kind: 'unknown' }
        const list = await fetch(new URL('/v1/devices', captured.origin), { method: 'GET', headers: { Authorization: `Bearer ${jwt}` }, redirect: 'error', signal: AbortSignal.timeout(20_000) })
        if (!list.ok || !current()) return { kind: 'unknown' }
        const body = await list.json() as { devices?: unknown }
        if (!current() || !Array.isArray(body.devices)) return { kind: 'unknown' }
        const matches = body.devices.filter((item: { deviceId?: unknown }) => item.deviceId === captured.id)
        return matches.length === 1 && matches[0]?.deviceKey === captured.identity.publicKey && matches[0]?.revoked === true ? { kind: 'revoked' } : { kind: 'unknown' }
      } catch { return { kind: 'unknown' } }
    }
  })
  account.on('epoch', () => { registered = null })
  account.setBindingRestorer(async (seal, binding) => {
    const current = () => account.isCloudSessionCurrent(seal)
    try {
      const existing = await deviceIdentity(seal.userId, current, 'read', scope())
      if (current() && existing.publicKey === binding.deviceKey && binding.origin === account.state.cloudURL) {
        registered = { seal, origin: binding.origin, id: binding.deviceId, identity: existing }
        return true
      }
    } catch { /* No existing valid key: logout remains ineligible. */ }
    return false
  })
  ipcMain.handle('cloud:request', async (_e, method: string, path: string, body: unknown, signed: boolean) => {
    const seal = account.captureCloudSession()
    const base = account.state.cloudURL
    const notSent = { ok: false, error: { status: 0, code: 'notSent', message: null } satisfies CloudFailure }
    const unknown = { ok: false, error: { status: 0, code: 'unknown', message: null } satisfies CloudFailure }
    if (!seal || !base) return notSent
    const current = () => account.isCloudSessionCurrent(seal)
    let url: URL
    try {
      url = new URL(path.replace(/^\//, ''), `${base}/`)
      if (url.origin !== base) return notSent
    } catch { return notSent }
    const payload = body === undefined ? Buffer.alloc(0) : Buffer.from(JSON.stringify(body))
    const headers: Record<string, string> = {}
    let signer: Identity | null = null
    let sent = false
    try {
      headers.Authorization = `Bearer ${await account.tokenForCloud(seal)}`
      if (!current()) return notSent
      if (body !== undefined) headers['Content-Type'] = 'application/json'
      if (signed) {
        signer = await deviceIdentity(seal.userId, current, method === 'POST' && url.pathname === '/v1/devices' ? 'register' : 'sign', scope())
        if (!current()) return notSent
        headers['Codync-Sig'] = signatureHeader(signer, method, url, payload)
      }
      if (!current()) return notSent
      sent = true
      const res = await fetch(url, { method, headers, body: body === undefined ? undefined : payload, redirect: 'error', signal: AbortSignal.timeout(20_000) })
      if (!current()) return unknown
      const text = await res.text()
      if (!current()) return unknown
      const json = text ? (JSON.parse(text) as Record<string, unknown>) : {}
      if (!res.ok) {
        const e = (json.error ?? {}) as { code?: string; message?: string }
        return { ok: false, error: { status: res.status, code: e.code ?? `http${res.status}`, message: e.message ?? null } }
      }
      if (method === 'POST' && url.pathname === '/v1/devices' && signer) {
        const device = json.device as { deviceId?: unknown; deviceKey?: unknown } | undefined
        if (typeof device?.deviceId === 'string' && /^dev_[A-Za-z0-9_-]+$/.test(device.deviceId) && device.deviceKey === signer.publicKey) {
          const binding = await account.saveDeviceBinding(seal, device.deviceId, signer.publicKey)
          if (!binding || !current()) return unknown
          registered = { seal, origin: base, id: binding.deviceId, identity: signer }
        } else return unknown
      }
      if (!current()) return unknown
      return { ok: true, value: json }
    } catch {
      return sent ? unknown : notSent
    }
  })
}

/** Internal only; no IPC entry point or inferred Hub association. */
export function openSealedRelay(account: AccountService, options: RelayLinkOptions): Promise<AccountRelay> {
  if (!forkFeatures.cloud || forkFeatures.cloudAccess) return Promise.resolve({ kind: 'notEligible' })
  return openAccountRelay(account, options)
}
