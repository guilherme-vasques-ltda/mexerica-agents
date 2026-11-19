import type { Entry } from '../../../../shared/models'

/** Vínculo de apresentação, sem inferir elegibilidade ou sucesso do turno. */
export function memoryResponse(notice: Entry, entries: Entry[]): Entry | undefined {
  if (notice.kind !== 'notice' || !notice.data.memory || notice.turn <= 0) return undefined
  const responseIds = notice.data.memory.responseIds
  const explicit = Array.isArray(responseIds) ? new Set(responseIds) : null
  // Campo explícito vazio não dá licença para adivinhar pelo turno legado.
  return entries.findLast((entry) => entry.kind === 'agent' && entry.botId === notice.botId && entry.turn === notice.turn
    && (explicit ? explicit.has(entry.id) : entry.data.final === true))
}

/** Move só recibos com alvo carregado; os demais itens mantêm a ordem original. */
export function placeMemoryNotices(entries: Entry[]): Entry[] {
  const anchored = new Map<string, Entry[]>()
  const moved = new Set<string>()
  for (const entry of entries) {
    const response = memoryResponse(entry, entries)
    if (!response) continue
    const notices = anchored.get(response.id) ?? []
    notices.push(entry)
    anchored.set(response.id, notices)
    moved.add(entry.id)
  }
  if (!moved.size) return entries
  return entries.flatMap((entry) => moved.has(entry.id) ? [] : [entry, ...(anchored.get(entry.id) ?? [])])
}
