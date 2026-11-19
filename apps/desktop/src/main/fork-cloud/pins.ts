import type { Deployment } from './principal'

/** A health response only compares the independently configured public summary. */
export function matchesDeployment(expected: Deployment, value: unknown): boolean {
  if (!value || typeof value !== 'object') return false
  const response = value as { ok?: unknown; deployment?: unknown }
  if (response.ok !== true || !response.deployment || typeof response.deployment !== 'object') return false
  const actual = response.deployment as Record<string, unknown>
  return actual.deploymentId === expected.deploymentId && actual.env === expected.env && actual.issuer === expected.issuer && actual.origin === expected.origin && actual.manifestHash === expected.manifestHash
}

import { createHash } from 'node:crypto'
import { constants, promises as fs } from 'node:fs'
import { dirname, join } from 'node:path'
import { app, safeStorage } from 'electron'
import { b64url, computerIdOf, sasCode, sasCommit, unb64url, verifyEd25519 } from './crypto'
import { deviceSigner } from './device-keys'
import type { Principal } from './principal'

export interface HubPin { readonly computerId: string; readonly signKey: string; readonly source: 'A1' | 'A2' | 'ACS4' }
/** Historical receipt issued only by Enrollment after its closed ceremony. It is
 * not a cloud grant and never authorizes the Hub handshake by itself. */
export interface PinReceipt extends HubPin {
  readonly source: 'A1' | 'A2'
  readonly version: 2
  readonly namespace: string
  readonly deviceKey: string
  readonly requestId: string
  readonly grantId: string
  readonly attemptId: string
  readonly deviceNonce: string
  readonly hostNonce: string
  readonly commit: string
  readonly code: string
  readonly deadline: number
  readonly confirmedAt: number
  readonly localTranscript: string | null
}
export interface PublicationIdentity { readonly attemptId: string; readonly sha256: string }
export interface StoredPin { readonly receipt: PinReceipt; readonly signature: string }
export const pinPath = (principal: Principal, computerId: string) => join(app.getPath('userData'), `hub-pin-${createHash('sha256').update(JSON.stringify([principal.namespace, computerId])).digest('hex')}.bin`)
export function pinMessage(receipt: PinReceipt): Uint8Array {
  // Fixed order, own domain: never reuse a signature made for HTTP or a relay.
  return Buffer.from(JSON.stringify(['donna/pin-receipt/v2', receipt.version, receipt.namespace, receipt.computerId, receipt.signKey, receipt.source, receipt.deviceKey, receipt.requestId, receipt.grantId, receipt.attemptId, receipt.deviceNonce, receipt.hostNonce, receipt.commit, receipt.code, receipt.deadline, receipt.confirmedAt, receipt.localTranscript]))
}
export function securePinsAvailable(): boolean {
  return safeStorage.isEncryptionAvailable() && !(process.platform === 'linux' && safeStorage.getSelectedStorageBackend() === 'basic_text')
}
function check(principal: Principal) { if (!principal.current()) throw new Error('Account session changed.') }
export async function verifyPin(principal: Principal, computerId: string, value: StoredPin): Promise<HubPin> {
  check(principal)
  const r = value?.receipt
  const host = unb64url(r?.signKey, 32), device = unb64url(r?.deviceKey, 32)
  const nd = unb64url(r?.deviceNonce, 32), nh = unb64url(r?.hostNonce, 32), sig = unb64url(value?.signature, 64)
  if (!r || r.version !== 2 || r.namespace !== principal.namespace || r.computerId !== computerId || !host || !device || !nd || !nh || !sig || computerIdOf(host) !== computerId ||
    (r.source !== 'A1' && r.source !== 'A2') || !/^req_[A-Za-z0-9_-]{1,60}$/.test(r.requestId) || !/^grt_[A-Za-z0-9_-]{1,60}$/.test(r.grantId) || !unb64url(r.attemptId, 16) ||
    r.commit !== b64url(sasCommit(device, nd)) || r.code !== sasCode(host, device, nd, nh) || !Number.isSafeInteger(r.confirmedAt) || !Number.isSafeInteger(r.deadline) || r.confirmedAt <= 0 || r.confirmedAt > r.deadline ||
    (r.source === 'A1' ? !unb64url(r.localTranscript, 32) : r.localTranscript !== null) || !verifyEd25519(device, sig, pinMessage(r))) throw new Error('Invalid stored pin provenance.')
  const signer = await deviceSigner(principal, 'read')
  check(principal)
  if (b64url(signer.publicKey) !== r.deviceKey) throw new Error('Different device generation.')
  return Object.freeze({ computerId, signKey: r.signKey, source: r.source })
}
/** O_NOFOLLOW and handle metadata keep a symbolic link out of the pin store.
 * This does not defend against the same OS account controlling the whole app. */
