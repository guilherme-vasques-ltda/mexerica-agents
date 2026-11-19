import { collapseKey } from '../../lib/bot-groups'
import { pref } from '../../lib/prefs'

// Preserva os pares até que cada computador esteja associado ao Hub e Space reais.
try {
  const previous = localStorage.getItem('donnabotCollapsedBotSections')
  if (previous !== null) {
    const old: unknown = JSON.parse(previous)
    const raw = localStorage.getItem('gruposRecolhidos')
    const current: unknown = raw === null ? [] : JSON.parse(raw)
    if (Array.isArray(old) && Array.isArray(current)) {
      const entries = [...current, ...old].filter((v): v is string => typeof v === 'string')
      localStorage.setItem('gruposRecolhidos', JSON.stringify([...new Set(entries)]))
    }
    localStorage.removeItem('donnabotCollapsedBotSections')
  }
} catch { /* armazenamento indisponível ou preferência inválida */ }
try { localStorage.removeItem('rosterBotSectionFilter') } catch { /* armazenamento indisponível */ }

export const gruposRecolhidos = pref<string[]>('gruposRecolhidos', [], (v): v is string[] => Array.isArray(v) && v.every((x) => typeof x === 'string'))

/** Migra apenas pares do computador atual; pares dos outros aguardam sua conexão. */
export function migrarRecolhimento(entries: string[], computerId: string, hubId: string, spaceId: string, groupIds: readonly string[]): string[] {
  let changed = false
  const mapped = entries.map((entry) => {
    try {
      const value: unknown = JSON.parse(entry)
      if (Array.isArray(value) && value.length === 2 && value[0] === computerId && typeof value[1] === 'string' && groupIds.includes(value[1])) {
        changed = true
        return collapseKey(hubId, spaceId, value[1])
      }
    } catch { /* entrada de outro formato permanece intacta */ }
    return entry
  })
  return changed ? [...new Set(mapped)] : entries
}
