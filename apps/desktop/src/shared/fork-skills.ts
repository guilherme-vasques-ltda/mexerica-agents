/** D166/D167. RPC-only: não integra Hello, bots ou o cache espelhado. */
export type SkillAvailability = 'text' | 'adapted' | 'dependencyUnavailable' | 'licenseUnavailable'

export interface SkillOrigin {
  repo: string
  revision: string
  path: string
  /** Digest do pacote original auditado; não confundir com o envelope da adaptação. */
  sourceDigest: string
}

export interface SkillFile {
  path: string
  bytes: number
  sha256: string
  mode: '100644'
}

export interface SkillReadiness {
  enabled: false
  runtime: 'unavailable'
  reason: string
  unavailableActions: string[]
}

export interface CatalogSkill {
  id: string
  /** Alias legado para leitores existentes; não é uma URL instalável. */
  source: string
  name: string
  description: string
  category: string
  aliases: string[]
  origin: SkillOrigin
  license: string
  availability: SkillAvailability
  reason: string
  requirements: string[]
  losses: string[]
  adaptation: string | null
  /** Digest SHA-256 do envelope curado, conferido em toda instalação. */
  digest: string
  files: SkillFile[]
  installable: boolean
  installed: boolean
  installedId: string | null
  readiness: SkillReadiness | null
}

export interface SkillReceipt {
  schema: 1
  catalogId: string
  digest: string
  origin: SkillOrigin
  adaptation: string | null
  license: string
  files: SkillFile[]
  result: 'imported'
  coverage: 'pinnedPassiveTextOnly'
  /** Registro de integridade/proveniência; não é scanner nem aprovação ISO. */
  limitations: string[]
}

export interface PassiveInstalledSkill {
  id: string
  name: string
  description: string
  source: string
  path: string
  /** null em legados e textos manuais. */
  receipt: SkillReceipt | null
}

export type SkillSelection = { id: string } | { source: string }
export interface SkillDetails {
  markdown: string
  catalog: CatalogSkill | null
  receipt: SkillReceipt | null
}
/** `expectedInstallationId` (D306): reinstala a curada alterada vista, pela versão do recibo. */
export interface InstallCatalogSkill { catalogId: string; expectedDigest: string; expectedInstallationId?: string }
/** @deprecated O Hub R6 recusa esta condição insuficiente. Use RemoveInstalledSkillV2. */
export interface RemovePassiveSkill { id: string; expectedDigest?: string }
export interface SkillMutation { skill: PassiveInstalledSkill; alreadyInstalled: boolean }

/** Interface estreita satisfeita pelo HostClient existente e por fixtures. */
export interface SkillClient {
  call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T>
}

export const passiveSkills = {
  catalog: async (c: SkillClient) => (await c.call<{ items: CatalogSkill[] }>('marketSkills')).items,
  installed: async (c: SkillClient) => (await c.call<{ items: PassiveInstalledSkill[] }>('skills')).items,
  details: (c: SkillClient, selection: SkillSelection) => c.call<SkillDetails>('skillDetails', selection),
  install: (c: SkillClient, intent: InstallCatalogSkill) => c.call<SkillMutation>('installSkill', intent, 60_000),
  remove: (c: SkillClient, intent: RemovePassiveSkill) => c.call<Record<string, never>>('removeSkill', intent),
  add: (c: SkillClient, name: string, description: string, instructions: string) =>
    c.call<SkillMutation>('installSkill', { name, description, instructions }),
}

/** Cada troca de Hub/invalidação descarta respostas antigas, inclusive erros e finally. */
export class SkillRequestScope {
  private generation = 0
  invalidate() { this.generation += 1 }
  async run<T>(operation: () => Promise<T>): Promise<{ current: true; value: T } | { current: false }> {
    const generation = this.generation
    try {
      const value = await operation()
      return generation === this.generation ? { current: true, value } : { current: false }
    } catch (error) {
      if (generation !== this.generation) return { current: false }
      throw error
    }
  }
}