export async function readPinBytes(path: string, limit = 16_384): Promise<Buffer> {
  const handle = await fs.open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const st = await handle.stat()
    if (!st.isFile() || st.size > limit || (process.getuid && (st.uid !== process.getuid() || (st.mode & 0o077) !== 0))) throw new Error('Unsafe pin storage.')
    return await handle.readFile()
  } finally { await handle.close() }
}
/** Legacy labels are not promoted. A pending publication must be reconciled
 * explicitly, never treated as committed by a reconnect or a restart. */
export async function readPin(principal: Principal, computerId: string): Promise<HubPin | null> {
  check(principal)
  if (!unb64url(computerId, 16) || !securePinsAvailable()) throw new Error('Secure pin storage unavailable.')
  const path = pinPath(principal, computerId)
  try { await fs.lstat(`${path}.pending`); throw Object.assign(new Error('Pin publication unconfirmed.'), { code: 'PIN_PUBLICATION_PENDING' }) }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
  check(principal)
  let data: Buffer
  try { data = await readPinBytes(path) }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT' && principal.current()) return null; throw error }
  check(principal)
  const stored = JSON.parse(safeStorage.decryptString(data)) as StoredPin
  const pin = (stored?.receipt as { version?: unknown })?.version === 3
    ? await (await import('./bootstrap-pin')).verifyBootstrapPin(principal, computerId, stored)
    : await verifyPin(principal, computerId, stored)
  check(principal)
  try { await fs.lstat(`${path}.pending`); throw Object.assign(new Error('Pin publication unconfirmed.'), { code: 'PIN_PUBLICATION_PENDING' }) }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
  check(principal)
  return pin
}

/** Explicit reconciliation of the same fully written, signed pending receipt.
 * Never repeats claim/approval or chooses a replacement pin. A partial pending
 * file or a different existing pin stays quarantined for local recovery. */
export async function reconcilePin(principal: Principal, computerId: string, expected?: PublicationIdentity, captured?: (identity: PublicationIdentity) => void): Promise<(HubPin & { reconciled: boolean }) | null> {
  check(principal)
  if (!unb64url(computerId, 16) || !securePinsAvailable()) throw new Error('Secure pin storage unavailable.')
  const path = pinPath(principal, computerId), pending = `${path}.pending`
  if (expected && (!unb64url(expected.attemptId, 16) || !/^[a-f0-9]{64}$/.test(expected.sha256))) throw new Error('Invalid retained publication identity.')
  const qualify = async (data: Buffer): Promise<PublicationIdentity> => {
    const value = JSON.parse(safeStorage.decryptString(data)) as StoredPin
    await verifyPin(principal, computerId, value); check(principal)
    const identity = { attemptId: value.receipt.attemptId, sha256: createHash('sha256').update(data).digest('hex') }
    if (expected && (identity.attemptId !== expected.attemptId || identity.sha256 !== expected.sha256)) throw new Error('Different retained publication.')
    return identity
  }
  let data: Buffer
  try { data = await readPinBytes(pending) }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
    if (!expected) { const pin = await readPin(principal, computerId); return pin ? { ...pin, reconciled: false } : null }
    // The previous operation may have removed pending before cancellation/fsync.
    // Only its retained exact bytes and receipt identity can finish the sync.
    data = await readPinBytes(path); check(principal)
    await qualify(data); check(principal)
    const file = await fs.open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
    try { await file.sync(); check(principal) } finally { await file.close() }
    const dir = await fs.open(dirname(path), 'r')
    try { await dir.sync(); check(principal) } finally { await dir.close() }
    const pin = await readPin(principal, computerId); check(principal)
    if (!(await readPinBytes(path)).equals(data)) throw new Error('Stored publication changed.')
    check(principal)
    return pin ? { ...pin, reconciled: true } : null
  }
  check(principal)
  const identity = await qualify(data); check(principal)
  // Capture provenance before the first filesystem change, including unlink.
  captured?.(identity); check(principal)
  const file = await fs.open(pending, constants.O_RDONLY | constants.O_NOFOLLOW)
  try { await file.sync() } finally { await file.close() }
  check(principal)
  if (!(await readPinBytes(pending)).equals(data)) throw new Error('Pending pin changed.')
  check(principal)
  try { await fs.link(pending, path) }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
  check(principal)
  if (!(await readPinBytes(path)).equals(data)) throw new Error('Different stored pin.')
  check(principal)
  const dir = await fs.open(dirname(path), 'r')
  try {
    await dir.sync(); check(principal)
    await fs.unlink(pending)
    await dir.sync(); check(principal)
  } finally { await dir.close() }
  const pin = await readPin(principal, computerId)
  return pin ? { ...pin, reconciled: true } : null
}
