import type { Place } from '../../../../shared/fork-workplaces.ts'

interface FocusDocument {
  readonly activeElement: unknown
  readonly body: unknown
  readonly documentElement: unknown
  addEventListener(type: 'keydown' | 'pointerdown', listener: () => void, capture: boolean): void
  removeEventListener(type: 'keydown' | 'pointerdown', listener: () => void, capture: boolean): void
}
interface FocusTarget { readonly isConnected: boolean; focus(options: { preventScroll: true }): void }
export interface FocusScope {
  generation: number; hubId: string; placeId: string; resourceId: string
  revision: number; admissionRevision: number; connectionRevision: number
}
export function placeFocusScope(place: Place, generation: number): FocusScope {
  return { generation, hubId: place.hubId, placeId: place.id, resourceId: place.resourceId,
    revision: place.revision, admissionRevision: place.admissionRevision, connectionRevision: place.connectionRevision }
}
export interface StepFocus {
  active(): boolean
  cancel(): void
  restore(target: FocusTarget | null, expected: FocusScope): boolean
}
/** A desativação automática pode deixar BODY; Tab/clique durante await cancela a restauração. */
export function trackStepFocus(document: FocusDocument, origin: FocusScope, current: () => FocusScope | null): StepFocus {
  const previous = document.activeElement
  let interacted = false
  let closed = false
  const interaction = () => { interacted = true }
  document.addEventListener('keydown', interaction, true)
  document.addEventListener('pointerdown', interaction, true)
  const cancel = () => {
    closed = true
    document.removeEventListener('keydown', interaction, true)
    document.removeEventListener('pointerdown', interaction, true)
  }
  return {
    active: () => !closed,
    cancel,
    restore(target, expected) {
      const actual = current()
      const sameOrigin = actual && actual.generation === origin.generation && expected.generation === origin.generation
        && actual.hubId === origin.hubId && expected.hubId === origin.hubId
        && actual.placeId === origin.placeId && expected.placeId === origin.placeId
        && actual.resourceId === origin.resourceId && expected.resourceId === origin.resourceId
      const sameTarget = actual && actual.revision === expected.revision
        && actual.admissionRevision === expected.admissionRevision && actual.connectionRevision === expected.connectionRevision
      const focusUnchanged = document.activeElement === previous || document.activeElement === document.body || document.activeElement === document.documentElement
      const restore = !closed && !interacted && sameOrigin && sameTarget && focusUnchanged && target?.isConnected === true
      cancel()
      if (restore && target) { target.focus({ preventScroll: true }); return true }
      return false
    },
  }
}
