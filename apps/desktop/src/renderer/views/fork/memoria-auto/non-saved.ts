import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
export type NonSavedReason = 'duplicate' | 'wrongBot' | 'notDurable' | 'technicalDeferred' | 'full' | 'expired' | 'noSelection' | 'ineligible'
export type NonSavedStatus = 'notSaved' | 'deferred'
export type NonSavedAvailability = 'transient' | 'full' | 'expired' | 'noSelection' | 'ineligible' | null

/** Metadados escopados pelo Hub. Não contêm candidato, erro livre ou memória alvo. */
export interface MemoryNonSaved {
  id: string
  jobId: string
  reason: NonSavedReason
  status: NonSavedStatus
  at: number
  messageIds: string[]
  responseIds: string[]
  availability: NonSavedAvailability
}

/** A projeção visual não transporta referências opacas nem campos livres. */
export type NonSavedRow = Pick<MemoryNonSaved, 'id' | 'reason' | 'status' | 'at'>
export type NonSavedHistory = { state: 'loading' | 'unknown' | 'unavailable' } | { state: 'ready'; rows: NonSavedRow[] }

const disposition: Record<NonSavedReason, readonly [NonSavedStatus, NonSavedAvailability]> = {
  duplicate: ['notSaved', null], wrongBot: ['notSaved', null], notDurable: ['notSaved', null],
  technicalDeferred: ['deferred', 'transient'], noSelection: ['deferred', 'noSelection'],
  full: ['notSaved', 'full'], expired: ['notSaved', 'expired'], ineligible: ['notSaved', 'ineligible'],
}
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const ids = (v: unknown): v is string[] => Array.isArray(v) && v.every((id) => typeof id === 'string' && !!id)

/** Não infere histórico de automatic/Entry nem corrige enum ou combinação incompatível. */
export function readNonSaved(value: unknown): NonSavedHistory {
  if (value === undefined) return { state: 'unavailable' }
  if (!Array.isArray(value) || value.length > 50) return { state: 'unknown' }
  const rows: NonSavedRow[] = []
  const seen = new Set<string>()
  for (const row of value) {
    if (!object(row) || typeof row.reason !== 'string' || !Object.hasOwn(disposition, row.reason)) continue
    const reason = row.reason as NonSavedReason
    const [status, availability] = disposition[reason]
    if (row.status !== status || row.availability !== availability) continue
    if (typeof row.id !== 'string' || !row.id || seen.has(row.id) || typeof row.jobId !== 'string' || !row.jobId) continue
    if (!ids(row.messageIds) || !ids(row.responseIds) || typeof row.at !== 'number' || !Number.isSafeInteger(row.at) || row.at < 0 || row.at > 8.64e15) continue
    seen.add(row.id)
    rows.push({ id: row.id, reason, status, at: row.at })
  }
  return { state: 'ready', rows }
}

/** Fatia 7 (D454): o motivo como chave do catálogo; a tela traduz ao desenhar. */
export const nonSavedReason: Record<NonSavedReason, ChaveTexto> = {
  duplicate: 'memoria.auto.motivo.duplicate',
  wrongBot: 'memoria.auto.motivo.wrongBot',
  notDurable: 'memoria.auto.motivo.notDurable',
  technicalDeferred: 'memoria.auto.motivo.technicalDeferred',
  full: 'memoria.auto.motivo.full',
  expired: 'memoria.auto.motivo.expired',
  noSelection: 'memoria.auto.motivo.noSelection',
  ineligible: 'memoria.auto.motivo.ineligible',
}
