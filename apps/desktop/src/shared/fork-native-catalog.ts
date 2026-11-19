// Biblioteca de especialistas (docs/features/bots-nativos.md, BN-03): o fio de `donnaNativeCatalog` e
// `donnaNativeCatalogAdd` como o Hub da MAIN o define. Só leitura estrita do DTO e montagem da intenção fechada;
// nenhuma regra de vínculo, perfil ou autoridade mora aqui. Perfis, rostos e Almas vêm sempre do Hub.

export type NativeLinkState = 'available' | 'active' | 'removed'

export interface NativeProfile {
  catalogKey: string
  profileVersion: number
  origin: 'donnaOriginal'
  name: string
  title: string
  avatarConfig: unknown
  /** A Alma integral, como o Hub a entrega. */
  description: string
  examples: string[]
  /** Sugestão editorial: não é inventário, acesso nem prontidão. */
  suggestedResources: string
}

export interface NativeLink {
  catalogKey: string
  generation: number
  revision: number
  botId: string | null
  profileVersion: number | null
  state: NativeLinkState
}

export interface NativeCatalog {
  hubId: string
  spaceId: string
  catalogVersion: number
  profiles: NativeProfile[]
  links: NativeLink[]
}

export interface NativeAdded {
  hubId: string
  spaceId: string
  catalogKey: string
  clientRequestKey: string
  generation: number
  botId: string
  state: 'active' | 'removed'
}

/** O corpo fechado da inclusão. Sem configuração, conta, grant, modelo ou ID de bot. */
export interface NativeAddBody {
  hubId: string
  spaceId: string
  catalogKey: string
  profileVersion: number
  clientRequestKey: string
  intent: 'add' | 'readd'
  expectedGeneration: number
  expectedRevision: number
  backend?: string
}

const invalid = () => new Error('native_invalid_answer')
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const text = (v: unknown) => typeof v === 'string' && v.length > 0
const count = (v: unknown) => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0

function profileOf(raw: unknown): NativeProfile {
  if (!isObject(raw)) throw invalid()
  const { catalogKey, profileVersion, origin, name, title, avatarConfig, description, examples, suggestedResources } = raw
  if (!text(catalogKey) || !count(profileVersion) || (profileVersion as number) < 1 || origin !== 'donnaOriginal' || !text(name)
    || typeof title !== 'string' || !text(description) || !Array.isArray(examples) || !examples.every(text) || typeof suggestedResources !== 'string') throw invalid()
  return {
    catalogKey: catalogKey as string, profileVersion: profileVersion as number, origin, name: name as string, title,
    avatarConfig, description: description as string, examples: examples as string[], suggestedResources,
  }
}

function linkOf(raw: unknown): NativeLink {
  if (!isObject(raw)) throw invalid()
  const { catalogKey, generation, revision, botId, profileVersion, state } = raw
  if (!text(catalogKey) || !count(generation) || !count(revision) || (state !== 'available' && state !== 'active' && state !== 'removed')) throw invalid()
  if (state === 'available' ? botId !== null : !text(botId)) throw invalid()
  if (profileVersion !== null && !count(profileVersion)) throw invalid()
  return { catalogKey: catalogKey as string, generation: generation as number, revision: revision as number, botId: botId as string | null, profileVersion: profileVersion as number | null, state }
}

/** A prévia de um Hub e space exatos; outra origem ou forma inesperada é recusada inteira, nunca pintada pela metade. */
export function parseNativeCatalog(raw: unknown, hubId: string, spaceId: string): NativeCatalog {
  if (!isObject(raw)) throw invalid()
  if (raw.hubId !== hubId) throw new Error('cross_hub_response')
  if (raw.spaceId !== spaceId) throw new Error('cross_space_response')
  if (!count(raw.catalogVersion) || !Array.isArray(raw.profiles) || !Array.isArray(raw.links)) throw invalid()
  const profiles = raw.profiles.map(profileOf)
  const links = raw.links.map(linkOf)
  const keys = profiles.map((p) => p.catalogKey)
  // Um vínculo por perfil, nem mais nem menos: o Hub projeta cada slot deste space.
  if (new Set(keys).size !== keys.length || links.length !== keys.length || !links.every((l) => keys.includes(l.catalogKey))
    || new Set(links.map((l) => l.catalogKey)).size !== links.length) throw invalid()
  return { hubId, spaceId, catalogVersion: raw.catalogVersion as number, profiles, links }
}

