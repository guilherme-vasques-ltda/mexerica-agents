import { createHash } from 'node:crypto'
import { computerIdOf, unb64url, verifyEd25519 } from './crypto'

/** Verificação criptográfica de wire3; não determina origem da raiz nem publica posse. */
export interface BootstrapExpected {
  readonly bootstrapId: string
  readonly provisioningIntentId: string
  readonly resourceDigest: string
  readonly hubGeneration: string
  readonly issuerKeyId: string
  readonly issuer: string
  readonly deploymentId: string
  readonly env: 'main' | 'dev'
  readonly manifestHash: string
  readonly ownerUserId: string
  readonly deviceId: string
  readonly deviceKey: string
  readonly nonceApp: string
  readonly computerId: string
  readonly signKey: string
  readonly boxKey: string
  readonly createdAt: number
  readonly expiresAt: number
}
export type BootstrapEnvelope = readonly [readonly unknown[], string]
export interface BootstrapOriginals { grant: BootstrapEnvelope; challenge: BootstrapEnvelope; proof: BootstrapEnvelope; receipt: BootstrapEnvelope }
export interface VerifiedGrant { readonly hash: string; readonly payload: readonly unknown[]; readonly original: BootstrapEnvelope }
export interface VerifiedChain { readonly grant: VerifiedGrant; readonly hashChallenge: string; readonly hashProof: string; readonly hashReceipt: string; readonly grantId: string; readonly committedAt: number }
/** Historical closure releases other Hubs; it never grants this Hub or erases its originals. */
export interface BootstrapClosure {
  readonly reason: 'canceled' | 'expired'
  readonly revision: number
  readonly observedAt: number
  readonly grantId: string
  readonly hashHubReceipt: string
  readonly revocation: BootstrapEnvelope | null
}
const digest = (value: unknown) => createHash('sha256').update(JSON.stringify(value)).digest('hex')
const hash = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value)
const identifier = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9_./:-]{1,256}$/.test(v)
const bootstrapId = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(v)
const safe = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const caps = (v: unknown) => Array.isArray(v) && v.length === 2 && v[0] === 'control' && v[1] === 'owner.devices'
const grantId = (v: unknown): v is string => typeof v === 'string' && v.startsWith('grt_') && !!unb64url(v.slice(4), 16)
const bytes = (v: unknown, n: number) => !!unb64url(v, n)

function expectedValid(e: BootstrapExpected): boolean {
  const host = unb64url(e.signKey, 32)
  return bootstrapId(e.bootstrapId) && identifier(e.provisioningIntentId) && hash(e.resourceDigest) && bytes(e.hubGeneration, 32) && identifier(e.issuerKeyId) && identifier(e.issuer) && identifier(e.deploymentId) && (e.env === 'main' || e.env === 'dev') && hash(e.manifestHash) && identifier(e.ownerUserId) && identifier(e.deviceId) && bytes(e.deviceKey, 32) && bytes(e.nonceApp, 32) && bytes(e.computerId, 16) && !!host && computerIdOf(host) === e.computerId && bytes(e.boxKey, 32) && safe(e.createdAt) && safe(e.expiresAt) && e.expiresAt > e.createdAt
}
function signed(value: unknown, domain: string, length: number, pub: unknown): { payload: readonly unknown[]; hash: string; original: BootstrapEnvelope } {
  if (!Array.isArray(value) || value.length !== 2 || !Array.isArray(value[0]) || value[0].length !== length || value[0][0] !== domain || typeof value[1] !== 'string') throw new Error('Invalid bootstrap envelope.')
  const key = unb64url(pub, 32), signature = unb64url(value[1], 64)
  const serialized = JSON.stringify(value)
  if (!key || !signature || Buffer.byteLength(serialized, 'utf8') > 16_384 || !verifyEd25519(key, signature, Buffer.from(JSON.stringify(value[0])))) throw new Error('Invalid bootstrap signature.')
  return { payload: value[0], hash: digest(value[0]), original: [value[0], value[1]] }
}
/** rootKey precisa vir do manifesto admitido, nunca de resposta da rede. */
export function verifyBootstrapGrant(value: unknown, e: BootstrapExpected, rootKey: string, now: number, committedAt?: number): VerifiedGrant {
  if (!expectedValid(e) || !safe(now)) throw new Error('Invalid bootstrap context.')
  const grant = signed(value, 'donna/bootstrap-grant/v1', 21, rootKey), g = grant.payload
  if (g[1] !== e.bootstrapId || g[2] !== e.provisioningIntentId || g[3] !== e.resourceDigest || g[4] !== e.hubGeneration || g[5] !== e.issuerKeyId || g[6] !== e.issuer || g[7] !== e.deploymentId || g[8] !== e.env || g[9] !== e.manifestHash || g[10] !== e.ownerUserId || g[11] !== e.deviceId || g[12] !== e.deviceKey || g[13] !== e.nonceApp || g[14] !== e.computerId || g[15] !== e.signKey || g[16] !== e.boxKey || !bytes(g[17], 32) || !safe(g[18]) || !safe(g[19]) || g[19] <= g[18] || g[19] - g[18] > 300_000 || g[19] > e.expiresAt || g[18] + 30_000 < e.createdAt || !caps(g[20])) throw new Error('Different bootstrap grant.')
  const at = committedAt ?? now
  if (!safe(at) || g[18] > at + 30_000 || at >= g[19] || at >= e.expiresAt) throw new Error('Expired bootstrap grant.')
  return grant
}
export function verifyBootstrapChallenge(value: unknown, e: BootstrapExpected, grant: VerifiedGrant, now: number, committedAt?: number) {
  const challenge = signed(value, 'donna/bootstrap-challenge/v1', 9, e.signKey), c = challenge.payload
  if (c[1] !== e.bootstrapId || c[2] !== e.hubGeneration || c[3] !== grant.hash || c[4] !== e.deviceKey || c[5] !== e.nonceApp || !bytes(c[6], 32) || !safe(c[7]) || !safe(c[8]) || c[8] <= c[7] || c[8] - c[7] > 60_000 || c[8] > (grant.payload[19] as number) || c[7] + 30_000 < (grant.payload[18] as number)) throw new Error('Different bootstrap challenge.')
  const at = committedAt ?? now
  if (!safe(at) || c[7] > at + 30_000 || at >= c[8]) throw new Error('Expired bootstrap challenge.')
  return challenge
}
/** Verify all four original envelopes, including the pending row hash. */
export function verifyBootstrapChain(originals: BootstrapOriginals, e: BootstrapExpected, rootKey: string, now: number): VerifiedChain {
  if (!originals || typeof originals !== 'object' || Array.isArray(originals) || Object.keys(originals).sort().join(',') !== 'challenge,grant,proof,receipt') throw new Error('Invalid bootstrap originals.')
  const receipt = signed(originals.receipt, 'donna/bootstrap-receipt/v1', 18, e.signKey), r = receipt.payload
  if (!safe(r[13]) || !safe(r[14]) || !safe(r[16]) || !hash(r[15]) || !hash(r[17]) || r[13] > now + 30_000 || r[13] < e.createdAt || !grantId(r[11]) || !caps(r[12])) throw new Error('Invalid bootstrap receipt.')
  const grant = verifyBootstrapGrant(originals.grant, e, rootKey, now, r[13])
  const challenge = verifyBootstrapChallenge(originals.challenge, e, grant, now, r[13])
  const proof = signed(originals.proof, 'donna/bootstrap-proof/v1', 5, e.deviceKey), p = proof.payload
  if (p[1] !== e.bootstrapId || p[2] !== e.hubGeneration || p[3] !== grant.hash || p[4] !== challenge.hash || r[1] !== e.bootstrapId || r[2] !== e.hubGeneration || r[3] !== grant.hash || r[4] !== challenge.hash || r[5] !== proof.hash || r[6] !== e.ownerUserId || r[7] !== e.deviceId || r[8] !== e.deviceKey || r[9] !== e.computerId || r[10] !== e.signKey || r[13] < (challenge.payload[7] as number)) throw new Error('Different bootstrap receipt.')
  const pending = ['donna/bootstrap-pending/v1', r[11], e.bootstrapId, e.hubGeneration, e.ownerUserId, e.deviceId, e.deviceKey, r[12], grant.hash, challenge.hash, proof.hash, r[13], r[14], 'pendingPublication']
  if (digest(pending) !== r[15]) throw new Error('Different bootstrap pending state.')
  return { grant, hashChallenge: challenge.hash, hashProof: proof.hash, hashReceipt: receipt.hash, grantId: r[11], committedAt: r[13] }
}

