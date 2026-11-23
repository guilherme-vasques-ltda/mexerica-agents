export type VinculoAmbienteState = 'unconfigured' | 'configured'
export type VinculoAmbienteMachineKind = 'local' | 'dock'

export interface VinculoAmbienteContext {
  hubId: string
  spaceId: string
  botId: string
}

export interface VinculoAmbienteProject {
  id: string
  label: string
  gitLinked: boolean
}

export interface VinculoAmbienteMachine {
  kind: VinculoAmbienteMachineKind
  id: string
  label: string
}

export interface VinculoAmbienteRef {
  id: string
  label: string
}

export interface VinculoAmbienteUnconfigured {
  schema: 1
  hubId: string
  spaceId: string
  botId: string
  revision: number
  state: 'unconfigured'
  execution: 'unavailable'
}

export interface VinculoAmbienteConfigured {
  schema: 1
  hubId: string
  spaceId: string
  botId: string
  revision: number
  state: 'configured'
  execution: 'unavailable'
  project: VinculoAmbienteProject
  machine: VinculoAmbienteMachine
  model: VinculoAmbienteRef
  access: VinculoAmbienteRef
}

export type VinculoAmbiente = VinculoAmbienteUnconfigured | VinculoAmbienteConfigured

export class VinculoAmbienteError extends Error {
  readonly code: string

  constructor(code: string, detail?: string) {
    super(detail ? `${code}: ${detail}` : code)
    this.name = 'VinculoAmbienteError'
    this.code = code
  }
}

const OPAQUE_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/
const SECRET_PREFIXES = [
  'sk-',
  'pk_',
  'rk_',
  'ghp_',
  'gho_',
  'ghu_',
  'ghs_',
  'ghr_',
  'github_pat_',
  'xox',
  'aiza',
  'akia',
  'asia',
]
const CONTROL_CHARS = /[\x00-\x1f\x7f]/
const LABEL_MAX_LEN = 120

const UNCONFIGURED_KEYS = ['schema', 'hubId', 'spaceId', 'botId', 'revision', 'state', 'execution'] as const
const CONFIGURED_KEYS = [...UNCONFIGURED_KEYS, 'project', 'machine', 'model', 'access'] as const

export function isOpaqueBindingId(value: unknown): value is string {
  if (typeof value !== 'string' || !OPAQUE_ID.test(value) || value.includes('..')) {
    return false
  }
  const lower = value.toLowerCase()
  return !SECRET_PREFIXES.some((prefix) => lower.startsWith(prefix))
}

function asObject(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new VinculoAmbienteError('binding_invalid_answer', field)
  }
  return value as Record<string, unknown>
}

function assertAllowedKeys(obj: Record<string, unknown>, allowed: readonly string[], field: string) {
  for (const key of Object.keys(obj)) {
    if (!allowed.includes(key)) {
      throw new VinculoAmbienteError('binding_unexpected_field', `${field}.${key}`)
    }
  }
}

function decodeOpaqueId(value: unknown, field: string): string {
  if (!isOpaqueBindingId(value)) {
    throw new VinculoAmbienteError('binding_invalid_id', field)
  }
  return value
}

function decodeLabel(value: unknown, field: string): string {
  if (
    typeof value !== 'string' ||
    value.trim().length === 0 ||
    value.length > LABEL_MAX_LEN ||
    CONTROL_CHARS.test(value)
  ) {
    throw new VinculoAmbienteError('binding_invalid_label', field)
  }
  return value
}

function decodeRef(value: unknown, field: string): VinculoAmbienteRef {
  const obj = asObject(value, field)
  assertAllowedKeys(obj, ['id', 'label'], field)
  return {
    id: decodeOpaqueId(obj.id, `${field}.id`),
    label: decodeLabel(obj.label, `${field}.label`),
  }
}

function decodeProject(value: unknown): VinculoAmbienteProject {
  const obj = asObject(value, 'project')
  assertAllowedKeys(obj, ['id', 'label', 'gitLinked'], 'project')
  if (typeof obj.gitLinked !== 'boolean') {
    throw new VinculoAmbienteError('binding_invalid_answer', 'project.gitLinked')
  }
  return {
    id: decodeOpaqueId(obj.id, 'project.id'),
    label: decodeLabel(obj.label, 'project.label'),
    gitLinked: obj.gitLinked,
  }
}

function decodeMachine(value: unknown): VinculoAmbienteMachine {
  const obj = asObject(value, 'machine')
  assertAllowedKeys(obj, ['kind', 'id', 'label'], 'machine')
  if (obj.kind !== 'local' && obj.kind !== 'dock') {
    throw new VinculoAmbienteError('binding_invalid_answer', 'machine.kind')
  }
  return {
    kind: obj.kind,
    id: decodeOpaqueId(obj.id, 'machine.id'),
    label: decodeLabel(obj.label, 'machine.label'),
  }
}

export function decodeVinculoAmbiente(value: unknown, expected: VinculoAmbienteContext): VinculoAmbiente {
  const expectedHubId = decodeOpaqueId(expected.hubId, 'expected.hubId')
  const expectedSpaceId = decodeOpaqueId(expected.spaceId, 'expected.spaceId')
  const expectedBotId = decodeOpaqueId(expected.botId, 'expected.botId')

  const root = asObject(value, 'root')
  if (root.schema !== 1) {
    throw new VinculoAmbienteError('binding_invalid_schema', 'schema')
  }

  const hubId = decodeOpaqueId(root.hubId, 'hubId')
  if (hubId !== expectedHubId) {
    throw new VinculoAmbienteError('cross_hub_response', 'hubId')
  }

  const spaceId = decodeOpaqueId(root.spaceId, 'spaceId')
  if (spaceId !== expectedSpaceId) {
    throw new VinculoAmbienteError('cross_space_response', 'spaceId')
  }

  const botId = decodeOpaqueId(root.botId, 'botId')
  if (botId !== expectedBotId) {
    throw new VinculoAmbienteError('cross_bot_response', 'botId')
  }

  if (typeof root.revision !== 'number' || !Number.isSafeInteger(root.revision) || root.revision < 0) {
    throw new VinculoAmbienteError('binding_invalid_answer', 'revision')
  }

  if (root.execution !== 'unavailable') {
    throw new VinculoAmbienteError('binding_execution_not_admitted', 'execution')
  }

  if (root.state === 'unconfigured') {
    assertAllowedKeys(root, UNCONFIGURED_KEYS, 'root')
    return {
      schema: 1,
      hubId,
      spaceId,
      botId,
      revision: root.revision,
      state: 'unconfigured',
      execution: 'unavailable',
    }
  }

  if (root.state === 'configured') {
    assertAllowedKeys(root, CONFIGURED_KEYS, 'root')
    return {
      schema: 1,
      hubId,
      spaceId,
      botId,
      revision: root.revision,
      state: 'configured',
      execution: 'unavailable',
      project: decodeProject(root.project),
      machine: decodeMachine(root.machine),
      model: decodeRef(root.model, 'model'),
      access: decodeRef(root.access, 'access'),
    }
  }

  throw new VinculoAmbienteError('binding_invalid_answer', 'state')
}
