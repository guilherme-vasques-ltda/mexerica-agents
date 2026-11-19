import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import * as wire from '../../main/fork-cloud/crypto.ts'
import { loadMainModule } from './main-module.ts'

export type FixtureReply = { kind: 'ok'; value: Record<string, unknown> } | { kind: 'unknown' | 'notSent' | 'unavailable' } | { kind: 'refused'; status: number; code: string }
class FixtureLocalFailure extends Error {
  readonly kind: 'notSent' | 'unknown'
  constructor(kind: 'notSent' | 'unknown') { super(kind); this.kind = kind }
}

export async function fixture() {
  const dir = await fs.promises.mkdtemp(path.join(tmpdir(), 'donna-pin-ceremony-'))
  const hostSigner = wire.signerFromSeed(new Uint8Array(32).fill(41))
  const host = hostSigner.publicKey
  const signer = wire.signerFromSeed(new Uint8Array(32).fill(42))
  const cid = wire.computerIdOf(host)
  const nh = wire.b64url(new Uint8Array(32).fill(43))
  const deadline = Date.now() + 60_000
  let live = true, approved = false, decisions = 0, revealed: string | null = null, writes = 0, failUnlink = false
  const principal = { seal: { epoch: 1, userId: 'fixture-user', sessionId: 'fixture-session' }, namespace: 'fixture-namespace', deployment: { deploymentId: 'fixture-deployment', env: 'dev' as const, origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', manifestHash: 'a'.repeat(64) }, current: () => live, token: async () => 'fixture-not-a-token' }
  // Test-only cipher: bytes exercise real publication/durability, not OS Keychain.
  const electron = { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, getSelectedStorageBackend: () => 'keychain', encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } }
  let afterUnlink: (() => Promise<void>) | null = null
  const fixtureFS = { ...fs, promises: { ...fs.promises, unlink: async (file: string) => {
    if (failUnlink) { failUnlink = false; throw new Error('Injected publication failure') }
    await fs.promises.unlink(file)
    if (file.endsWith('.pending') && afterUnlink) { const callback = afterUnlink; afterUnlink = null; await callback() }
  } } }
  const pins = loadMainModule<typeof import('../../main/fork-cloud/pins.ts')>('fork-cloud/pins', {
    'node:crypto': crypto, 'node:fs': fixtureFS, 'node:path': path, electron, './crypto': wire,
    './device-keys': { deviceSigner: async () => signer },
  }, { Buffer, process })
  let owner: string | null = principal.seal.userId
  const replies = new Map<string, () => Promise<FixtureReply>>()
  const apiCalls: string[] = []
  let localFault: { kind: 'notSent' | 'unknown'; intent: string } | null = null
  const api = class {
    async captureDeviceKey() { return signer.publicKey }
    async execute(intent: { kind: string; nonce?: string }): Promise<FixtureReply> {
      apiCalls.push(intent.kind)
      const next = replies.get(intent.kind)
      if (next) { replies.delete(intent.kind); return next() }
      if (intent.kind === 'health') return { kind: 'ok', value: {} }
      if (intent.kind === 'createClaim') return { kind: 'ok', value: { claimId: 'clm_fixture', nonce: wire.b64url(new Uint8Array(16).fill(46)), expiresAt: deadline } }
      if (intent.kind === 'completeClaim') return { kind: 'ok', value: {} }
      if (intent.kind === 'requestAccess') { writes++; return { kind: 'ok', value: { requestId: 'req_fixture', expiresAt: deadline } } }
      if (intent.kind === 'listComputers') return { kind: 'ok', value: { computers: [{ computerId: cid, signKey: wire.b64url(host), name: 'fixture Hub' }] } }
      if (intent.kind === 'getAccess') return { kind: 'ok', value: { requestId: 'req_fixture', computerId: cid, hostNonce: nh, expiresAt: deadline, status: approved ? 'approved' : 'pending', ...(approved ? { grantId: 'grt_fixture' } : {}) } }
      if (intent.kind === 'revealAccess') { revealed = intent.nonce!; return { kind: 'ok', value: {} } }
      throw new Error('Unexpected fixture intent')
    }
  }
  const mod = loadMainModule<typeof import('../../main/fork-cloud/enroll.ts')>('fork-cloud/enroll', {
    'node:crypto': crypto, 'node:fs': fixtureFS, 'node:path': path, electron,
    './crypto': wire, './device-keys': { deviceSigner: async () => signer }, './account-api': { AccountApi: api }, './pins': pins,
  }, { Buffer, process })
  const local = { profileId: 'fixture', computerId: cid, signKey: wire.b64url(host), current: () => live, wasNotSent: (error: unknown) => error instanceof FixtureLocalFailure && error.kind === 'notSent',
    async execute(intent: { kind: string; claimId?: string; nonce?: string; userId?: string; expect?: { code: string; transcript: string; deviceKey: string } }) {
      if (localFault?.intent === intent.kind) { const fault = localFault; localFault = null; if (fault.kind === 'unknown' && intent.kind === 'decideAccessRequest') { decisions++; approved = true }; throw new FixtureLocalFailure(fault.kind) }
      if (intent.kind === 'hello') return { computerId: cid, signKey: wire.b64url(host) }
      if (intent.kind === 'cloudStatus') return { enabled: true, url: principal.deployment.origin, owner: owner === null ? null : { userId: owner } }
      if (intent.kind === 'claimSign') {
        const boxKey = wire.b64url(new Uint8Array(32).fill(45))
        const bytes = new TextEncoder().encode(['codync/claim/v1', intent.claimId, intent.nonce, intent.userId, cid, boxKey].join('\n'))
        return { computerId: cid, signKey: wire.b64url(host), boxKey, sig: wire.b64url(hostSigner.sign(bytes)), name: 'fixture Hub', platform: 'linux', version: 'fixture' }
      }
      if (intent.kind === 'accessRequests') return { requests: [{ requestId: 'req_fixture', deviceKey: wire.b64url(signer.publicKey), code: wire.sasCode(host, signer.publicKey, wire.unb64url(revealed!, 32)!, wire.unb64url(nh, 32)!), transcript: wire.b64url(new Uint8Array(32).fill(44)), deadline, expiresAt: deadline }] }
      if (intent.kind === 'decideAccessRequest') { decisions++; approved = true; return { result: 'approved', deviceKey: intent.expect!.deviceKey, grantId: 'grt_fixture' } }
      throw new Error('Unexpected Local fixture intent')
    } }
  return { dir, host, signer, cid, principal, pins, mod, local, api,
    replyOnce: (kind: string, next: () => Promise<FixtureReply>) => replies.set(kind, next),
    apiCalls: (kind: string) => apiCalls.filter(x => x === kind).length,
    afterPublicationUnlink: (callback: () => Promise<void>) => { afterUnlink = callback },
    setOwner: (value: string | null) => { owner = value },
    localFailure: (intent: string, kind: 'notSent' | 'unknown') => { localFault = { intent, kind } }, approve: () => { approved = true }, failPublication: () => { failUnlink = true }, stop: () => { live = false }, decisions: () => decisions, writes: () => writes,
    clear: () => fs.promises.rm(dir, { recursive: true, force: true }) }
}