/** The caller captures an authenticated terminal Worker read, after validating its exact intent. */
export function verifyBootstrapClosure(value: BootstrapClosure, originals: BootstrapOriginals, e: BootstrapExpected, rootKey: string): BootstrapClosure {
  if (!value || Object.keys(value).sort().join(',') !== 'grantId,hashHubReceipt,observedAt,reason,revision,revocation' ||
      !safe(value.revision) || value.revision === 0 || !safe(value.observedAt)) throw new Error('Invalid bootstrap closure.')
  const chain = verifyBootstrapChain(originals, e, rootKey, value.observedAt)
  if (value.grantId !== chain.grantId || value.hashHubReceipt !== chain.hashReceipt) throw new Error('Different bootstrap closure.')
  if (value.reason === 'expired') {
    if (value.observedAt < e.expiresAt || value.revocation !== null) throw new Error('Bootstrap resource still live.')
  } else if (value.reason === 'canceled') {
    const v = signed(value.revocation, 'donna/bootstrap-revocation/v1', 9, e.signKey).payload
    if (v[1] !== e.bootstrapId || v[2] !== e.hubGeneration || v[3] !== chain.grantId || v[4] !== chain.hashReceipt ||
        !safe(v[5]) || v[5] <= Number(originals.receipt[0][14]) || !safe(v[6]) ||
        v[6] < chain.committedAt || v[6] > value.observedAt + 30_000 || !safe(v[7]) || !hash(v[8])) throw new Error('Different bootstrap revocation.')
  } else throw new Error('Invalid bootstrap closure.')
  return value
}

/** Positive signed promotion, distinct from the pending Hub receipt and D1 publication. */
export function verifyBootstrapActivation(value: unknown, originals: BootstrapOriginals, e: BootstrapExpected, rootKey: string, at: number): BootstrapEnvelope {
  const chain = verifyBootstrapChain(originals, e, rootKey, at)
  const signedActivation = signed(value, 'donna/bootstrap-activation/v1', 15, e.signKey)
  const a = signedActivation.payload, r = originals.receipt[0]
  if (!safe(at) || a[1] !== e.bootstrapId || a[2] !== e.hubGeneration || a[3] !== chain.grantId || a[4] !== e.ownerUserId ||
      a[5] !== e.deviceId || a[6] !== e.deviceKey || a[7] !== e.computerId || a[8] !== chain.hashReceipt ||
      !safe(a[9]) || a[9] <= Number(r[14]) || !safe(a[10]) || a[10] <= Number(r[16]) || !hash(a[11]) ||
      !safe(a[12]) || a[12] < chain.committedAt || a[12] > at + 30_000 || !safe(a[13]) || a[13] <= at ||
      a[13] <= a[12] || a[13] - a[12] > 900_000 || !caps(a[14])) throw new Error('Different bootstrap activation.')
  return signedActivation.original
}
