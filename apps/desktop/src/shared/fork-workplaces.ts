/** RPC privada do04. Nunca é parte de Hello, Mirror ou cache persistente. */
export type Axis = 'nao_verificado' | 'conferido' | 'indisponivel'
export type Account = 'assinatura_oficial' | 'api_nao_admitida' | 'desconectado' | 'nao_verificado' | 'indisponivel'
export interface Place {
  id: string; hubId: string; resourceId: string; name: string; kind: 'cloud'; provider: 'boat'
  revision: number; admissionRevision: number; connectionRevision: number; capacityRevision: number
  prepared: boolean; pinned: boolean; connected: boolean; helpers: 'indisponivel'
}
export interface Manifest {
  schema: 1; commit: string; sha256: string; size: number; os: 'linux'; arch: 'x86_64' | 'aarch64'
  libc: string; protocol: 1; journal: 1
  driver: { kind: 'indisponivel' } | { kind: 'codex_account_v1'; version: string; sha256: string }
}
export interface ConnectionPackage { sandbox_id: string; host: string; port: number; known_hosts_line: string }
export interface PublicMaterial {
  sshPublic: string; controlPublic: string; hubId: string; placeId: string; resourceId: string; runnerPort: 47631
  runnerPin: string | null; manifest: Manifest | null
}
export interface Operation {
  id: string; placeId: string; hubId: string; admissionRevision: number; connectionRevision: number
  authority: 'local'; intent: string; deadline: number; observedAt: number | null
  phase: 'verificando' | 'concluida' | 'falhou' | 'cancelada_incerta' | 'interrompida_incerta'
  reason: string | null
  observation: { connection: Axis; identity: Axis; preparation: Axis; account: Account; helpers: 'indisponivel' }
}
export type Intent =
  | { type: 'create'; name: string; resource_id: string }
  | { type: 'rename'; name: string }
  | { type: 'prepare_connection'; rotate: boolean }
  | { type: 'pin_runner'; pin: string; manifest: Manifest; rotate: boolean }
  | { type: 'refix'; package: ConnectionPackage }
  | { type: 'inspect_start' } | { type: 'remove' }