/** A resposta só confirma a intenção enviada: mesmo Hub, space, perfil e chave. */
export function parseNativeAdded(raw: unknown, body: NativeAddBody): NativeAdded {
  if (!isObject(raw)) throw invalid()
  if (raw.hubId !== body.hubId) throw new Error('cross_hub_response')
  if (raw.spaceId !== body.spaceId) throw new Error('cross_space_response')
  if (raw.catalogKey !== body.catalogKey || raw.clientRequestKey !== body.clientRequestKey || !count(raw.generation) || !text(raw.botId)
    || (raw.state !== 'active' && raw.state !== 'removed')) throw invalid()
  return { hubId: body.hubId, spaceId: body.spaceId, catalogKey: body.catalogKey, clientRequestKey: body.clientRequestKey, generation: raw.generation as number, botId: raw.botId as string, state: raw.state }
}

/**
 * A intenção de um gesto, a partir do vínculo lido na mesma prévia. Pessoal não manda motor (segue a fonte canônica
 * do Hub); fora dele o motor é obrigatório. A chave nasce aqui e acompanha o corpo até uma resposta definitiva.
 */
export function nativeAddBody(catalog: NativeCatalog, catalogKey: string, opts: { personal: boolean; backend: string | null; key: string }): NativeAddBody {
  const profile = catalog.profiles.find((p) => p.catalogKey === catalogKey)
  const link = catalog.links.find((l) => l.catalogKey === catalogKey)
  if (!profile || !link || link.state === 'active') throw new Error('native_gesture_unavailable')
  if (!opts.personal && !opts.backend) throw new Error('space_backend_selection_required')
  return {
    hubId: catalog.hubId,
    spaceId: catalog.spaceId,
    catalogKey,
    profileVersion: profile.profileVersion,
    clientRequestKey: opts.key,
    intent: link.state === 'removed' ? 'readd' : 'add',
    expectedGeneration: link.generation,
    expectedRevision: link.revision,
    ...(opts.personal ? {} : { backend: opts.backend! }),
  }
}

export type NativeFailure = 'unknown' | 'denied' | 'conflict' | 'old_hub' | 'moved' | 'gone' | 'refused'

/** Sem resposta (ou 5xx) não prova ausência de efeito: a intenção fica guardada com a mesma chave. */
export function nativeFailure(e: unknown): NativeFailure {
  const status = isObject(e) || e instanceof Error ? (e as { status?: unknown }).status : undefined
  if (status === 0 || (typeof status === 'number' && status >= 500)) return 'unknown'
  if (status === 401 || status === 403) return 'denied'
  const message = e instanceof Error ? e.message : isObject(e) && typeof e.message === 'string' ? e.message : String(e)
  if (/not_allowed|remote_setup_unavailable|owner_unavailable|binding_unavailable|cloud_unsynced|forbidden/i.test(message)) return 'denied'
  if (/native_catalog_conflict|native_profile_version_conflict/.test(message)) return 'conflict'
  if (/unknown method|unknown_method/.test(message) || status === 404) return 'old_hub'
  if (/cross_hub|cross_space|hub_context_mismatch|stale_space_context/.test(message)) return 'moved'
  if (/space_not_found|invalid_space/.test(message)) return 'gone'
  return 'refused'
}

/** Os códigos do Hub em palavras para leigo; o código nunca chega à tela. */
export function nativeFailureText(e: unknown): string {
  const message = e instanceof Error ? e.message : isObject(e) && typeof e.message === 'string' ? e.message : String(e)
  switch (nativeFailure(e)) {
    case 'unknown': return 'Sem resposta do computador dos seus bots. Não sabemos se o bot foi adicionado.'
    case 'denied': return 'Este aparelho não pode adicionar bots neste computador agora.'
    case 'conflict': return 'O catálogo mudou desde a prévia. Confira de novo antes de adicionar.'
    case 'old_hub': return 'Este computador ainda não tem a biblioteca de especialistas.'
    case 'moved': return 'A conexão passou para outro computador ou space. Abra a biblioteca de novo.'
    case 'gone': return 'Este space não está mais disponível neste computador.'
    default:
      if (message.includes('space_backend_selection_required')) return 'Escolha o motor antes de adicionar.'
      if (message.includes('motor desconhecido')) return 'Esse motor não está disponível neste computador.'
      return 'O computador dos seus bots não adicionou agora. Tente de novo.'
  }
}
