/** Explicit build-time pins. Values are public configuration, never credentials. */
export type AccountConfig =
  | { kind: 'unavailable'; reason: 'missing' | 'invalid' }
  | { kind: 'configured'; frontendOrigin: string; proxyOrigin: string; tokenOrigin: string; cloudOrigin: string; issuer: string; deploymentId: string; env: 'main' | 'dev'; manifestHash: string }

function origin(value: unknown): string | null {
  if (typeof value !== 'string') return null
  try {
    const url = new URL(value)
    if (url.protocol !== 'https:' || url.username || url.password || url.pathname !== '/' || url.search || url.hash || url.port) return null
    if (!/^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(url.hostname)) return null
    return url.origin
  } catch { return null }
}

export function accountConfig(input: { clerkPublishableKey?: unknown; cloudURL?: unknown; clerkFrontendOrigin?: unknown; clerkProxyOrigin?: unknown; clerkTokenOrigin?: unknown; clerkIssuer?: unknown; deploymentId?: unknown; manifestHash?: unknown; deploymentEnv?: unknown; environment?: unknown }): AccountConfig {
  const values = [input.clerkPublishableKey, input.cloudURL, input.clerkFrontendOrigin, input.clerkProxyOrigin, input.clerkTokenOrigin, input.clerkIssuer, input.deploymentId, input.manifestHash, input.deploymentEnv, input.environment]
  if (values.some((v) => v === undefined || v === null || v === '')) return { kind: 'unavailable', reason: 'missing' }
  if ((input.environment !== 'main' && input.environment !== 'dev') || input.deploymentEnv !== input.environment) return { kind: 'unavailable', reason: 'invalid' }
  if (typeof input.clerkPublishableKey !== 'string') return { kind: 'unavailable', reason: 'invalid' }
  const match = /^pk_(?:test|live)_([A-Za-z0-9+/]+={0,2})$/.exec(input.clerkPublishableKey)
  if (!match) return { kind: 'unavailable', reason: 'invalid' }
  const encoded = match[1]!
  const bytes = Buffer.from(encoded, 'base64')
  if (bytes.toString('base64').replace(/=+$/, '') !== encoded.replace(/=+$/, '')) return { kind: 'unavailable', reason: 'invalid' }
  const payload = bytes.toString('utf8')
  const frontendOrigin = origin(input.clerkFrontendOrigin)
  const proxyOrigin = origin(input.clerkProxyOrigin)
  const tokenOrigin = origin(input.clerkTokenOrigin)
  const cloudOrigin = origin(input.cloudURL)
  const issuer = origin(input.clerkIssuer)
  if (!frontendOrigin || !proxyOrigin || (input.environment === 'main' && proxyOrigin !== 'https://auth.example.invalid') || !tokenOrigin || !cloudOrigin || !issuer || payload !== `${new URL(frontendOrigin).hostname}$` ||
      typeof input.deploymentId !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(input.deploymentId) ||
      typeof input.manifestHash !== 'string' || !/^[a-f0-9]{64}$/.test(input.manifestHash)) return { kind: 'unavailable', reason: 'invalid' }
  return { kind: 'configured', frontendOrigin, proxyOrigin, tokenOrigin, cloudOrigin, issuer, deploymentId: input.deploymentId, env: input.environment, manifestHash: input.manifestHash }
}

/** Troca somente o endereço conhecido da FAPI; URLs externas do provedor seguem intactas. */
export function accountBrowserURL(value: string, config: AccountConfig): string {
  if (config.kind !== 'configured') throw new Error('account_unavailable')
  const url = new URL(value)
  if (url.protocol !== 'https:' || url.username || url.password) throw new Error('invalid_account_url')
  if (url.origin === config.frontendOrigin) url.host = new URL(config.proxyOrigin).host
  return url.href
}
