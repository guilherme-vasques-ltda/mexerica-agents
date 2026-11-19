import { forkFeatures } from '../../shared/fork-features'
import type { AccountService, CloudSeal, DeviceBinding, DeviceRevocation } from '../fork-account/service'
import { accountConfig } from '../fork-account/config'
import { deviceIdentity, verifyDeviceGeneration } from '../fork-account/device-key'
import { buildConfig } from '../environment'
import { AccountApi } from './account-api'
import { b64url, signatureHeader, signerFromKey, unb64url } from './crypto'
import { deviceSigner, retireCapturedDevice, discardCapturedDevice } from './device-keys'
import { matchesDeployment } from './pins'
import { capturePrincipal, namespaceFor, type Deployment, type Principal } from './principal'

type Platform = 'ios' | 'macos' | 'linux' | 'windows' | 'android'
type Registered = { readonly seal: CloudSeal; readonly binding: DeviceBinding }
type RegisterResult = { kind: 'registered' } | { kind: 'notSent' | 'unknown' | 'unavailable' }
const idValid = (value: unknown): value is string => typeof value === 'string' && /^dev_[A-Za-z0-9_-]{1,60}$/.test(value)
const sameBinding = (a: DeviceBinding, b: DeviceBinding) => (['version', 'userId', 'sessionId', 'origin', 'issuer', 'deploymentId', 'manifestHash', 'deviceId', 'deviceKey'] as const).every((key) => a[key] === b[key])
const sameSeal = (a: CloudSeal, b: CloudSeal) => a.epoch === b.epoch && a.sessionId === b.sessionId && a.userId === b.userId

