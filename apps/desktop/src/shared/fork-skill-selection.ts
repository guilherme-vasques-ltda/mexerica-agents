// c1/D288: intenção tipada de UMA célula no mesmo `updateBot`. A condição (versão
// opaca do bot + instalação vista) é capturada no gesto, antes de qualquer await.
import type { InstalledSkillV2 } from './fork-skills.ts'

export interface SkillSelectionStamp { readonly protocol: 1; readonly version: string }
export interface SkillSelectionRequest {
  readonly id: string
  readonly skillSelection: Readonly<{ protocol: 1; expectedVersion: string; skillId: string; enabled: boolean; expectedInstallationId: string }>
}
export interface SkillSelectionClient { call<T>(method: string, body?: unknown): Promise<T> }
export interface SkillSelectionSource {
  readonly hello: { hostId: string } | null
  readonly connectedClient: SkillSelectionClient | null
  /** Deve mudar em cada troca de principal ou reconexão, inclusive ao mesmo Hub. */
  readonly principalEpoch: number
  readonly spaceId?: string
  ready(): Promise<SkillSelectionClient>
}
export interface SkillSelectionOrigin {
  readonly source: SkillSelectionSource
  readonly hubId: string
  readonly client: SkillSelectionClient
  readonly epoch: number
  readonly spaceId: string | undefined
}
export type SkillSelectionRefusalCode = 'skill_selection_unsupported' | 'skill_selection_conflict'
  | 'skill_installation_stale' | 'skill_installation_changed' | 'skill_installation_unavailable'
  | 'skill_selection_origin_changed' | 'skill_selection_not_ready' | 'skills_personal_only'
