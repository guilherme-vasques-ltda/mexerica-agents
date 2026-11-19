import type { HostClient } from '../../client/host-client'
import type { RetornoLogin } from '../fork/meus-mcp/retorno-login'
import { traduzir } from '../../lib/fork-i18n/index.ts'

// Marketplace wire types (host/src/market/): connectors are MCP servers,
// skills are instruction folders; agents come from `Hello.backends`. (kit Market.swift)

export interface InstalledConnector {
  id: string
  name: string
  description: string
  /** Passive historical provenance; never hide a saved connection by this field. */
  registryName?: string | null
  /** O id do serviço do catálogo (ficha do Hub) de onde a conta veio; ausente ou `null` no MCP do próprio usuário. */
  servico?: string | null
  /** A chave da logo do serviço (o Hub a resolve pelo catálogo); ausente ou `null` sem serviço do catálogo. */
  marca?: string | null
  /** `local` (a command the agent starts) or `remote` (a URL). */
  kind: string
  /** Saved target only; neither readiness, current authentication nor bot authorization. */
  configuration?: 'configured' | 'manualRequired'
  command?: string | null
  url?: string | null
  /** Names of the keys and headers that are set; their values stay on the computer. */
  keys: string[]
  /** Remote servers: `none`, `signedOut` (waits for sign-in; bots don't get it yet) or `signedIn`. */
  auth?: string | null
}

export const needsSignIn = (c: InstalledConnector) => c.auth === 'signedOut'

/** Older Hubs omit configuration; infer only whether the public saved target exists. */
export function connectorConfiguration(c: InstalledConnector): 'configured' | 'manualRequired' {
  if (c.configuration) return c.configuration
  const target = c.command ?? c.url
  if (!target?.trim()) return 'manualRequired'
  return c.command != null || !/[{}]/.test(target) ? 'configured' : 'manualRequired'
}

// D454 (fatia 8a): o código fica no começo, como antes; o texto vem do catálogo em inglês (o módulo não lê o idioma
// da tela; ninguém na interface chama mais estes métodos aposentados).
export const MCP_REGISTRY_UNAVAILABLE = `mcp_registry_unavailable: ${traduzir('en', 'extensoes.mcp.registroRetirado')}`

const registryUnavailable = (): never => { throw new Error(MCP_REGISTRY_UNAVAILABLE) }

/** Where a connector's sign-in page is and who catches its return: `app`, `host` or `desktop` (the app's own listener). */
export interface ConnectorSignIn {
  url: string
  callback: string
}

export interface MarketInput {
  name: string
  description?: string | null
  secret: boolean
  required: boolean
  default?: string | null
  placeholder?: string | null
}

export interface InstallOption {
  id: string
  /** npm | pypi | oci | remote */
  kind: string
  label: string
  inputs: MarketInput[]
}

export interface MarketConnector {
  name: string
  title: string
  description?: string | null
  version?: string | null
  website?: string | null
  installed: boolean
  options: InstallOption[]
}

export interface ConnectionRequest {
  toolkit?: string | null
  kind: string
  status: string
  title: string
  botId: string
  registryName?: string | null
  connectorId?: string | null
  field?: string | null
  location?: string | null
  /** Login requests: the website's domain or the app's name. */
  site?: string | null
  /** Agent sign-in requests: the agent (backend id) to sign in to. */
  backend?: string | null
}

/** A saved website or app login; the password stays on the computer. */
export interface SavedLogin {
  id: string
  site: string
  username: string
  /** The user's name for it, or `site · username` (an older host sends none). */
  label?: string
}

/** What a login is called on screen. */
export const loginTitle = (l: SavedLogin) => l.label || (l.username ? `${l.site} · ${l.username}` : l.site)

export interface CredentialStatus {
  provider: string
  onePasswordConnected: boolean
}

// Agent setup (kit Models.swift)

export type AuthMethodKind = 'terminal' | 'agent' | 'envVar'