/** Só uma fábrica interna; não instala IPC nem inicia registro ou rede. */
export function registerForkCloud(account: AccountService): { registerDevice(name: string, platform: Platform): Promise<RegisterResult>; reconcileRegistration(): Promise<RegisterResult> } | null {
  const config = accountConfig(buildConfig)
  if (!forkFeatures.cloudAccess || config.kind !== 'configured' || account.state.cloudURL !== config.cloudOrigin) return null
  const deployment: Deployment = Object.freeze({ deploymentId: config.deploymentId, env: config.env, issuer: config.issuer, origin: config.cloudOrigin, manifestHash: config.manifestHash })
  const scope = { origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash }
  let registered: Registered | null = null
  let retirementProof: { binding: DeviceBinding; current: () => boolean } | null = null
  let registering = false
  const pendingRegistrations = new Map<string, { deviceKey: string; expectedDeviceId: string | null }>()
  account.on('epoch', () => { registered = null; retirementProof = null })

  // O restaurador recebe um selo já emitido pelo serviço durante a restauração;
  // captureCloudSession() recusa esse intervalo e não pode recapturar outra conta.
  const fromSeal = (seal: CloudSeal): Principal | null => {
    const actual = account.deviceIdentityScope()
    if (!account.isCloudSessionCurrent(seal) || !actual || actual.origin !== scope.origin || actual.issuer !== scope.issuer || actual.deploymentId !== scope.deploymentId || actual.manifestHash !== scope.manifestHash) return null
    const current = () => account.isCloudSessionCurrent(seal)
    return Object.freeze({ seal, deployment, namespace: namespaceFor(deployment, seal.userId), current, async token() {
      if (!current()) throw new Error('Account session changed.')
      const jwt = await account.tokenForCloud(seal)
      if (!current()) throw new Error('Account session changed.')
      return jwt
    } })
  }
  const bindingInDeployment = (binding: DeviceBinding) => binding.version === 1 && typeof binding.userId === 'string' && binding.userId.length > 0 && typeof binding.sessionId === 'string' && binding.sessionId.length > 0 && binding.origin === deployment.origin && binding.issuer === deployment.issuer && binding.deploymentId === deployment.deploymentId && binding.manifestHash === deployment.manifestHash && idValid(binding.deviceId) && !!unb64url(binding.deviceKey, 32)
  const bindingMatches = (seal: CloudSeal, binding: DeviceBinding) => bindingInDeployment(binding) && binding.userId === seal.userId && binding.sessionId === seal.sessionId

  const healthy = async (current: () => boolean) => {
    if (!current()) return false
    const reply = await fetch(new URL('/v1/health', deployment.origin), { method: 'GET', redirect: 'error', signal: AbortSignal.timeout(20_000) })
    return current() && reply.ok && matchesDeployment(deployment, await reply.json()) && current()
  }
  const readRevocationProof = async (binding: DeviceBinding, jwt: string, current: () => boolean): Promise<boolean> => {
    if (!current() || !jwt || !bindingInDeployment(binding)) return false
    const response = await fetch(new URL('/v1/devices', deployment.origin), { method: 'GET', headers: { Authorization: `Bearer ${jwt}` }, redirect: 'error', signal: AbortSignal.timeout(20_000) })
    if (!current() || !response.ok) return false
    const body = await response.json() as { devices?: unknown }
    if (!current() || !Array.isArray(body.devices)) return false
    const rows = body.devices.filter((row: { deviceId?: unknown }) => row?.deviceId === binding.deviceId)
    const device = rows[0] as { deviceKey?: unknown; revoked?: unknown; revocations?: unknown } | undefined
    if (rows.length !== 1 || device?.deviceKey !== binding.deviceKey || device.revoked !== true || !Array.isArray(device.revocations)) return false
    const timestamp = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
    const seen = new Set<string>()
    for (const raw of device.revocations) {
      if (!raw || typeof raw !== 'object') return false
      const receipt = raw as { fenceId?: unknown; grantId?: unknown; computerId?: unknown; deviceId?: unknown; deviceKey?: unknown; deploymentId?: unknown; env?: unknown; cause?: unknown; requestedAt?: unknown; relay?: { state?: unknown; at?: unknown; attempts?: unknown }; hub?: { state?: unknown; at?: unknown } }
      const requested = receipt.requestedAt, relayAt = receipt.relay?.at, hubAt = receipt.hub?.at
      if (typeof receipt.fenceId !== 'string' || typeof receipt.grantId !== 'string' || receipt.fenceId !== `fnc_${receipt.grantId}` || !/^grt_[A-Za-z0-9_-]{1,60}$/.test(receipt.grantId) || seen.has(receipt.grantId) || !unb64url(receipt.computerId, 16) || receipt.deviceId !== binding.deviceId || receipt.deviceKey !== binding.deviceKey || receipt.deploymentId !== deployment.deploymentId || receipt.env !== deployment.env || typeof receipt.cause !== 'string' || !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(receipt.cause) || !timestamp(requested) || !timestamp(relayAt) || !timestamp(hubAt) || relayAt < requested || hubAt < requested || !timestamp(receipt.relay?.attempts) || receipt.relay?.state !== 'fenced' || receipt.hub?.state !== 'fenced') return false
      seen.add(receipt.grantId)
    }
    // An explicit empty revocations list for this revoked device means zero
    // grants under v3 §4.5; an absent device/field or an empty ACL proves nothing.
    if (!current()) return false
    retirementProof = { binding: Object.freeze({ ...binding }), current }
    return true
  }
  account.setDeviceRevocationVerifier(async (binding, jwt, current): Promise<DeviceRevocation> => {
    if (!current() || !jwt || !bindingInDeployment(binding)) return { kind: 'unknown' }
    retirementProof = null
    try {
      return await healthy(current) && await readRevocationProof(binding, jwt, current) ? { kind: 'revoked' } : { kind: 'unknown' }
    } catch { return { kind: 'unknown' } }
  })

  account.setBindingRestorer(async (seal, binding) => {
    const principal = fromSeal(seal)
    if (!principal || !bindingMatches(seal, binding)) return false
    try {
      const signer = await deviceSigner(principal, 'read')
      if (!principal.current() || b64url(signer.publicKey) !== binding.deviceKey) return false
      registered = { seal, binding: Object.freeze({ ...binding }) }
      return true
    } catch { return false }
  })

  account.setDeviceRevoker((seal, binding) => {
    const target = registered
    if (!target || !binding || !sameSeal(seal, target.seal) || !sameBinding(binding, target.binding) || !bindingMatches(seal, binding) || !account.isCloudSessionCurrent(seal)) return null
    registered = null
    const captured = Object.freeze({ ...binding })
    let sent = false
    return async (jwt, current): Promise<DeviceRevocation> => {
      if (sent || !current() || !jwt) return { kind: 'unknown' }
      sent = true
      try {
        const headers = { Authorization: `Bearer ${jwt}` }
        if (!(await healthy(current))) return { kind: 'unknown' }
        const deleted = await fetch(new URL(`/v1/devices/${encodeURIComponent(captured.deviceId)}`, deployment.origin), { method: 'DELETE', headers, redirect: 'error', signal: AbortSignal.timeout(20_000) })
        if (!current() || !deleted.ok) return { kind: 'unknown' }
        return await readRevocationProof(captured, jwt, current) ? { kind: 'revoked' } : { kind: 'unknown' }
      } catch { return { kind: 'unknown' } }
    }
  })

  // Essas leituras da reconciliação nunca geram uma chave; a época é capturada
  // mesmo quando a sessão normal já foi cercada.
  account.setReconciliationKeyReader(async (userId) => {
    const epoch = account.sessionEpoch
    const current = () => account.sessionEpoch === epoch
    try { const found = await deviceIdentity(namespaceFor(deployment, userId), current, 'read', scope); return current() ? found.publicKey : null } catch { return null }
  })
  account.setReconciliationKeyVerifier(async (userId, key) => {
    const epoch = account.sessionEpoch
    const result = await verifyDeviceGeneration(namespaceFor(deployment, userId), key, scope)
    return account.sessionEpoch === epoch && result
  })
  account.setReconciliationRegistrar(async (userId, key, jwt, origin, current) => {
    if (!current() || origin !== deployment.origin || !jwt || !unb64url(key, 32)) return null
    try {
      const identity = await deviceIdentity(namespaceFor(deployment, userId), current, 'read', scope)
      if (!current() || identity.publicKey !== key) return null
      const health = await fetch(new URL('/v1/health', origin), { method: 'GET', redirect: 'error', signal: AbortSignal.timeout(20_000) })
      if (!current() || !health.ok || !matchesDeployment(deployment, await health.json()) || !current()) return null
      const platform = ({ darwin: 'macos', linux: 'linux', win32: 'windows' } as Record<string, string>)[process.platform]
      if (!platform) return null
      const url = new URL('/v1/devices', origin)
      const body = Buffer.from(JSON.stringify({ name: 'Donnabot', platform }))
      const headers = { Authorization: `Bearer ${jwt}`, 'Content-Type': 'application/json', 'Codync-Sig': signatureHeader(signerFromKey(identity.key), 'POST', url, body) }
      if (!current()) return null
      const response = await fetch(url, { method: 'POST', headers, body, redirect: 'error', signal: AbortSignal.timeout(20_000) })
      if (!current() || !response.ok) return null
      const result = await response.json() as { device?: { deviceId?: unknown; deviceKey?: unknown } }
      return current() && result.device?.deviceKey === key && idValid(result.device.deviceId) ? result.device.deviceId : null
    } catch { return null }
  })
  // D185: discard the captured account/configuration identity before re-login.
  // Local epoch + key-store lock exclude a later generation; no remote proof.
  account.setDeviceIdentityResetter?.(async (binding, current) => {
    if (!current() || !bindingInDeployment(binding)) throw new Error('Different device binding.')
    await discardCapturedDevice(binding, deployment, current)
    if (!current()) throw new Error('Account operation changed.')
    pendingRegistrations.delete(namespaceFor(deployment, binding.userId))
  })
  account.setDeviceIdentityRetirer(async (binding, current) => {
    const proof = retirementProof
    if (!proof || !current() || !proof.current() || !sameBinding(binding, proof.binding)) return false
    const allowed = () => current() && proof.current()
    try { await retireCapturedDevice(binding, deployment, allowed); if (!allowed()) return false; if (retirementProof === proof) retirementProof = null; return true } catch { return false }
  })

  return { async registerDevice(name, platform) {
    if (registering) return { kind: 'notSent' }
    const principal = capturePrincipal(account)
    if (!principal) return { kind: 'unavailable' }
    registering = true
    try {
      const api = new AccountApi(principal)
      const health = await api.execute({ kind: 'health' })
      if (!principal.current() || health.kind !== 'ok') return { kind: 'unavailable' }
      const prepared = b64url(await api.prepareRegistrationKey())
      if (!principal.current()) return { kind: 'notSent' }
      const pending = { deviceKey: prepared, expectedDeviceId: null as string | null }
      pendingRegistrations.set(principal.namespace, pending)
      const response = await api.execute({ kind: 'registerDevice', name, platform })
      if (!principal.current() || response.kind !== 'ok') {
        if (response.kind === 'notSent' && pendingRegistrations.get(principal.namespace) === pending) pendingRegistrations.delete(principal.namespace)
        return { kind: response.kind === 'notSent' ? 'notSent' : 'unknown' }
      }
      const key = b64url(await api.captureDeviceKey())
      if (!principal.current()) return { kind: 'unknown' }
      const device = response.value.device as { deviceId?: unknown; deviceKey?: unknown } | undefined
      if (key !== prepared || !idValid(device?.deviceId) || device.deviceKey !== key) return { kind: 'unknown' }
      pending.expectedDeviceId = device.deviceId
      const binding = await account.saveDeviceBinding(principal.seal, device.deviceId, key)
      if (!binding || !principal.current() || !bindingMatches(principal.seal, binding)) return { kind: 'unknown' }
      registered = { seal: principal.seal, binding: Object.freeze({ ...binding }) }
      if (pendingRegistrations.get(principal.namespace) === pending) pendingRegistrations.delete(principal.namespace)
      return { kind: 'registered' }
    } catch { return { kind: 'unknown' } } finally { registering = false }
  }, async reconcileRegistration() {
    if (registering) return { kind: 'notSent' }
    const principal = capturePrincipal(account)
    if (!principal) return { kind: 'unavailable' }
    registering = true
    const hadPending = pendingRegistrations.has(principal.namespace)
    try {
      const existing = await deviceSigner(principal, 'read')
      if (!principal.current()) return { kind: 'unknown' }
      const key = b64url(existing.publicKey)
      const pending = pendingRegistrations.get(principal.namespace) ?? { deviceKey: key, expectedDeviceId: null }
      if (pending.deviceKey !== key) return { kind: 'unknown' }
      pendingRegistrations.set(principal.namespace, pending)
      const current = () => principal.current() && pendingRegistrations.get(principal.namespace) === pending
      const api = new AccountApi(principal)
      const health = await api.execute({ kind: 'health' })
      if (!current() || health.kind !== 'ok') return { kind: 'unknown' }
      const reply = await api.execute({ kind: 'listDevices' })
      if (!current() || reply.kind !== 'ok' || !Array.isArray(reply.value.devices)) return { kind: 'unknown' }
      const matches = reply.value.devices.filter((row: { deviceKey?: unknown }) => row?.deviceKey === key)
      const row = matches[0] as { deviceId?: unknown; revoked?: unknown } | undefined
      if (matches.length !== 1 || !idValid(row?.deviceId) || row.revoked !== false || pending.expectedDeviceId !== null && row.deviceId !== pending.expectedDeviceId) return { kind: 'unknown' }
      const confirmedKey = await deviceSigner(principal, 'read')
      if (!current() || b64url(confirmedKey.publicKey) !== key) return { kind: 'unknown' }
      pending.expectedDeviceId = row.deviceId
      const binding = await account.saveDeviceBinding(principal.seal, row.deviceId, key)
      if (!current() || !binding || !bindingMatches(principal.seal, binding) || binding.deviceKey !== key || binding.deviceId !== row.deviceId) return { kind: 'unknown' }
      registered = { seal: principal.seal, binding: Object.freeze({ ...binding }) }
      pendingRegistrations.delete(principal.namespace)
      return { kind: 'registered' }
    } catch (error) { return { kind: !hadPending && (error as NodeJS.ErrnoException).code === 'ENOENT' ? 'notSent' : 'unknown' } } finally { registering = false }
  } }
}

