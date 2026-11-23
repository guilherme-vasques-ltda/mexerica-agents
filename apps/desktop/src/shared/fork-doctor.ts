// Contrato v1 (seção 3): parser fail-closed e tipos do diagnóstico somente leitura do Hub (`donnaHubDoctor`).
// Reutilizável pelo Desktop, Web e camadas compartilhadas, sem dependência de Electron.

export type DoctorOverallStatus = 'healthy' | 'waiting_user' | 'degraded' | 'error'
export type DoctorCheckStatus = 'ok' | 'warning' | 'error' | 'unavailable'
export type DoctorArea = 'hub' | 'ai_engine' | 'approvals' | 'channels' | 'mcps' | 'routines' | 'delegation'
export type DoctorActionKind =
  | 'none'
  | 'start_hub'
  | 'connect_ai'
  | 'review_waiting'
  | 'check_channels'
  | 'check_mcps'
  | 'check_routines'
  | 'open_torre'

export interface DoctorCheck {
  id: string
  area: DoctorArea
  status: DoctorCheckStatus
  code: string
  count: number | null
  actionKind: DoctorActionKind
}

export interface HubDoctorReport {
  version: 1
  hubId: string
  spaceId: string
  collectedAt: number
  overall: DoctorOverallStatus
  checks: DoctorCheck[]
}

export type DoctorErrorKind = 'hub_offline' | 'unsupported' | 'denied' | 'unavailable'

const OVERALL_STATUSES = new Set<DoctorOverallStatus>(['healthy', 'waiting_user', 'degraded', 'error'])
const CHECK_STATUSES = new Set<DoctorCheckStatus>(['ok', 'warning', 'error', 'unavailable'])
const AREAS = new Set<DoctorArea>(['hub', 'ai_engine', 'approvals', 'channels', 'mcps', 'routines', 'delegation'])
const ACTIONS = new Set<DoctorActionKind>([
  'none',
  'start_hub',
  'connect_ai',
  'review_waiting',
  'check_channels',
  'check_mcps',
  'check_routines',
  'open_torre',
])

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const nonEmpty = (v: unknown): v is string => typeof v === 'string' && v.trim().length > 0
const nonNegInt = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0
const timestamp = (v: unknown): v is number => nonNegInt(v) && v <= 8_640_000_000_000_000

function parseCheck(raw: unknown): DoctorCheck | null {
  if (!isObj(raw)) return null
  if (!nonEmpty(raw.id) || !nonEmpty(raw.code)) return null
  if (!AREAS.has(raw.area as DoctorArea)) return null
  if (!CHECK_STATUSES.has(raw.status as DoctorCheckStatus)) return null
  if (!ACTIONS.has(raw.actionKind as DoctorActionKind)) return null
  const count = raw.count === null ? null : nonNegInt(raw.count) ? raw.count : undefined
  if (count === undefined) return null
  return {
    id: raw.id,
    area: raw.area as DoctorArea,
    status: raw.status as DoctorCheckStatus,
    code: raw.code,
    count,
    actionKind: raw.actionKind as DoctorActionKind,
  }
}

/**
 * Parser fail-closed do `HubDoctorReport`.
 * Qualquer campo malformado, área desconhecida, duplicata de check ou mismatch de `hubId`/`spaceId`
 * retorna `null` (nunca degrada silenciosamente para «Tudo ok»).
 */
export function parseHubDoctorReport(
  raw: unknown,
  expected?: { hubId?: string; spaceId?: string },
): HubDoctorReport | null {
  if (!isObj(raw) || raw.version !== 1) return null
  if (!nonEmpty(raw.hubId) || !nonEmpty(raw.spaceId)) return null
  if (expected?.hubId && raw.hubId !== expected.hubId) return null
  if (expected?.spaceId && raw.spaceId !== expected.spaceId) return null
  if (!timestamp(raw.collectedAt)) return null
  if (!OVERALL_STATUSES.has(raw.overall as DoctorOverallStatus)) return null
  if (!Array.isArray(raw.checks) || raw.checks.length === 0) return null

  const checks: DoctorCheck[] = []
  const seen = new Set<string>()
  for (const entry of raw.checks) {
    const parsed = parseCheck(entry)
    if (!parsed || seen.has(parsed.id)) return null
    seen.add(parsed.id)
    checks.push(parsed)
  }

  return {
    version: 1,
    hubId: raw.hubId,
    spaceId: raw.spaceId,
    collectedAt: raw.collectedAt,
    overall: raw.overall as DoctorOverallStatus,
    checks,
  }
}

/** Classifica erro de chamada RPC do Doctor para distinguir Hub offline de Hub antigo (404) ou recusa (403). */
export function classifyDoctorError(error: unknown): DoctorErrorKind {
  const status = typeof error === 'object' && error && 'status' in error
    ? Number((error as { status: unknown }).status)
    : null
  const message = error instanceof Error
    ? error.message
    : typeof error === 'object' && error && 'message' in error
      ? String((error as { message: unknown }).message)
      : String(error)

  if (status === 404 || /unknown method/i.test(message)) return 'unsupported'
  if (status === 403 || status === 401) return 'denied'
  if (status !== null && Number.isFinite(status) && status >= 400) return 'unavailable'
  if (/invalid_json|doctor_invalid/i.test(message)) return 'unavailable'
  return 'hub_offline'
}
