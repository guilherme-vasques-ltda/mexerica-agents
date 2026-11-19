// D541/PR1: contrato negativo do RPC donnaBrowserStatus. Não abre navegador nem mantém perfis.
export const BROWSER_PROTOCOL = 2
export type BrowserPreference = 'integrated' | 'computer' | 'off'
export type BrowserTarget = 'desktop' | 'host'
export interface BrowserScope {
  hubId: string
  spaceId: string
  botId: string
}
export interface BrowserStatus {
  version: 2
  browserPreference: BrowserPreference
  scope: BrowserScope
  target: BrowserTarget | null
  status: 'unavailable'
  reason: 'backend_absent' | 'client_unsupported'
  profiles: { status: 'unavailable' }
  capabilities: {
    persistentProfile: false
    ephemeralProfile: false
    observe: false
    control: false
    files: false
    privateLogin: false
  }
  /** Diagnóstico do protocolo; UI futura usa reason e i18n. */
  message: string
}

const record = (value: unknown): Record<string, unknown> | null =>
  value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown> : null
const exact = (value: Record<string, unknown>, keys: string[]): boolean =>
  Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key))
const capabilities = ['persistentProfile', 'ephemeralProfile', 'observe', 'control', 'files', 'privateLogin']

/** Ausência/erro/campo novo nunca vira perfil vazio, capacidade positiva ou fallback de Space. */
export function parseBrowserStatus(value: unknown, origin: BrowserScope, target: BrowserTarget): BrowserStatus {
  const row = record(value)
  const scope = record(row?.scope)
  const profiles = record(row?.profiles)
  const flags = record(row?.capabilities)
  if (!row || !scope || !profiles || !flags
    || !exact(row, ['version', 'browserPreference', 'scope', 'target', 'status', 'reason', 'profiles', 'capabilities', 'message'])
    || !exact(scope, ['hubId', 'spaceId', 'botId'])
    || !exact(profiles, ['status']) || !exact(flags, capabilities)
    || !['integrated', 'computer', 'off'].includes(row.browserPreference as string)
    || row.version !== BROWSER_PROTOCOL || row.status !== 'unavailable'
    || (row.reason !== 'backend_absent' && row.reason !== 'client_unsupported')
    || (row.target !== target && !(row.reason === 'client_unsupported' && row.target === null))
    || profiles.status !== 'unavailable' || !capabilities.every((key) => flags[key] === false)
    || typeof row.message !== 'string' || row.message.length === 0 || row.message.length > 1024) {
    throw new Error('browser_unavailable')
  }
  if (!origin.hubId || !origin.spaceId || !origin.botId
    || scope.hubId !== origin.hubId || scope.spaceId !== origin.spaceId || scope.botId !== origin.botId) {
    throw new Error('browser_context_changed')
  }
  return row as unknown as BrowserStatus
}

/** Só monta a consulta. O Hub confere a autoridade original do transporte e o Space do bot. */
export function browserStatusRequest(origin: BrowserScope, target: BrowserTarget) {
  if (!origin.hubId || !origin.spaceId || !origin.botId || !['desktop', 'host'].includes(target)) {
    throw new Error('browser_context_required')
  }
  return { hubId: origin.hubId, spaceId: origin.spaceId, botId: origin.botId, version: BROWSER_PROTOCOL, target }
}
