import { createHash } from 'node:crypto'
import { constants, promises as fs } from 'node:fs'
import { dirname } from 'node:path'
import { safeStorage } from 'electron'
import type { Principal } from './principal'
import { deviceSigner } from './device-keys'
import { b64url, unb64url, verifyEd25519 } from './crypto'
import { pinPath, readPinBytes, securePinsAvailable, type HubPin } from './pins'
import { bootstrapAuthority } from './bootstrap-authority'
import { verifyBootstrapActivation, verifyBootstrapChain, type BootstrapEnvelope, type BootstrapExpected, type BootstrapOriginals } from './bootstrap-codec'

const expectedKeys = ['bootstrapId','provisioningIntentId','resourceDigest','hubGeneration','issuerKeyId','issuer','deploymentId','env','manifestHash','ownerUserId','deviceId','deviceKey','nonceApp','computerId','signKey','boxKey','createdAt','expiresAt'] as const
interface Receipt extends BootstrapExpected {
  version: 3; source: 'ACS4'; namespace: string; deployment: Principal['deployment'];
  grantId: string; hashHubReceipt: string; originals: BootstrapOriginals; activation: BootstrapEnvelope; publishedAt: number
}
interface Stored { receipt: Receipt; signature: string }
const keys = ['version','source','namespace','deployment',...expectedKeys,'grantId','hashHubReceipt','originals','activation','publishedAt']
const check = (p: Principal) => { if (!p.current()) throw new Error('Bootstrap pin superseded.') }
const sum = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')
function message(r: Receipt): Buffer {
  const d = r.deployment
  const deployment = { deploymentId: d.deploymentId, env: d.env, issuer: d.issuer, origin: d.origin, manifestHash: d.manifestHash }
  const payload = Object.fromEntries(keys.map(k => [k, k === 'deployment' ? deployment : k === 'originals'
    ? { grant: r.originals.grant, challenge: r.originals.challenge, proof: r.originals.proof, receipt: r.originals.receipt }
    : r[k as keyof Receipt]]))
  return Buffer.from(JSON.stringify(['donna/bootstrap-pin/v3', payload]))
}
export async function verifyBootstrapPin(p: Principal, computerId: string, input: unknown): Promise<HubPin> {
  check(p)
  const root = bootstrapAuthority(p)
  if (!root || !input || typeof input !== 'object' || Array.isArray(input) || Object.keys(input).sort().join(',') !== 'receipt,signature') throw new Error('Bootstrap pin authority unavailable.')
  const { receipt: r, signature } = input as Stored
  if (!r || typeof r !== 'object' || Array.isArray(r) || Object.keys(r).length !== keys.length || !keys.every(k => Object.hasOwn(r, k)) ||
      r.version !== 3 || r.source !== 'ACS4' || r.namespace !== p.namespace || r.computerId !== computerId || r.ownerUserId !== p.seal.userId ||
      r.issuerKeyId !== root.issuerKeyId || !Number.isSafeInteger(r.publishedAt) || r.publishedAt <= 0 || r.publishedAt > Date.now() + 30_000 ||
      !r.deployment || Object.keys(r.deployment).sort().join(',') !== 'deploymentId,env,issuer,manifestHash,origin' ||
      (['deploymentId','env','issuer','manifestHash'] as const).some(k => r[k] !== p.deployment[k] || r.deployment[k] !== p.deployment[k]) ||
      r.deployment.origin !== p.deployment.origin) throw new Error('Different bootstrap pin.')
  const key = unb64url(r.deviceKey, 32), sig = unb64url(signature, 64)
  if (!key || !sig || !verifyEd25519(key, sig, message(r))) throw new Error('Invalid bootstrap pin signature.')
  const chain = verifyBootstrapChain(r.originals, r, root.issuerPublicKey, r.publishedAt)
  if (chain.grantId !== r.grantId || chain.hashReceipt !== r.hashHubReceipt) throw new Error('Different bootstrap pin receipt.')
  // Historical receipt validity is checked at publication. Current admission is
  // independently enforced by the Hub lease/ACL and the normal verified hello.
  verifyBootstrapActivation(r.activation, r.originals, r, root.issuerPublicKey, r.publishedAt)
  const signer = await deviceSigner(p, 'read'); check(p)
  if (b64url(signer.publicKey) !== r.deviceKey) throw new Error('Different bootstrap device.')
  return Object.freeze({ computerId, signKey: r.signKey, source: 'ACS4' })
}
/** Produces exact ciphertext once; caller durably journals it BEFORE filesystem publication. */
export async function prepareBootstrapPin(p: Principal, expected: BootstrapExpected, originals: BootstrapOriginals, activation: BootstrapEnvelope): Promise<{ sealedPin: string; pinHash: string }> {
  check(p)
  if (!securePinsAvailable()) throw new Error('Secure bootstrap storage unavailable.')
  const root = bootstrapAuthority(p), at = Date.now()
  if (!root || expected.issuerKeyId !== root.issuerKeyId) throw new Error('Bootstrap authority unavailable.')
  const chain = verifyBootstrapChain(originals, expected, root.issuerPublicKey, at)
  verifyBootstrapActivation(activation, originals, expected, root.issuerPublicKey, at)
  const receipt: Receipt = { version: 3, source: 'ACS4', namespace: p.namespace, deployment: { ...p.deployment }, ...expected, grantId: chain.grantId, hashHubReceipt: chain.hashReceipt, originals, activation, publishedAt: at }
  const signer = await deviceSigner(p, 'read'); check(p)
  const stored = { receipt, signature: b64url(signer.sign(message(receipt))) }
  await verifyBootstrapPin(p, expected.computerId, stored); check(p)
  const sealed = safeStorage.encryptString(JSON.stringify(stored))
  if (sealed.length > 16_384) throw new Error('Bootstrap pin too large.')
  return { sealedPin: sealed.toString('base64'), pinHash: sum(sealed) }
}
/** Idempotent local sync of retained exact bytes, including the post-unlink window. */
export async function publishBootstrapPin(p: Principal, computerId: string, sealedPin: string, pinHash: string): Promise<void> {
  check(p)
  const bytes = Buffer.from(sealedPin, 'base64')
  if (!securePinsAvailable() || bytes.length === 0 || bytes.length > 16_384 || bytes.toString('base64') !== sealedPin || sum(bytes) !== pinHash) throw new Error('Different bootstrap publication.')
  await verifyBootstrapPin(p, computerId, JSON.parse(safeStorage.decryptString(bytes))); check(p)
  const path = pinPath(p, computerId), pending = `${path}.pending`
  // Never recreate a removed pending marker when the same final pin already exists.
  let finalExists = false
  try { finalExists = (await readPinBytes(path)).equals(bytes); if (!finalExists) throw new Error('Different existing pin.') }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
  check(p)
  if (!finalExists) {
    let file
    try { file = await fs.open(pending, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600) }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
    if (file) { try { check(p); await file.writeFile(bytes); await file.sync() } finally { await file.close() } }
    check(p)
    if (!(await readPinBytes(pending)).equals(bytes)) throw new Error('Different pending pin.')
    check(p)
    try { await fs.link(pending, path) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
  }
  check(p)
  if (!(await readPinBytes(path)).equals(bytes)) throw new Error('Different stored pin.')
  check(p)
  const file = await fs.open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try { await file.sync(); check(p) } finally { await file.close() }
  const dir = await fs.open(dirname(path), 'r')
  try {
    await dir.sync(); check(p)
    let remaining: Buffer | null = null
    try { remaining = await readPinBytes(pending) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
    check(p)
    if (remaining) { if (!remaining.equals(bytes)) throw new Error('Different pending publication.'); await fs.unlink(pending) }
    await dir.sync(); check(p)
    if (!(await readPinBytes(path)).equals(bytes)) throw new Error('Stored pin changed.')
    check(p)
  } finally { await dir.close() }
}
