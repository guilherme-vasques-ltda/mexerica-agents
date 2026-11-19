// Test fixtures only (fork-cloud-build and account-entry tests): public-shaped configs, never real values.
import { createHash } from 'node:crypto'

export const hash = (value: unknown) => createHash('sha256').update(JSON.stringify(value)).digest('hex')

/** A complete public release config for one variant, shaped like tools/account-config.mjs writes it. */
export function release(env: 'dev' | 'main', issuer: string, cloud: string, tokenOrigin: string) {
  const deploymentId = `donna-${env}-fixture`
  const manifest = {
    schema: 2, deploymentId, env, issuer, origin: cloud, accountId: 'a'.repeat(32), workerScript: `donna-${env}`,
    d1DatabaseId: '12345678-1234-1234-1234-123456789abc', relayNamespaceId: 'b'.repeat(32),
    relayClass: 'ComputerRelay', relayScript: `donna-${env}`, relayEnvironment: 'production',
    relayWitnessName: `donna/deployment/v4/${deploymentId}`, relayWitnessId: 'c'.repeat(64), schemaVersion: 6,
    bootstrap: { issuerKeyId: 'fixture-root', issuerPublicKey: Buffer.alloc(32, 7).toString('base64url') },
  }
  const key = (prefix: string) => `${prefix}${Buffer.from(`${new URL(issuer).hostname}$`).toString('base64')}`
  const input = {
    environment: env, clerkPublishableKey: key(env === 'dev' ? 'pk_test_' : 'pk_live_'),
    cloudURL: cloud, clerkFrontendOrigin: issuer, clerkProxyOrigin: env === 'main' ? 'https://auth.example.invalid' : issuer, clerkTokenOrigin: tokenOrigin, clerkIssuer: issuer,
    deploymentId, deploymentEnv: env, deploymentManifest: manifest, manifestHash: hash(manifest),
  }
  return { input, manifest, key }
}

/** Production shape: Clerk FAPI is the issuer and the token origin is the site (docs/guides/app-mac-local.md). */
export const productionRelease = () => release('main', 'https://clerk.example.invalid', 'https://api.example.invalid', 'https://app.example.invalid')

/** What `node tools/account-config.mjs main` writes from the committed, empty main.plist. */
export const emptyMainRelease = { environment: 'main', clerkPublishableKey: null, cloudURL: null, clerkFrontendOrigin: null, clerkProxyOrigin: null, clerkTokenOrigin: null, clerkIssuer: null, deploymentId: null, deploymentEnv: null, manifestHash: null, deploymentManifest: null }
