// Donnabot: busca por texto da tela "Rotinas" (D85) e da lista de bots da barra lateral. Sem
// acento e sem caixa: "financas" acha "Finanças". Todas as palavras precisam aparecer.

/** O valor de "todos" nos filtros de Time e de bot. */
export const ALL = 'all'

export function normalizeSearch(text: string): string {
  return text.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLocaleLowerCase('pt-BR')
}

/** As palavras de uma busca, já normalizadas (nenhuma: mostra tudo). */
export function searchWords(query: string): string[] {
  return normalizeSearch(query).split(/\s+/).filter(Boolean)
}

/** Cada palavra aparece em algum dos campos. */
export function matchesWords(words: readonly string[], fields: readonly string[]): boolean {
  if (!words.length) return true
  const text = normalizeSearch(fields.join(' '))
  return words.every((word) => text.includes(word))
}

export interface SearchableRoutine {
  botId: string
  name: string
  teamId: string
}

export interface SearchableTeam {
  id: string
  name: string
  botIds: string[]
}

export interface RoutineFilters {
  teamId: string
  botId: string
  /** "Buscar rotinas": o nome da rotina, do bot ou do Time. */
  query: string
}

/** As rotinas que os filtros de Time e de bot e a busca deixam ver. */
export function filterRoutines<R extends SearchableRoutine>(
  routines: readonly R[],
  teams: readonly SearchableTeam[],
  filters: RoutineFilters,
  botName: (botId: string) => string,
): R[] {
  const team = teams.find((t) => t.id === filters.teamId)
  const words = searchWords(filters.query)
  return routines.filter((routine) =>
    (filters.teamId === ALL || (team ? team.botIds.includes(routine.botId) : routine.teamId === filters.teamId))
    && (filters.botId === ALL || routine.botId === filters.botId)
    && matchesWords(words, [routine.name, botName(routine.botId), teams.find((t) => t.id === routine.teamId)?.name ?? '']))
}

/**
 * A nova ordem de uma parte da lista (a que a busca deixou ver) dentro da lista toda: as linhas
 * reordenadas trocam de lugar entre si e as outras ficam onde estavam.
 */
export function mergeOrder(all: readonly string[], reordered: readonly string[]): string[] {
  const moved = new Set(reordered)
  let next = 0
  return all.map((id) => (moved.has(id) && next < reordered.length ? reordered[next++]! : id))
}