export const methods: Record<Intent['type'], string> = {
  create: 'donnaWorkplaceCreate', rename: 'donnaWorkplaceRename', prepare_connection: 'donnaWorkplacePrepareConnection',
  pin_runner: 'donnaWorkplacePinRunner', refix: 'donnaWorkplaceRefix', inspect_start: 'donnaWorkplaceInspectStart', remove: 'donnaWorkplaceRemove',
}
export interface Reply {
  hubId: string; available?: true; origin?: 'local'; helpers?: 'indisponivel'
  places?: Place[]; place?: Place; publicMaterial?: PublicMaterial; operation?: Operation; operations?: Operation[]; removed?: boolean
}
export interface WorkplaceGuide { schema: 1; hubVersion: string; text: string }
type ObjectValue = Record<string, unknown>
function object(v: unknown): ObjectValue {
  if (!v || typeof v !== 'object' || Array.isArray(v)) throw Error('workplace_response_invalid')
  return v as ObjectValue
}
function keys(o: ObjectValue, allowed: string[]) {
  if (Object.keys(o).some((k) => !allowed.includes(k))) throw Error('workplace_response_invalid')
}
function text(v: unknown): string {
  if (typeof v !== 'string' || !v || v.length > 4096 || /[\r\n\x00]/.test(v)) throw Error('workplace_response_invalid')
  return v
}
function number(v: unknown): number {
  if (typeof v !== 'number' || !Number.isSafeInteger(v) || v < 0) throw Error('workplace_response_invalid')
  return v
}
function boolean(v: unknown): boolean { if (typeof v !== 'boolean') throw Error('workplace_response_invalid'); return v }
function oneOf<T extends string>(v: unknown, options: readonly T[]): T {
  if (!options.includes(v as T)) throw Error('workplace_response_invalid'); return v as T
}
function same(actual: unknown, expected: string) { if (actual !== expected) throw Error('workplace_hub_changed') }
export function decodeGuideReply(value: unknown, hub: string): WorkplaceGuide {
  const reply = object(value)
  keys(reply, ['hubId', 'guide'])
  same(reply.hubId, hub)
  const guide = object(reply.guide)
  keys(guide, ['schema', 'hubVersion', 'text'])
  const hubVersion = text(guide.hubVersion)
  if (guide.schema !== 1 || !hubVersion.trim() || hubVersion.length > 80 || typeof guide.text !== 'string'
    || !guide.text.trim() || guide.text.includes('\0') || new TextEncoder().encode(guide.text).length > 65_536) throw Error('workplace_response_invalid')
  return { schema: 1, hubVersion, text: guide.text }
}
export function decodeManifest(value: unknown): Manifest {
  const o = object(value)
  keys(o, ['schema', 'commit', 'sha256', 'size', 'os', 'arch', 'libc', 'protocol', 'journal', 'driver'])
  if (o.schema !== 1 || o.protocol !== 1 || o.journal !== 1 || o.os !== 'linux' || !/^[a-f0-9]{40}$/.test(text(o.commit)) || !/^[a-f0-9]{64}$/.test(text(o.sha256))) throw Error('artifact_invalid')
  const d = object(o.driver)
  const kind = oneOf(d.kind, ['indisponivel', 'codex_account_v1'])
  keys(d, kind === 'indisponivel' ? ['kind'] : ['kind', 'version', 'sha256'])
  if (kind === 'codex_account_v1' && !/^[a-f0-9]{64}$/.test(text(d.sha256))) throw Error('artifact_invalid')
  const size = number(o.size)
  if (!size) throw Error('artifact_invalid')
  return { schema: 1, commit: text(o.commit), sha256: text(o.sha256), size, os: 'linux', arch: oneOf(o.arch, ['x86_64', 'aarch64']), libc: text(o.libc), protocol: 1, journal: 1,
    driver: kind === 'indisponivel' ? { kind } : { kind, version: text(d.version), sha256: text(d.sha256) } }
}
function decodePlace(value: unknown, hub: string): Place {
  const o = object(value)
  keys(o, ['id', 'hubId', 'resourceId', 'name', 'kind', 'provider', 'revision', 'admissionRevision', 'connectionRevision', 'capacityRevision', 'prepared', 'pinned', 'connected', 'helpers'])
  same(o.hubId, hub)
  return { id: text(o.id), hubId: hub, resourceId: text(o.resourceId), name: text(o.name), kind: oneOf(o.kind, ['cloud']), provider: oneOf(o.provider, ['boat']),
    revision: number(o.revision), admissionRevision: number(o.admissionRevision), connectionRevision: number(o.connectionRevision), capacityRevision: number(o.capacityRevision),
    prepared: boolean(o.prepared), pinned: boolean(o.pinned), connected: boolean(o.connected), helpers: oneOf(o.helpers, ['indisponivel']) }
}
function decodeOperation(value: unknown, hub: string): Operation {
  const o = object(value)
  keys(o, ['id', 'placeId', 'hubId', 'admissionRevision', 'connectionRevision', 'authority', 'intent', 'deadline', 'observedAt', 'phase', 'reason', 'observation'])
  same(o.hubId, hub)
  const a = object(o.observation)
  keys(a, ['connection', 'identity', 'preparation', 'account', 'helpers'])
  const axes = ['nao_verificado', 'conferido', 'indisponivel'] as const
  return { id: text(o.id), placeId: text(o.placeId), hubId: hub, admissionRevision: number(o.admissionRevision), connectionRevision: number(o.connectionRevision),
    authority: oneOf(o.authority, ['local']), intent: text(o.intent), deadline: number(o.deadline), observedAt: o.observedAt === null ? null : number(o.observedAt),
    phase: oneOf(o.phase, ['verificando', 'concluida', 'falhou', 'cancelada_incerta', 'interrompida_incerta']), reason: o.reason === null ? null : text(o.reason),
    observation: { connection: oneOf(a.connection, axes), identity: oneOf(a.identity, axes), preparation: oneOf(a.preparation, axes),
      account: oneOf(a.account, ['assinatura_oficial', 'api_nao_admitida', 'desconectado', 'nao_verificado', 'indisponivel']), helpers: oneOf(a.helpers, ['indisponivel']) } }
}
export function decodeReply(value: unknown, hub: string): Reply {
  const o = object(value)
  keys(o, ['hubId', 'available', 'origin', 'helpers', 'places', 'place', 'publicMaterial', 'operation', 'operations', 'removed'])
  same(o.hubId, hub)
  const reply: Reply = { hubId: hub }
  if (o.available !== undefined) {
    if (o.available !== true || o.origin !== 'local' || o.helpers !== 'indisponivel') throw Error('workplace_response_invalid')
    reply.available = true; reply.origin = 'local'; reply.helpers = 'indisponivel'
  }
  if (o.places !== undefined) {
    if (!Array.isArray(o.places)) throw Error('workplace_response_invalid')
    reply.places = o.places.map((p) => decodePlace(p, hub))
  }
  if (o.place !== undefined) reply.place = decodePlace(o.place, hub)
  if (o.operation !== undefined) reply.operation = decodeOperation(o.operation, hub)
  if (o.operations !== undefined) {
    if (!Array.isArray(o.operations)) throw Error('workplace_response_invalid')
    reply.operations = o.operations.map((op) => decodeOperation(op, hub))
  }
  if (o.removed !== undefined) reply.removed = boolean(o.removed)
  if (o.publicMaterial !== undefined) {
    const m = object(o.publicMaterial)
    keys(m, ['sshPublic', 'controlPublic', 'hubId', 'placeId', 'resourceId', 'runnerPort', 'runnerPin', 'manifest'])
    same(m.hubId, hub)
    if (m.runnerPort !== 47631 || !text(m.sshPublic).startsWith('ssh-ed25519 ') || !/^[A-Za-z0-9_-]{43}$/.test(text(m.controlPublic))) throw Error('workplace_response_invalid')
    if (!reply.place || reply.place.id !== m.placeId || reply.place.resourceId !== m.resourceId) throw Error('workplace_response_invalid')
    reply.publicMaterial = { sshPublic: text(m.sshPublic), controlPublic: text(m.controlPublic), hubId: hub, placeId: text(m.placeId), resourceId: text(m.resourceId), runnerPort: 47631,
      runnerPin: m.runnerPin === null ? null : text(m.runnerPin), manifest: m.manifest === null ? null : decodeManifest(m.manifest) }
  }
  return reply
}
