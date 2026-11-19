// Keyboard for a radio group (ChoiceList), as in the WAI-ARIA radio group: one Tab stop, arrows move
// and choose, wrapping at the ends. Home and End are not part of it.

/** The row Tab lands on: the chosen one, or the first when no row holds `selection` (-1: no rows). */
export function radioTabStop(ids: readonly string[], selection: string): number {
  const chosen = ids.indexOf(selection)
  return chosen >= 0 ? chosen : ids.length > 0 ? 0 : -1
}

/** The keys a radio group answers: ↓/→ and ↑/← without modifiers, which stay the view's. */
export interface RadioKey {
  key: string
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
}

/** The row an arrow takes the group to from row `index` of `count`; `null`: a key the group leaves alone. */
export function radioArrowTarget(e: RadioKey, index: number, count: number): number | null {
  if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || count === 0) return null
  if (e.key === 'ArrowDown' || e.key === 'ArrowRight') return (index + 1) % count
  if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') return (index - 1 + count) % count
  return null
}