/** Identidade opaca emitida/persistida pelo Hub; não derivar de nome/digest. */
export type SkillContentStatus = 'verified' | 'changed' | 'unavailable'
export interface InstalledSkillV2 extends PassiveInstalledSkill {
  installationId: string
  contentStatus: SkillContentStatus
  /**
   * Skills: acabamento (proposta de 10/11/2026) — PRECISA DO HUB. Último uso da skill: só data e contagem, nunca
   * texto de conversa. O Hub de hoje não manda o campo; sem ele a tela não mostra nada de uso.
   */
  usage?: SkillUsage | null
  /** MX-264: o Hub marca `true` quando bots novos já vêm com esta skill (só skills; ausente em Hub anterior). */
  novosBots?: boolean
}
export interface SkillUsage { lastUsedAt: number | null; count: number }
export interface SkillDetailsV2 extends SkillDetails {
  skillProtocol: 2
  installation: InstalledSkillV2 | null
}
export interface SkillMutationV2 { skillProtocol: 2; skill: InstalledSkillV2; alreadyInstalled: boolean }
export interface SkillRemovalV2 { skillProtocol: 2 }
export interface SkillListV2<T> { skillProtocol: 2; items: T[] }
export type ExpectedSkillReceipt = { kind: 'none' } | { kind: 'catalog'; catalogId: string; digest: string }
export interface RemoveInstalledSkillV2 {
  id: string
  expectedInstallationId: string
  expectedReceipt: ExpectedSkillReceipt
}
export type SkillInstallationErrorCode = 'skill_installation_unsupported' | 'skill_installation_stale'
  | 'skill_installation_changed' | 'skill_installation_unavailable'
export class SkillInstallationError extends Error {
  readonly code: SkillInstallationErrorCode
  constructor(code: SkillInstallationErrorCode) { super(code); this.code = code; this.name = 'SkillInstallationError' }
}
/** Envio sem reply confiável exige reconciliação no Hub de origem; nunca retry automático. */
export class SkillMutationOutcomeUnknown extends Error {
  readonly code = 'skill_mutation_outcome_unknown'
  readonly needsReconciliation = true
  readonly method: 'installSkill' | 'removeSkill'
  constructor(method: 'installSkill' | 'removeSkill', cause: unknown) {
    super('skill_mutation_outcome_unknown', { cause })
    this.name = 'SkillMutationOutcomeUnknown'
    this.method = method
  }
}
const unsupported = (): never => { throw new SkillInstallationError('skill_installation_unsupported') }
const object = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const exact = (v: Record<string, unknown>, keys: string[]) =>
  Object.keys(v).length === keys.length && keys.every((key) => Object.hasOwn(v, key))
const digest = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{64}$/.test(v)
function versioned(v: unknown): Record<string, unknown> {
  if (!object(v) || v.skillProtocol !== 2) return unsupported()
  return v
}
function installationId(v: unknown): v is string {
  if (typeof v !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}:[1-9][0-9]*$/.test(v)) return false
  return BigInt(v.split(':')[1]) <= 18446744073709551615n
}
function receiptExpectation(v: unknown): v is ExpectedSkillReceipt {
  if (!object(v)) return false
  if (v.kind === 'none') return exact(v, ['kind'])
  return v.kind === 'catalog' && exact(v, ['kind', 'catalogId', 'digest'])
    && typeof v.catalogId === 'string' && v.catalogId.length > 0 && digest(v.digest)
}
function installedV2(v: unknown): InstalledSkillV2 {
  if (!object(v) || !installationId(v.installationId)
    || !['verified', 'changed', 'unavailable'].includes(String(v.contentStatus))
    || !['id', 'name', 'description', 'source', 'path'].every((k) => typeof v[k] === 'string')
    || !(v.receipt === null || (object(v.receipt) && typeof v.receipt.catalogId === 'string' && digest(v.receipt.digest)))) return unsupported()
  return v as unknown as InstalledSkillV2
}
function mutationV2(raw: unknown): SkillMutationV2 {
  const v = versioned(raw)
  if (typeof v.alreadyInstalled !== 'boolean') return unsupported()
  return { skillProtocol: 2, skill: installedV2(v.skill), alreadyInstalled: v.alreadyInstalled }
}
function removalV2(v: unknown): RemoveInstalledSkillV2 {
  if (!object(v) || !exact(v, ['id', 'expectedInstallationId', 'expectedReceipt'])
    || typeof v.id !== 'string' || !v.id || !installationId(v.expectedInstallationId)
    || !receiptExpectation(v.expectedReceipt)) return unsupported()
  const receipt = v.expectedReceipt
  return { id: v.id, expectedInstallationId: v.expectedInstallationId,
    expectedReceipt: receipt.kind === 'none' ? { kind: 'none' }
      : { kind: 'catalog', catalogId: receipt.catalogId, digest: receipt.digest } }
}
// textos-ok: mensagens exatas que Hubs antigos devolvem; protocolo comparado, nunca mostrado (D454: não se traduz).
const OLD_PARSER_REFUSALS = new Set([
  'Intenção de catálogo inválida; use catalogId e expectedDigest',
  'Texto manual inválido',
  'Intenção de remoção inválida',
  'method_not_found',
  'skill_installation_unsupported',
])
// textos-ok: mensagens exatas de recusa do Hub; protocolo comparado, nunca mostrado (D454: não se traduz).
const BEFORE_EFFECT_REFUSALS = new Set([
  'skill_installation_stale', 'skill_installation_changed', 'skill_installation_unavailable',
  'skills_personal_only', 'hub_context_mismatch', 'space_not_found', 'invalid_space',
  'cross_space_reference', 'cross_conversation_reference', 'unauthorized', 'forbidden',
  'This can only be done at the computer itself.',
])
async function mutateV2<T>(c: SkillClient, method: 'installSkill' | 'removeSkill', body: unknown,
  parse: (reply: unknown) => T, timeoutMs?: number): Promise<T> {
  let reply: unknown
  try { reply = await c.call<unknown>(method, body, timeoutMs) }
  catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    if (OLD_PARSER_REFUSALS.has(message)) return unsupported()
    if (BEFORE_EFFECT_REFUSALS.has(message)) throw error
    throw new SkillMutationOutcomeUnknown(method, error)
  }
  try { return parse(reply) }
  catch (error) { throw new SkillMutationOutcomeUnknown(method, error) }
}