export class SkillSelectionRefusal extends Error {
  readonly code: SkillSelectionRefusalCode
  constructor(code: SkillSelectionRefusalCode, options?: ErrorOptions) { super(code, options); this.code = code; this.name = 'SkillSelectionRefusal' }
}
export class SkillSelectionOutcomeUnknown extends Error {
  readonly code = 'skill_selection_outcome_unknown'
  readonly needsReconciliation = true
  readonly origin: SkillSelectionOrigin
  constructor(origin: SkillSelectionOrigin, cause: unknown) {
    super('skill_selection_outcome_unknown', { cause }); this.origin = origin; this.name = 'SkillSelectionOutcomeUnknown'
  }
}
const object = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
export function readSkillSelectionStamp(raw: unknown): SkillSelectionStamp | null {
  if (!object(raw) || Object.keys(raw).length !== 2 || !Object.hasOwn(raw, 'protocol') || !Object.hasOwn(raw, 'version')
    || raw.protocol !== 1 || typeof raw.version !== 'string' || !/^[0-9a-f]{32}$/.test(raw.version)) return null
  return { protocol: 1, version: raw.version }
}
function installationId(value: unknown): value is string {
  if (typeof value !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}:[1-9][0-9]*$/.test(value)) return false
  const sequence = value.slice(37)
  return sequence.length <= 20 && BigInt(sequence) <= 18446744073709551615n
}
// Vínculo local do gesto; não representa concessão de autoridade.
const captures = new WeakMap<SkillSelectionRequest, SkillSelectionOrigin>()
/** Só id, instalação e estado do conteúdo são lidos; tudo é revalidado aqui. */
export type SkillSelectionItem = Pick<InstalledSkillV2, 'id'> & { readonly installationId?: unknown; readonly contentStatus?: unknown }
export function captureSkillSelection(origin: SkillSelectionOrigin, bot: { id: string; skillSelection?: SkillSelectionStamp | null }, skill: SkillSelectionItem, enabled: boolean): SkillSelectionRequest {
  if (!isSkillSelectionOriginCurrent(origin)) throw new SkillSelectionRefusal('skill_selection_origin_changed')
  const stamp = readSkillSelectionStamp(bot.skillSelection)
  if (!stamp || typeof bot.id !== 'string' || !bot.id || typeof enabled !== 'boolean') throw new SkillSelectionRefusal('skill_selection_unsupported')
  if (!skill || skill.contentStatus === 'unavailable') throw new SkillSelectionRefusal('skill_installation_unavailable')
  // Desligar uma versão alterada é permitido; ligar exige o conteúdo verificado.
  if (skill.contentStatus === 'changed' && enabled) throw new SkillSelectionRefusal('skill_installation_changed')
  if ((skill.contentStatus !== 'verified' && skill.contentStatus !== 'changed') || typeof skill.id !== 'string' || !skill.id || !installationId(skill.installationId)) throw new SkillSelectionRefusal('skill_installation_stale')
  const request = Object.freeze({ id: bot.id, skillSelection: Object.freeze({ protocol: 1 as const, expectedVersion: stamp.version,
    skillId: skill.id, enabled, expectedInstallationId: skill.installationId }) })
  captures.set(request, origin)
  return request
}
export function captureSkillSelectionOrigin(source: SkillSelectionSource): SkillSelectionOrigin {
  if (source.spaceId !== undefined && source.spaceId !== 'personal') throw new SkillSelectionRefusal('skills_personal_only')
  if (!source.hello?.hostId || !source.connectedClient) throw new SkillSelectionRefusal('skill_selection_origin_changed')
  return Object.freeze({ source, hubId: source.hello.hostId, client: source.connectedClient, epoch: source.principalEpoch, spaceId: source.spaceId })
}
export function isSkillSelectionOriginCurrent(origin: SkillSelectionOrigin): boolean {
  const s = origin.source
  return s.hello?.hostId === origin.hubId && s.connectedClient === origin.client && s.principalEpoch === origin.epoch
    && s.spaceId === origin.spaceId && (s.spaceId === undefined || s.spaceId === 'personal')
}
const refusals = new Set<string>(['skill_selection_unsupported', 'skill_selection_conflict', 'skill_installation_stale', 'skill_installation_changed', 'skill_installation_unavailable'])
export interface SkillSelectionResult {
  readonly origin: SkillSelectionOrigin
  readonly bot: { readonly id: string; readonly skillSelection: SkillSelectionStamp; readonly skills: readonly string[] }
}
export async function sendSkillSelection(origin: SkillSelectionOrigin, request: SkillSelectionRequest): Promise<SkillSelectionResult> {
  if (captures.get(request) !== origin) throw new SkillSelectionRefusal('skill_selection_origin_changed')
  if (!isSkillSelectionOriginCurrent(origin)) throw new SkillSelectionRefusal('skill_selection_origin_changed')
  let client: SkillSelectionClient
  try { client = await origin.source.ready() }
  catch (error) {
    throw new SkillSelectionRefusal(isSkillSelectionOriginCurrent(origin) ? 'skill_selection_not_ready' : 'skill_selection_origin_changed', { cause: error })
  }
  if (!isSkillSelectionOriginCurrent(origin) || client !== origin.client) throw new SkillSelectionRefusal('skill_selection_origin_changed')
  let reply: unknown
  try {
    reply = await client.call<unknown>('updateBot', { hubId: origin.hubId, id: request.id, skillSelection: { ...request.skillSelection } })
  } catch (error) {
    if (!isSkillSelectionOriginCurrent(origin)) throw new SkillSelectionOutcomeUnknown(origin, error)
    const message = error instanceof Error ? error.message : ''
    if (refusals.has(message)) throw new SkillSelectionRefusal(message as SkillSelectionRefusalCode, { cause: error })
    throw new SkillSelectionOutcomeUnknown(origin, error)
  }
  if (!isSkillSelectionOriginCurrent(origin)) throw new SkillSelectionOutcomeUnknown(origin, new Error('skill_selection_origin_changed'))
  const bot = object(reply) && object(reply.bot) ? reply.bot : null
  const stamp = readSkillSelectionStamp(bot?.skillSelection)
  const skills = bot?.skills
  if (!bot || bot.id !== request.id || !stamp || !Array.isArray(skills) || !skills.every((s) => typeof s === 'string')
    || skills.includes(request.skillSelection.skillId) !== request.skillSelection.enabled) {
    throw new SkillSelectionOutcomeUnknown(origin, new Error('skill_selection_invalid_reply'))
  }
  return { origin, bot: { id: request.id, skillSelection: stamp, skills: [...skills] } }
}
