import { createHash } from 'node:crypto'
import { accountConfig } from '../main/fork-account/config.ts'

const fields = ['schema', 'deploymentId', 'env', 'issuer', 'origin', 'accountId', 'workerScript', 'd1DatabaseId', 'relayNamespaceId', 'relayClass', 'relayScript', 'relayEnvironment', 'relayWitnessName', 'relayWitnessId', 'schemaVersion', 'bootstrap'] as const
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value)
const name = (value: unknown) => typeof value === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value)
const hex = (value: unknown, size: number) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${size}}$`).test(value)

// Each variant only with its own Clerk instance: Dev with a development key, main with a production key.
const keyPrefix = { dev: 'pk_test_', main: 'pk_live_' } as const

/** Build availability only. Account, bootstrap, pin and Host authority stay in their existing guards.
 * Called by the builder, never from a preference, IPC, health response or runtime environment. */
export function admitsCloudBuild(input: unknown): boolean {
  const env = object(input) ? input.environment : undefined
  if (!object(input) || (env !== 'dev' && env !== 'main')) return false
  if (typeof input.clerkPublishableKey !== 'string' || !input.clerkPublishableKey.startsWith(keyPrefix[env])) return false
  const config = accountConfig(input)
  if (config.kind !== 'configured' || config.env !== env || config.frontendOrigin !== config.issuer) return false
  const m = input.deploymentManifest
  if (!object(m) || Object.keys(m).length !== fields.length || !fields.every(k => Object.hasOwn(m, k)) ||
      m.schema !== 2 || m.schemaVersion !== 6 || m.env !== env || m.deploymentId !== config.deploymentId ||
      m.issuer !== config.issuer || m.origin !== config.cloudOrigin || !hex(m.accountId, 32) ||
      !hex(m.relayNamespaceId, 32) || !hex(m.relayWitnessId, 64) ||
      typeof m.d1DatabaseId !== 'string' || !/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(m.d1DatabaseId) ||
      m.relayClass !== 'ComputerRelay' || ![m.workerScript, m.relayScript, m.relayEnvironment].every(name) ||
      m.workerScript !== m.relayScript || m.relayWitnessName !== `donna/deployment/v4/${m.deploymentId}`) return false
  const root = m.bootstrap
  if (!object(root) || Object.keys(root).sort().join(',') !== 'issuerKeyId,issuerPublicKey' ||
      typeof root.issuerKeyId !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(root.issuerKeyId) ||
      typeof root.issuerPublicKey !== 'string' || !/^[A-Za-z0-9_-]{43}$/.test(root.issuerPublicKey) ||
      Buffer.from(root.issuerPublicKey, 'base64url').toString('base64url') !== root.issuerPublicKey) return false
  const canonicalRoot = { issuerKeyId: root.issuerKeyId, issuerPublicKey: root.issuerPublicKey }
  const canonical = Object.fromEntries(fields.map(k => [k, k === 'bootstrap' ? canonicalRoot : m[k]]))
  return createHash('sha256').update(JSON.stringify(canonical)).digest('hex') === config.manifestHash
}
