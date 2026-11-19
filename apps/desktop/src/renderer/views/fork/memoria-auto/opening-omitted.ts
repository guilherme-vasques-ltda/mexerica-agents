import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

export type OpeningOmittedReason = 'overBudget' | 'pinnedLimit' | 'scope'
export type OpeningOmittedLayer = 'yours' | 'bot'

/** Uma memória que ficou de fora da última abertura renderizada. Sem conteúdo: id, camada, motivo e data. */
export interface MemoryOpeningOmitted {
  entry: number
  layer: OpeningOmittedLayer
  reason: OpeningOmittedReason
  at: number
}

export type OpeningOmittedHistory = { state: 'loading' | 'unknown' | 'unavailable' } | { state: 'ready'; rows: MemoryOpeningOmitted[] }

const reasons: Record<OpeningOmittedReason, true> = { overBudget: true, pinnedLimit: true, scope: true }
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/** Não inventa linhas para campo ausente nem perdoa enum ou id fora do formato. O Hub corta em 500. */
export function readOpeningOmitted(value: unknown): OpeningOmittedHistory {
  if (value === undefined) return { state: 'unavailable' }
  if (!Array.isArray(value) || value.length > 500) return { state: 'unknown' }
  const rows: MemoryOpeningOmitted[] = []
  const seen = new Set<string>()
  for (const row of value) {
    if (!object(row)) continue
    if (row.layer !== 'yours' && row.layer !== 'bot') continue
    if (typeof row.reason !== 'string' || !Object.hasOwn(reasons, row.reason)) continue
    if (typeof row.entry !== 'number' || !Number.isSafeInteger(row.entry) || row.entry <= 0) continue
    if (typeof row.at !== 'number' || !Number.isSafeInteger(row.at) || row.at < 0 || row.at > 8.64e15) continue
    const key = `${row.layer}:${row.entry}`
    if (seen.has(key)) continue
    seen.add(key)
    rows.push({ entry: row.entry, layer: row.layer, reason: row.reason as OpeningOmittedReason, at: row.at })
  }
  return { state: 'ready', rows }
}

/** O motivo como chave do catálogo; a tela traduz ao desenhar. */
export const openingOmittedReason: Record<OpeningOmittedReason, ChaveTexto> = {
  overBudget: 'memoria.painel.foraAbertura.motivo.overBudget',
  pinnedLimit: 'memoria.painel.foraAbertura.motivo.pinnedLimit',
  scope: 'memoria.painel.foraAbertura.motivo.scope',
}
