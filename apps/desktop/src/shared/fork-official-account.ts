import { createHash } from 'node:crypto'

const fields = ['clerkPublishableKey', 'clerkFrontendOrigin', 'clerkProxyOrigin', 'clerkTokenOrigin'] as const
type PublicAccount = Record<typeof fields[number], string>

function httpsOrigin(value: string) {
  if (!/^https:\/\/(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]*$/.test(value)) return false
  try { return new URL(value).origin === value } catch { return false }
}

export function officialAccount(input: unknown): (PublicAccount & { environment: 'main' }) | null {
  if (!input || typeof input !== 'object' || Array.isArray(input)) return null
  const config = input as Record<string, unknown>
  if (config.environment !== 'main' || Object.keys(config).length !== fields.length + 1 ||
      !fields.every(key => Object.hasOwn(config, key) && typeof config[key] === 'string')) return null
  const { clerkPublishableKey, clerkFrontendOrigin, clerkProxyOrigin, clerkTokenOrigin } = config as PublicAccount
  if (![clerkFrontendOrigin, clerkProxyOrigin, clerkTokenOrigin].every(httpsOrigin) || !clerkPublishableKey.startsWith('pk_live_')) return null
  const encoded = Buffer.from(`${new URL(clerkFrontendOrigin).hostname}$`).toString('base64')
  if (clerkPublishableKey !== `pk_live_${encoded}` && clerkPublishableKey !== `pk_live_${encoded.replace(/=+$/, '')}`) return null
  return { environment: 'main', clerkPublishableKey, clerkFrontendOrigin, clerkProxyOrigin, clerkTokenOrigin }
}

export function officialConfigHash(config: NonNullable<ReturnType<typeof officialAccount>>): string {
  return createHash('sha256').update(JSON.stringify(config)).digest('hex')
}

export function officialAccountPlist(plist: string) {
  const body = /^(?:<\?xml [^<>]*\?>\s*)?(?:<!DOCTYPE plist PUBLIC "-\/\/Apple\/\/DTD PLIST 1\.0\/\/EN" "http:\/\/www\.apple\.com\/DTDs\/PropertyList-1\.0\.dtd">\s*)?<plist version="1\.0">\s*<dict>([\s\S]*?)<\/dict>\s*<\/plist>\s*$/.exec(plist.trim())?.[1]
  if (body === undefined) return null
  const values: Record<string, unknown> = { environment: 'main' }
  let remaining = body.replace(/<!--[\s\S]*?-->/g, '').trim()
  while (remaining.length) {
    const pair = /^<key>(\w+)<\/key>\s*<string>([^<]*)<\/string>\s*/.exec(remaining)
    if (!pair) return null
    const [, field, value] = pair
    if (!fields.some(key => key === field) || Object.hasOwn(values, field)) return null
    values[field] = value.trim()
    remaining = remaining.slice(pair[0].length)
  }
  return officialAccount(values)
}
