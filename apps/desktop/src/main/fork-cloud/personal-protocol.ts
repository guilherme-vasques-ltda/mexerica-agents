/** Inspeção pura de bytes públicos. Só personal-authority seleciona o artefato do build. */
import { createHash } from 'node:crypto'
import { unb64url } from './crypto.ts'

export const PERSONAL_PROFILE = 'personal-linux-v1'
const LIMIT = 16 * 1024
const FIELDS = ['schema', 'deploymentId', 'env', 'issuer', 'origin', 'accountId', 'workerScript', 'd1DatabaseId', 'relayNamespaceId', 'relayClass', 'relayScript', 'relayEnvironment', 'relayWitnessName', 'relayWitnessId', 'schemaVersion', 'bootstrap', 'personalBootstrap'] as const
export type PersonalContext = readonly [typeof PERSONAL_PROFILE, 1, 'main' | 'dev', string, string, string, string, string]
export interface PersonalManifest {
  readonly context: PersonalContext
  readonly issuerPublicKey: string
}
function need(value: unknown): asserts value {
  if (!value) throw new Error('invalidPersonalManifest')
}
function exact(value: unknown, fields: readonly string[]): asserts value is Record<string, unknown> {
  need(value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length === fields.length && fields.every(k => Object.hasOwn(value, k)))
}
function safe(value: unknown): boolean {
  if (typeof value === 'number') return Number.isSafeInteger(value) && value >= 0
  if (typeof value === 'string') return [...value].every(c => { const n = c.codePointAt(0)!; return n < 0xd800 || n > 0xdfff })
  if (Array.isArray(value)) return value.every(safe)
  if (value && typeof value === 'object') return Object.entries(value).every(([k, v]) => safe(k) && safe(v))
  return true
}
export function personalCanonical(raw: Uint8Array): unknown {
  need(raw.byteLength > 0 && raw.byteLength <= LIMIT)
  const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(raw)
  const value: unknown = JSON.parse(text)
  need(safe(value) && JSON.stringify(value) === text)
  return value
}
const name = (v: unknown, max = 128): v is string => typeof v === 'string' && v.length >= 1 && v.length <= max && /^[A-Za-z0-9_-]+$/.test(v)
const hex = (v: unknown, length: number) => typeof v === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(v)
const origin = (v: unknown): v is string => {
  try { const u = new URL(String(v)); return typeof v === 'string' && u.protocol === 'https:' && u.origin === v } catch { return false }
}
function root(value: unknown, personal: boolean) {
  exact(value, personal ? ['profile', 'wire', 'issuerKeyId', 'issuerPublicKey'] : ['issuerKeyId', 'issuerPublicKey'])
  need(name(value.issuerKeyId, 64) && typeof value.issuerPublicKey === 'string' && unb64url(value.issuerPublicKey, 32))
  if (personal) need(value.profile === PERSONAL_PROFILE && value.wire === 1)
  return Object.freeze(personal
    ? { profile: PERSONAL_PROFILE, wire: 1, issuerKeyId: value.issuerKeyId, issuerPublicKey: value.issuerPublicKey }
    : { issuerKeyId: value.issuerKeyId, issuerPublicKey: value.issuerPublicKey })
}
export function inspectPersonalManifest(raw: Uint8Array): PersonalManifest {
  const m = personalCanonical(raw)
  exact(m, FIELDS)
  need(m.schema === 3 && m.schemaVersion === 7 && (m.env === 'dev' || m.env === 'main'))
  need(name(m.deploymentId) && origin(m.issuer) && origin(m.origin))
  need(hex(m.accountId, 32) && hex(m.relayNamespaceId, 32) && hex(m.relayWitnessId, 64))
  need(typeof m.d1DatabaseId === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(m.d1DatabaseId))
  need(m.relayClass === 'ComputerRelay' && [m.workerScript, m.relayScript, m.relayEnvironment].every(v => name(v)))
  need(m.relayWitnessName === `donna/deployment/v4/${m.deploymentId}`)
  const operator = root(m.bootstrap, false), personal = root(m.personalBootstrap, true)
  need(operator.issuerPublicKey !== personal.issuerPublicKey)
  const canonical = Object.fromEntries(FIELDS.map(k => [k, k === 'bootstrap' ? operator : k === 'personalBootstrap' ? personal : m[k]]))
  need(Buffer.from(JSON.stringify(canonical)).equals(Buffer.from(raw)))
  const manifestHash = createHash('sha256').update(raw).digest('hex')
  const context: PersonalContext = Object.freeze([PERSONAL_PROFILE, 1, m.env, m.origin, m.issuer, m.deploymentId, manifestHash, personal.issuerKeyId])
  return Object.freeze({ context, issuerPublicKey: personal.issuerPublicKey })
}
