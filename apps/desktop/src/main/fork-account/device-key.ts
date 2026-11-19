import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, randomBytes, type KeyObject } from 'node:crypto'
import { promises as fs } from 'node:fs'
import { join } from 'node:path'
import { app, safeStorage } from 'electron'
import type { DeviceBinding } from './service'

export interface DeviceIdentity { key: KeyObject; publicKey: string }
export type DeviceScope = Pick<DeviceBinding, 'origin' | 'issuer' | 'deploymentId' | 'manifestHash'>
type Current = () => boolean
type Marker = { version: 1; binding: DeviceBinding; phase: 'retired' | 'prepared' | 'active'; seed?: string; publicKey?: string }
const locks = new Map<string, Promise<unknown>>()
const b64 = (value: Buffer) => value.toString('base64url')
const check = (current: Current) => { if (!current()) throw new Error('Session superseded.') }
const available = () => safeStorage.isEncryptionAvailable() && !(process.platform === 'linux' && safeStorage.getSelectedStorageBackend() === 'basic_text')
const directory = () => app.getPath('userData')
const pathFor = (userId: string) => join(directory(), `device-key-${createHash('sha256').update(userId).digest('hex').slice(0, 16)}.bin`)
const markerFor = (userId: string) => `${pathFor(userId)}.generation`

async function locked<T>(userId: string, action: () => Promise<T>): Promise<T> {
  const prior = locks.get(userId) ?? Promise.resolve()
  const next = prior.catch(() => {}).then(action)
  locks.set(userId, next)
  try { return await next } finally { if (locks.get(userId) === next) locks.delete(userId) }
}
async function syncPath(path: string) {
  const file = await fs.open(path, 'r')
  try { await file.sync() } finally { await file.close() }
  const dir = await fs.open(directory(), 'r')
  try { await dir.sync() } finally { await dir.close() }
}
async function publish(path: string, raw: Buffer, current: Current, replace: boolean) {
  await fs.mkdir(directory(), { recursive: true }); check(current)
  const tmp = `${path}.${randomBytes(16).toString('hex')}.tmp`
  const file = await fs.open(tmp, 'wx', 0o600)
  try {
    try { await file.writeFile(raw); await file.sync() } finally { await file.close() }
    check(current)
    if (replace) await fs.rename(tmp, path)
    else await fs.link(tmp, path)
    await syncPath(path); check(current)
  } finally { await fs.rm(tmp, { force: true }).catch(() => {}) }
}
function fromSeed(encoded: string): DeviceIdentity {
  const seed = Buffer.from(encoded, 'base64url')
  if (seed.length !== 32 || b64(seed) !== encoded) throw new Error('Invalid device key.')
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed])
  const privateKey = createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })
  return { key: privateKey, publicKey: (createPublicKey(privateKey).export({ format: 'jwk' }) as { x: string }).x }
}
function bindingValid(binding: DeviceBinding, userId: string) {
  return binding?.version === 1 && binding.userId === userId && typeof binding.sessionId === 'string' && !!binding.sessionId &&
    /^dev_[A-Za-z0-9_-]+$/.test(binding.deviceId) && /^[A-Za-z0-9_-]{43}$/.test(binding.deviceKey) &&
    [binding.origin, binding.issuer, binding.deploymentId, binding.manifestHash].every((v) => typeof v === 'string' && !!v)
}
function sameBinding(a: DeviceBinding, b: DeviceBinding) {
  return (['version', 'userId', 'sessionId', 'origin', 'issuer', 'deploymentId', 'manifestHash', 'deviceId', 'deviceKey'] as const).every((key) => a[key] === b[key])
}
async function marker(userId: string, scope: DeviceScope): Promise<Marker | null> {
  let raw: Buffer
  try { raw = await fs.readFile(markerFor(userId)) } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return null
    throw error
  }
  const value = JSON.parse(safeStorage.decryptString(raw)) as Marker
  if (value.version !== 1 || !bindingValid(value.binding, userId) || !['retired', 'prepared', 'active'].includes(value.phase)) throw new Error('Invalid generation marker.')
  if (!scope || (['origin', 'issuer', 'deploymentId', 'manifestHash'] as const).some((key) => value.binding[key] !== scope[key])) throw new Error('Different device configuration.')
  if (value.phase !== 'retired') {
    if (typeof value.seed !== 'string' || typeof value.publicKey !== 'string' || fromSeed(value.seed).publicKey !== value.publicKey || value.publicKey === value.binding.deviceKey) throw new Error('Invalid generation.')
  } else if (value.seed !== undefined || value.publicKey !== undefined) throw new Error('Invalid retired generation.')
  return value
}
async function writeMarker(userId: string, value: Marker, current: Current, replace: boolean) {
  await publish(markerFor(userId), safeStorage.encryptString(JSON.stringify(value)), current, replace)
}
type Anchor = { version: 1; kind: 'retired'; binding: DeviceBinding }
async function anchor(userId: string): Promise<DeviceIdentity | Anchor> {
  const plaintext = safeStorage.decryptString(await fs.readFile(pathFor(userId)))
  if (plaintext.startsWith('{')) {
    const value = JSON.parse(plaintext) as Anchor
    if (value.version !== 1 || value.kind !== 'retired' || !bindingValid(value.binding, userId)) throw new Error('Invalid identity anchor.')
    return value
  }
  return fromSeed(plaintext)
}
function isAnchor(value: DeviceIdentity | Anchor): value is Anchor { return 'kind' in value }