export interface AuthVar {
  name: string
  label: string
  secret: boolean
  optional: boolean
}

export interface AuthMethod {
  id: string
  name: string
  description?: string | null
  kind?: AuthMethodKind | null
  vars?: AuthVar[] | null
  link?: string | null
}

/** How an agent can be signed in, from asking the agent itself. */
export interface AgentAuth {
  /** Antigravity: confirma o CLI, sem afirmar login no ACP. */
  cliSignedIn?: boolean | null
  signedIn?: boolean | null
  /** The agent's own words when it wants signing in (can hold a pairing code). */
  detail?: string | null
  methods: AuthMethod[]
  /** Keys saved on the computer for this agent (names only). */
  savedEnv?: string[] | null
  /** Codync's own sign-in command is available. */
  login?: boolean | null
}

/** What a setup terminal on the computer runs. */
export type SetupStep = 'install' | 'node' | 'login'

const s = (n: number) => n * 1000

// Host calls, same methods and timeouts as kit's HostClient extensions.

export const market = {
  connectors: async (c: HostClient) => (await c.call<{ items: InstalledConnector[] }>('connectors')).items,
  /** Native user configurations, including historical catalog entries. */
  nativeConnectors: async (c: HostClient) => (await c.call<{ items: InstalledConnector[] }>('connectors')).items,
  /** Retired compatibility signature. Never send discovery to an older Hub either. */
  marketConnectors: async (_c: HostClient, _search: string, _cursor: string | null = null): Promise<{ items: MarketConnector[]; next: string | null }> => registryUnavailable(),
  /** Retired catalog installation. Manual addConnector remains available. */
  installConnector: async (_c: HostClient, _registryName: string, _option: string, _inputs: Record<string, string>): Promise<InstalledConnector> => registryUnavailable(),
  /** A connector you describe yourself: a command line with env, or an https URL with headers. */
  addConnector: async (c: HostClient, body: { name: string; description?: string; command: string | null; args?: string[]; url: string | null; env: Record<string, string>; headers: Record<string, string> }) =>
    (await c.call<{ connector: InstalledConnector }>('installConnector', body, s(45))).connector,
  /** Adds every server in a pasted MCP config (`mcpServers` / `servers` JSON, or one server's entry). */
  importConnectors: async (c: HostClient, config: string) => (await c.call<{ items: InstalledConnector[] }>('importConnectors', { config }, s(60))).items,
  // `loopbackPort`: a porta do ouvinte do app (Hub em outro computador, via túnel); sem ela, o retorno cai no próprio Hub.
  connectorSignIn: (c: HostClient, id: string, loopbackPort?: number | null) =>
    c.call<ConnectorSignIn>('connectorSignIn', { id, ...(loopbackPort ? { loopbackPort } : {}) }, s(60)),
  /** M3-B: `iss` segue como veio do retorno; o Hub decide. */
  finishConnectorSignIn: async (c: HostClient, retorno: RetornoLogin) =>
    (await c.call<{ connector: InstalledConnector }>('connectorSignInFinish', { state: retorno.state, code: retorno.code, error: retorno.error, iss: retorno.iss }, s(60))).connector,
  removeConnector: (c: HostClient, id: string) => c.call('removeConnector', { id }),
  // D166 R8: skills only through `passiveSkillsV2` (shared/fork-skills.ts); no weak V1 RPCs here.
  /** Retired metadata lookup; passive registryName never triggers network access. */
  connectorInfo: async (_c: HostClient, _registryName: string): Promise<MarketConnector> => registryUnavailable(),
  verifyConnector: (c: HostClient, id: string) => c.call<{ status: string }>('connectorVerify', { id }, s(120)),
  finishConnectionRequest: (
    c: HostClient,
    entryId: string,
    opts: { connectorId?: string; value?: string; username?: string; label?: string; loginId?: string } = {},
  ) =>
    c.call('connectorRequestFinish', {
      entryId, connectorId: opts.connectorId ?? null, value: opts.value ?? null, username: opts.username ?? null,
      label: opts.label ?? null, loginId: opts.loginId ?? null,
    }, s(120)),
  logins: async (c: HostClient) => (await c.call<{ items: SavedLogin[] }>('credentialLogins', {}, s(60))).items,
  saveLogin: (c: HostClient, site: string, username: string, password: string, label = '') =>
    c.call('credentialSaveLogin', { site, username, password, label }, s(60)),
  /** An empty label goes back to `site · username`. */
  labelLogin: (c: HostClient, id: string, label: string) => c.call('credentialLabelLogin', { id, label }, s(60)),
  removeLogin: (c: HostClient, id: string) => c.call('credentialRemoveLogin', { id }, s(60)),
  credentialStatus: (c: HostClient) => c.call<CredentialStatus>('credentialStatus', {}, s(60)),
  setOnePasswordToken: (c: HostClient, token: string) => c.call<CredentialStatus>('credentialSetOnePassword', { token }, s(60)),
  updateConnectorCredentials: (c: HostClient, id: string, fields: Record<string, string>) => c.call('credentialUpdateConnector', { id, fields }, s(120)),

  // Agent setup terminals
  /** The first sign-in through a registry build can download it. */
  agentSetup: async (c: HostClient, backend: string, step: SetupStep, method: string | null, cols: number, rows: number) =>
    await c.call<{ term: string; restrictedLinks?: boolean }>('agentSetup', { backend, step, method, cols, rows }, s(120)),
  agentAuth: (c: HostClient, backend: string) => c.call<AgentAuth>('agentAuth', { backend }, s(120)),
  agentAuthenticate: (c: HostClient, backend: string, method: string) => c.call<AgentAuth>('agentAuthenticate', { backend, method }, s(660)),
  setAgentEnv: (c: HostClient, backend: string, vars: Record<string, string>): Promise<AgentAuth> => {
    if (backend === 'opencode' && Object.keys(vars).length === 1 && 'OPENCODE_API_KEY' in vars) {
      // Slot do Hub: usar o transporte sem a injeção automática de spaceId do cliente.
      return c.transport.call('engineGoKeySave', { backend, vars, ...(c.boundHubId ? { hubId: c.boundHubId } : {}) }, s(120)) as Promise<AgentAuth>
    }
    return c.call<AgentAuth>('setAgentEnv', { backend, vars }, s(120))
  },
  termInput: (c: HostClient, term: string, data: string) => c.call('termInput', { term, data }),
  termResize: (c: HostClient, term: string, cols: number, rows: number) => c.call('termResize', { term, cols, rows }),
  termClose: (c: HostClient, term: string) => c.call('termClose', { term }),
}

/** Keep fetched catalog metadata in memory, but create rows only as the user requests them. */
export interface MarketplacePage<T> {
  items: T[]
  visibleCount: number
}

export const emptyPage = <T>(): MarketplacePage<T> => ({ items: [], visibleCount: 12 })

export function appendPage<T>(page: MarketplacePage<T>, more: T[], id: (t: T) => string): MarketplacePage<T> {
  const known = new Set(page.items.map(id))
  return { ...page, items: [...page.items, ...more.filter((t) => !known.has(id(t)) && (known.add(id(t)), true))] }
}

export const replacePage = <T>(items: T[], id: (t: T) => string) => appendPage(emptyPage<T>(), items, id)
export const revealMore = <T>(page: MarketplacePage<T>): MarketplacePage<T> => ({ ...page, visibleCount: Math.min(page.items.length, page.visibleCount + 12) })
export const visible = <T>(page: MarketplacePage<T>) => page.items.slice(0, page.visibleCount)
export const hasHiddenItems = <T>(page: MarketplacePage<T>) => page.items.length > page.visibleCount

export const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))
export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