/** RPC-only forte. Sem negociação por lista vazia, fallback ou repetição de mutação. */
export const passiveSkillsV2 = {
  catalog: async (c: SkillClient): Promise<CatalogSkill[]> => {
    const v = versioned(await c.call<unknown>('marketSkills', { skillProtocol: 2 }))
    if (!Array.isArray(v.items)) return unsupported()
    return v.items as CatalogSkill[]
  },
  installed: async (c: SkillClient): Promise<InstalledSkillV2[]> => {
    const v = versioned(await c.call<unknown>('skills', { skillProtocol: 2 }))
    if (!Array.isArray(v.items)) return unsupported()
    return v.items.map(installedV2)
  },
  details: async (c: SkillClient, selection: SkillSelection): Promise<SkillDetailsV2> => {
    const v = versioned(await c.call<unknown>('skillDetails', { ...selection, skillProtocol: 2 }))
    if (typeof v.markdown !== 'string' || !Object.hasOwn(v, 'receipt')
      || !Object.hasOwn(v, 'catalog') || !Object.hasOwn(v, 'installation')) return unsupported()
    if ('id' in selection) {
      const installed = installedV2(v.installation)
      if (installed.id !== selection.id || JSON.stringify(installed.receipt) !== JSON.stringify(v.receipt)) return unsupported()
      if (installed.contentStatus !== 'verified') throw new SkillInstallationError(
        installed.contentStatus === 'changed' ? 'skill_installation_changed' : 'skill_installation_unavailable')
    } else if (v.installation !== null) return unsupported()
    return v as unknown as SkillDetailsV2
  },
  install: (c: SkillClient, intent: InstallCatalogSkill): Promise<SkillMutationV2> => {
    const reparo = object(intent) && Object.hasOwn(intent, 'expectedInstallationId')
    if (!object(intent) || !exact(intent, reparo ? ['catalogId', 'expectedDigest', 'expectedInstallationId'] : ['catalogId', 'expectedDigest'])
      || typeof intent.catalogId !== 'string' || !intent.catalogId || !digest(intent.expectedDigest)
      || (reparo && !installationId(intent.expectedInstallationId))) return Promise.reject(new SkillInstallationError('skill_installation_unsupported'))
    return mutateV2(c, 'installSkill', { skillProtocol: 2, catalogId: intent.catalogId, expectedDigest: intent.expectedDigest,
      ...(reparo ? { expectedInstallationId: intent.expectedInstallationId } : {}) }, mutationV2, 60_000)
  },
  add: (c: SkillClient, name: string, description: string, instructions: string): Promise<SkillMutationV2> =>
    mutateV2(c, 'installSkill', { skillProtocol: 2, name, description, instructions }, mutationV2),
  captureRemoval: (value: InstalledSkillV2): RemoveInstalledSkillV2 => {
    // D306: alterada ou ilegível também se remove; o Hub exige a encarnação e o recibo vistos.
    const skill = installedV2(value)
    return removalV2({ id: skill.id, expectedInstallationId: skill.installationId,
      expectedReceipt: skill.receipt === null ? { kind: 'none' }
        : { kind: 'catalog', catalogId: skill.receipt.catalogId, digest: skill.receipt.digest } })
  },
  remove: async (c: SkillClient, intent: RemoveInstalledSkillV2): Promise<SkillRemovalV2> => {
    const captured = removalV2(intent)
    return mutateV2(c, 'removeSkill', { ...captured, skillProtocol: 2 }, (reply) => {
      const v = versioned(reply)
      if (!exact(v, ['skillProtocol'])) return unsupported()
      return { skillProtocol: 2 }
    })
  },
}
