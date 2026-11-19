import { createHash, randomBytes } from 'node:crypto'
import { constants, promises as fs } from 'node:fs'
import { join } from 'node:path'
import { app, safeStorage } from 'electron'
import type { DeviceBinding } from '../fork-account/service'
import { b64url, unb64url } from './crypto'
import { readPinBytes, securePinsAvailable } from './pins'
import type { Principal } from './principal'
import type { BootstrapClosure, BootstrapExpected, BootstrapEnvelope, BootstrapOriginals } from './bootstrap-codec'

/** O journal nunca prova posse: registra intenção imutável e incerteza para não repetir POST/prova. */
export type BootstrapPhase = 'prepared' | 'createUnknown' | 'requested' | 'openingUnknown' | 'challengeUnknown' | 'challenged' | 'proofUnknown' | 'receipt' | 'reconcileUnknown' | 'published' | 'pinUnknown' | 'complete' | 'closed'
export interface BootstrapJournalData {
  readonly expected: BootstrapExpected | null
  readonly originals: Partial<BootstrapOriginals>
  readonly activation: BootstrapEnvelope | null
  readonly sealedPin: string | null
  readonly pinHash: string | null
  readonly terminal?: BootstrapClosure
}
export interface BootstrapJournal {
  readonly version: 2
  readonly namespace: string
  readonly deploymentId: string
  readonly env: 'main' | 'dev'
  readonly issuer: string
  readonly manifestHash: string
  readonly ownerUserId: string
  readonly deviceId: string
  readonly deviceKey: string
  readonly bootstrapId: string
  readonly nonceApp: string
  readonly phase: BootstrapPhase
  readonly revision: number
  readonly data: BootstrapJournalData
}
const keys = ['version','namespace','deploymentId','env','issuer','manifestHash','ownerUserId','deviceId','deviceKey','bootstrapId','nonceApp','phase','revision','data']
const transitions: Record<BootstrapPhase, BootstrapPhase[]> = { prepared: ['createUnknown'], createUnknown: ['prepared', 'requested', 'receipt', 'published'], requested: ['requested', 'openingUnknown', 'receipt', 'published'], openingUnknown: ['challengeUnknown', 'receipt', 'published'], challengeUnknown: ['challenged', 'receipt', 'published'], challenged: ['proofUnknown', 'receipt', 'published'], proofUnknown: ['receipt', 'published'], receipt: ['receipt', 'reconcileUnknown', 'published'], reconcileUnknown: ['receipt', 'published'], published: ['published', 'pinUnknown'], pinUnknown: ['complete'], complete: [], closed: [] }
for (const phase of Object.keys(transitions) as BootstrapPhase[]) if (phase !== 'prepared' && phase !== 'pinUnknown' && phase !== 'complete' && phase !== 'closed') transitions[phase].push('closed')
const phases = Object.keys(transitions)
const locks = new Map<string, Promise<unknown>>()
const check = (p: Principal) => { if (!p.current()) throw new Error('Account session changed.') }
const legacyPath = (p: Principal) => join(app.getPath('userData'), `bootstrap-journal-${createHash('sha256').update(p.namespace).digest('hex')}.bin`)
const pathFor = (p: Principal, b: DeviceBinding) => join(app.getPath('userData'), `bootstrap-journal-device-${createHash('sha256').update(JSON.stringify([p.namespace,b.deviceId,b.deviceKey])).digest('hex')}.bin`)
const syncDirectory = async () => { const dir = await fs.open(app.getPath('userData'), 'r'); try { await dir.sync() } finally { await dir.close() } }
async function locked<T>(path: string, work: () => Promise<T>): Promise<T> {
  const previous = locks.get(path) ?? Promise.resolve()
  const run = previous.catch(() => {}).then(work)
  locks.set(path, run)
  try { return await run } finally { if (locks.get(path) === run) locks.delete(path) }
}
function sameBinding(p: Principal, b: DeviceBinding): boolean {
  return b.version === 1 && b.userId === p.seal.userId && b.sessionId === p.seal.sessionId && b.origin === p.deployment.origin && b.issuer === p.deployment.issuer && b.deploymentId === p.deployment.deploymentId && b.manifestHash === p.deployment.manifestHash && /^dev_[A-Za-z0-9_-]{1,60}$/.test(b.deviceId) && !!unb64url(b.deviceKey, 32)
}
function decode(p: Principal, binding: DeviceBinding, sealed: Buffer, historical = false): BootstrapJournal {
  if (!securePinsAvailable() || !sameBinding(p, binding)) throw new Error('Bootstrap storage unavailable.')
  const raw: unknown = JSON.parse(safeStorage.decryptString(sealed))
  if (!raw || typeof raw !== 'object' || Array.isArray(raw) || Object.keys(raw).sort().join(',') !== [...keys].sort().join(',')) throw new Error('Invalid bootstrap journal.')
  const j = raw as BootstrapJournal
  if (j.version !== 2 || j.namespace !== p.namespace || j.deploymentId !== p.deployment.deploymentId || j.env !== p.deployment.env || j.issuer !== p.deployment.issuer || j.manifestHash !== p.deployment.manifestHash || j.ownerUserId !== p.seal.userId || !/^dev_[A-Za-z0-9_-]{1,60}$/.test(j.deviceId) || !unb64url(j.deviceKey, 32) || !historical && (j.deviceId !== binding.deviceId || j.deviceKey !== binding.deviceKey) || !unb64url(j.bootstrapId, 16) || !unb64url(j.nonceApp, 32) || !phases.includes(j.phase) || !Number.isSafeInteger(j.revision) || j.revision < 0 || !j.data || Object.keys(j.data).sort().join(',') !== (j.phase === 'closed' ? 'activation,expected,originals,pinHash,sealedPin,terminal' : 'activation,expected,originals,pinHash,sealedPin') || !j.data.originals || typeof j.data.originals !== 'object' || Array.isArray(j.data.originals) || Object.keys(j.data.originals).some(k => !['grant','challenge','proof','receipt'].includes(k))) throw new Error('Different bootstrap journal.')
  if (j.phase === 'closed' && (!j.data.expected || !j.data.originals.receipt || !j.data.terminal)) throw new Error('Missing bootstrap closure.')
  return Object.freeze(j)
}
async function read(p: Principal, binding: DeviceBinding, path: string, historical = false): Promise<BootstrapJournal | null> {
  check(p)
  try { const data = await readPinBytes(path, 65_536); check(p); const result = decode(p, binding, data, historical); check(p); return result }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT' && p.current()) return null; throw error }
}
/** Keep the old filename and bytes for the exact old binding; a retired key cannot poison a new one. */
async function locate(p: Principal, binding: DeviceBinding): Promise<{ path: string; journal: BootstrapJournal | null }> {
  const path = pathFor(p, binding), current = await read(p, binding, path)
  if (current) return { path, journal:current }
  const legacy = legacyPath(p), old = await read(p, binding, legacy, true)
  return old?.deviceId === binding.deviceId && old.deviceKey === binding.deviceKey ? { path:legacy, journal:old } : { path, journal:null }
}
/** Carregar não abre canal e não inicia rede. */
export async function readBootstrapJournal(p: Principal, binding: DeviceBinding): Promise<BootstrapJournal | null> {
  if (!securePinsAvailable() || !sameBinding(p, binding)) throw new Error('Bootstrap storage unavailable.')
  return locked(legacyPath(p), async () => (await locate(p, binding)).journal)
}
/** Nova intenção explícita; uma referência existente nunca é substituída. */
export async function createBootstrapJournal(p: Principal, binding: DeviceBinding): Promise<BootstrapJournal> {
  if (!securePinsAvailable() || !sameBinding(p, binding)) throw new Error('Bootstrap storage unavailable.')
  return locked(legacyPath(p), async () => {
    check(p)
    const { path, journal } = await locate(p, binding)
    if (journal) throw new Error('Bootstrap already exists.')
    check(p)
    const value: BootstrapJournal = { version: 2, namespace: p.namespace, deploymentId: p.deployment.deploymentId, env: p.deployment.env, issuer: p.deployment.issuer, manifestHash: p.deployment.manifestHash, ownerUserId: p.seal.userId, deviceId: binding.deviceId, deviceKey: binding.deviceKey, bootstrapId: b64url(randomBytes(16)), nonceApp: b64url(randomBytes(32)), phase: 'prepared', revision: 0, data: { expected: null, originals: {}, activation: null, sealedPin: null, pinHash: null } }
    const sealed = safeStorage.encryptString(JSON.stringify(value))
    const file = await fs.open(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    try { check(p); await file.writeFile(sealed); await file.sync() } finally { await file.close() }
    check(p); await syncDirectory(); check(p)
    return Object.freeze(value)
  })
}
/** Avança apenas o mesmo journal e fase; chamador persiste Unknown ANTES de enviar. */
export async function advanceBootstrapJournal(p: Principal, binding: DeviceBinding, expected: BootstrapJournal, phase: BootstrapPhase, data: BootstrapJournalData = expected.data): Promise<BootstrapJournal> {
  return locked(legacyPath(p), async () => {
    const { path, journal:current } = await locate(p, binding)
    check(p)
    if (!current || JSON.stringify(current) !== JSON.stringify(expected) || !transitions[current.phase].includes(phase)) throw new Error('Different bootstrap operation.')
    // Once captured, an identity/original/pin cannot be exchanged by later network replies.
    const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
    if (current.data.expected && !equal(current.data.expected, data.expected) ||
        Object.entries(current.data.originals).some(([k, v]) => !equal(v, data.originals[k as keyof BootstrapOriginals])) ||
        current.data.terminal && !equal(current.data.terminal, data.terminal) ||
        current.data.sealedPin !== null && (data.sealedPin !== current.data.sealedPin || data.pinHash !== current.data.pinHash)) throw new Error('Bootstrap provenance changed.')
    const next: BootstrapJournal = JSON.parse(JSON.stringify({ ...current, phase, data, revision: current.revision + 1 })) as BootstrapJournal
    if (!Number.isSafeInteger(next.revision)) throw new Error('Bootstrap revision exhausted.')
    if (Buffer.byteLength(JSON.stringify(next), 'utf8') > 48_000) throw new Error('Bootstrap journal too large.')
    const sealed = safeStorage.encryptString(JSON.stringify(next))
    const tmp = `${path}.${randomBytes(16).toString('hex')}.tmp`
    const file = await fs.open(tmp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    try { check(p); await file.writeFile(sealed); await file.sync() } finally { await file.close() }
    check(p)
    await fs.rename(tmp, path)
    check(p); await syncDirectory(); check(p)
    return Object.freeze(next)
  })
}
