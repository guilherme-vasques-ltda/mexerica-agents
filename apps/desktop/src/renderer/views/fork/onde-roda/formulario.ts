import type { Intent, Place, PublicMaterial } from '../../../../shared/fork-workplaces.ts'

export interface PlaceDraft {
  target: Place
  material: PublicMaterial | null
  name: string
  pin: string
  manifest: string
  host: string
  port: string
  known: string
  confirmation: { intent: Intent; target: Place } | null
}
export function createPlaceDraft(place: Place): PlaceDraft {
  return { target: { ...place }, material: null, name: place.name, pin: '', manifest: '', host: '', port: '22', known: '', confirmation: null }
}
/** Reconciliar uma resposta não remonta o formulário nem apaga campos de outro passo. */
export function reconcilePlaceDraft(draft: PlaceDraft, place: Place, material: PublicMaterial | null): PlaceDraft {
  if (draft.target.id !== place.id || draft.target.hubId !== place.hubId) return createPlaceDraft(place)
  const metadataChanged = draft.target.revision !== place.revision
  const admissionChanged = draft.target.admissionRevision !== place.admissionRevision
  const connectionChanged = draft.target.connectionRevision !== place.connectionRevision
  const next = { ...draft, target: { ...place }, material }
  if (metadataChanged) next.name = place.name
  if (admissionChanged) { next.pin = ''; next.manifest = '' }
  if (connectionChanged) { next.host = ''; next.port = '22'; next.known = '' }
  if (metadataChanged || admissionChanged || connectionChanged) next.confirmation = null
  if (material && material.placeId === place.id && material.hubId === place.hubId && material !== draft.material) {
    next.pin = material.runnerPin ?? ''
    next.manifest = material.manifest ? JSON.stringify(material.manifest, null, 2) : ''
  }
  return next
}
