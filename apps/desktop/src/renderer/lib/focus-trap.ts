// The edges of a modal's focus trap (Overlay's useModalFocus): where Tab wraps instead of leaving it.

/** Where each of the modal's Tab stops lies against the focused element, in document order. */
export type StopPlace = 'before' | 'at' | 'after'

/**
 * The end Tab (`shift`: Shift+Tab) wraps to, or `null` to let the browser move focus. The stops are
 * only controls with a tabindex of 0 or more, so a focused control outside that order (a radio
 * group's unchosen row, the card itself) wraps once no stop lies beyond it, like the last stop.
 */
export function trapWrap(stops: readonly StopPlace[], shift: boolean, inside: boolean): 'first' | 'last' | null {
  if (stops.length === 0) return null
  if (!inside) return shift ? 'last' : null
  if (shift) return stops.includes('before') ? null : 'last'
  return stops.includes('after') ? null : 'first'
}