/** All operations are serialized per account; verification never creates or signs a key. */
export async function deviceIdentity(userId: string, current: Current, mode: 'read' | 'sign' | 'register', scope: DeviceScope): Promise<DeviceIdentity> {
  return locked(userId, async () => {
    check(current); if (!available()) throw new Error('Secure storage unavailable.')
    let state = await marker(userId, scope); check(current)
    if (state) {
      // The original file is kept as the installation's durable anchor.
      const root = await anchor(userId); if (!isAnchor(root)) throw new Error('Uncommitted retirement anchor.'); check(current)
      if (state.phase === 'retired') {
        if (mode !== 'register') throw new Error('Device identity retired.')
        const generated = generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' }) as { d: string; x: string }
        if (generated.x === state.binding.deviceKey) throw new Error('Repeated device key.')
        state = { ...state, phase: 'prepared', seed: generated.d, publicKey: generated.x }
        await writeMarker(userId, state, current, true)
      }
      if (state.phase === 'prepared') {
        if (mode !== 'register') throw new Error('Device generation uncommitted.')
        await syncPath(markerFor(userId)); check(current)
        state = { ...state, phase: 'active' }
        await writeMarker(userId, state, current, true)
      }
      await syncPath(markerFor(userId)); check(current)
      return fromSeed(state.seed!)
    }
    let found: DeviceIdentity
    try {
      const root = await anchor(userId)
      if (isAnchor(root)) throw new Error('Retirement marker unavailable.')
      found = root
    } catch (error) {
      if (mode !== 'register' || (error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      check(current)
      const generated = generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' }) as { d: string }
      await publish(pathFor(userId), safeStorage.encryptString(generated.d), current, false)
      found = fromSeed(generated.d)
    }
    if (mode !== 'read') { await syncPath(pathFor(userId)); check(current) }
    return found
  })
}

/** D185 local sign-out: discard this account/configuration's signing identity.
 * The caller's epoch barrier excludes a later login; the same per-identity lock
 * excludes concurrent publication. No keychain/decryption or remote proof is needed. */
export async function discardDeviceIdentity(userId: string, current: Current): Promise<void> {
  return locked(userId, async () => {
    check(current)
    await fs.rm(pathFor(userId), { force: true }); check(current)
    await fs.rm(markerFor(userId), { force: true }); check(current)
    try {
      const dir = await fs.open(directory(), 'r')
      try { await dir.sync() } finally { await dir.close() }
    } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
    check(current)
  })
}

/** The marker is an encrypted record of this exact proven retirement, never a new vault. */
export async function retireDeviceIdentity(binding: DeviceBinding, current: Current): Promise<void> {
  return locked(binding.userId, async () => {
    check(current); if (!available() || !bindingValid(binding, binding.userId)) throw new Error('Invalid retirement.')
    const state = await marker(binding.userId, binding); check(current)
    const root = await anchor(binding.userId); check(current)
    if (state?.phase === 'retired' && sameBinding(state.binding, binding)) {
      if (!isAnchor(root)) throw new Error('Retirement anchor unavailable.')
      await syncPath(pathFor(binding.userId)); await syncPath(markerFor(binding.userId)); check(current); return
    }
    if (!state && isAnchor(root)) {
      if (!sameBinding(root.binding, binding)) throw new Error('Different retirement anchor.')
      await syncPath(pathFor(binding.userId)); check(current)
      await writeMarker(binding.userId, { version: 1, binding, phase: 'retired' }, current, false)
      return
    }
    const existing = state ? (state.phase === 'active' ? fromSeed(state.seed!) : null) : isAnchor(root) ? null : root
    if (!existing || existing.publicKey !== binding.deviceKey) throw new Error('Different device generation.')
    if (!isAnchor(root)) {
      const sealed = safeStorage.encryptString(JSON.stringify({ version: 1, kind: 'retired', binding } satisfies Anchor))
      await publish(pathFor(binding.userId), sealed, current, true)
    }
    check(current)
    await writeMarker(binding.userId, { version: 1, binding, phase: 'retired' }, current, !!state)
  })
}

export async function verifyDeviceGeneration(userId: string, publicKey: string, scope: DeviceScope): Promise<boolean> {
  return locked(userId, async () => {
    if (!available()) return false
    try {
      const state = await marker(userId, scope)
      if (state) {
        if (!isAnchor(await anchor(userId))) return false
        if (state.binding.deviceKey === publicKey) return true
        return state.phase === 'active' && state.publicKey === publicKey
      }
      const root = await anchor(userId)
      return isAnchor(root) ? root.binding.deviceKey === publicKey && (['origin', 'issuer', 'deploymentId', 'manifestHash'] as const).every((key) => root.binding[key] === scope[key]) : root.publicKey === publicKey
    } catch { return false }
  })
}
