import { createHash } from 'node:crypto'
import { buildConfig } from '../environment'
import { unb64url } from './crypto'
import type { Principal } from './principal'

const fields = ['schema', 'deploymentId', 'env', 'issuer', 'origin', 'accountId', 'workerScript', 'd1DatabaseId', 'relayNamespaceId', 'relayClass', 'relayScript', 'relayEnvironment', 'relayWitnessName', 'relayWitnessId', 'schemaVersion', 'bootstrap'] as const
const name = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(v)
const hex = (v: unknown, n: number) => typeof v === 'string' && new RegExp(`^[a-f0-9]{${n}}$`).test(v)
const origin = (v: unknown) => { try { const u = new URL(String(v)); return typeof v === 'string' && u.origin === v && u.protocol === 'https:' && !u.port } catch { return false } }
export interface BootstrapAuthority { readonly issuerKeyId: string; readonly issuerPublicKey: string }

/** The only production entry: fixed release artifact, exact hash and captured account.
 * A health response, IPC value or environment variable can never supply this root. */
export function bootstrapAuthority(p: Principal): BootstrapAuthority | null {
  if (!p.current() || buildConfig.environment !== p.deployment.env) return null
  const raw = buildConfig.deploymentManifest
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const m = raw as Record<string, unknown>
  if (Object.keys(m).length !== fields.length || !fields.every(k => Object.hasOwn(m, k)) ||
      m.schema !== 2 || m.schemaVersion !== 6 || m.env !== p.deployment.env ||
      m.deploymentId !== p.deployment.deploymentId || !name(m.deploymentId) ||
      m.issuer !== p.deployment.issuer || !origin(m.issuer) || m.origin !== p.deployment.origin || !origin(m.origin) ||
      !hex(m.accountId, 32) || !hex(m.relayNamespaceId, 32) || !hex(m.relayWitnessId, 64) ||
      typeof m.d1DatabaseId !== 'string' || !/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(m.d1DatabaseId) ||
      m.relayClass !== 'ComputerRelay' || ![m.workerScript, m.relayScript, m.relayEnvironment].every(name) ||
      m.relayWitnessName !== `donna/deployment/v4/${m.deploymentId}`) return null
  const root = m.bootstrap
  if (!root || typeof root !== 'object' || Array.isArray(root)) return null
  const r = root as Record<string, unknown>
  if (Object.keys(r).sort().join(',') !== 'issuerKeyId,issuerPublicKey' ||
      typeof r.issuerKeyId !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(r.issuerKeyId) ||
      typeof r.issuerPublicKey !== 'string' || !unb64url(r.issuerPublicKey, 32)) return null
  const canonicalRoot = { issuerKeyId: r.issuerKeyId, issuerPublicKey: r.issuerPublicKey }
  const canonical = Object.fromEntries(fields.map(k => [k, k === 'bootstrap' ? canonicalRoot : m[k]]))
  if (createHash('sha256').update(JSON.stringify(canonical)).digest('hex') !== p.deployment.manifestHash || !p.current()) return null
  return Object.freeze(canonicalRoot)
}
