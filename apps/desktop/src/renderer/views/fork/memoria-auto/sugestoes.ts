// D389 `suggest`: o que um bot quer guardar e ainda não está na memória. Sem DOM nem React.
import type { MemoryNature, MemoryOrigin } from '../../../../shared/models'

/** `confirm`: o sono achou a inferência, letra por letra, numa mensagem limpa sua (2b1c). `reclassify`: o modelo achou
 * a inferência sustentada por uma mensagem sua; aprovada, fica "Confirmada pela sua mensagem" (2b1d-2). */
export type SuggestionAction = 'remember' | 'revise' | 'archive' | 'note' | 'confirm' | 'reclassify'
export interface MemorySuggestion {
  id: string
  action: SuggestionAction
  /** Texto proposto; na nota, o conteúdo novo (`null` = remover a nota). */
  text: string | null
  file: string | null
  shared: boolean
  origin: MemoryOrigin | null
  nature: MemoryNature
  createdAt: number
}
export type SuggestionOutcome = 'approved' | 'discarded' | 'noChange' | 'stale' | 'error'

const ACTIONS: readonly string[] = ['remember', 'revise', 'archive', 'note', 'confirm', 'reclassify']

/** `null`: computador sem a lista (versão anterior); itens fora do contrato são omitidos, nunca inventados. */
export function readSuggestions(value: unknown): MemorySuggestion[] | null {
  if (!Array.isArray(value)) return null
  return value.flatMap((row): MemorySuggestion[] => {
    if (!row || typeof row !== 'object') return []
    const r = row as Record<string, unknown>
    if (typeof r.id !== 'string' || !r.id || typeof r.action !== 'string' || !ACTIONS.includes(r.action)) return []
    if (r.text !== null && typeof r.text !== 'string') return []
    return [{
      id: r.id,
      action: r.action as SuggestionAction,
      text: (r.text as string | null) ?? null,
      file: typeof r.file === 'string' ? r.file : null,
      shared: r.shared === true,
      origin: (r.origin as MemoryOrigin | null) ?? null,
      nature: (typeof r.nature === 'string' ? r.nature : 'unknown') as MemoryNature,
      createdAt: typeof r.createdAt === 'number' ? r.createdAt : 0,
    }]
  })
}

/** O Hub responde `status`; a recusa por memória alterada vem como código estável. */
export function suggestionOutcome(approve: boolean, result: unknown, error?: unknown): SuggestionOutcome {
  if (error !== undefined) return String((error as Error)?.message ?? error).includes('memory_suggestion_stale') ? 'stale' : 'error'
  const status = (result as { status?: unknown } | null)?.status
  if (status === 'stale') return 'stale'
  if (approve) return status === 'approved' ? 'approved' : status === 'noChange' ? 'noChange' : 'error'
  return status === 'discarded' ? 'discarded' : 'error'
}
