import { useCallback, useState } from 'react'

type StarterTour = {
  scope: string | null
  starterDraft: { scope: string; prompt: string } | null
  spotlightOpen: boolean
}

export function useStarterTour(scope: string | null) {
  const [tour, setTour] = useState<StarterTour>(() => ({ scope, starterDraft: null, spotlightOpen: false }))
  if (tour.scope !== scope) setTour({ scope, starterDraft: null, spotlightOpen: false })
  const starterDraft = scope === tour.scope ? tour.starterDraft : null

  const pick = useCallback((prompt: string) => {
    if (scope === null) return
    setTour(current => current.scope === scope ? { scope, starterDraft: { scope, prompt }, spotlightOpen: false } : current)
  }, [scope])
  const handled = useCallback((applied: boolean) => {
    setTour(current => starterDraft && current.scope === scope && current.starterDraft === starterDraft
      ? { scope, starterDraft: null, spotlightOpen: applied } : current)
  }, [scope, starterDraft])
  const dismiss = useCallback(() => {
    setTour(current => current.scope === scope && current.spotlightOpen ? { ...current, spotlightOpen: false } : current)
  }, [scope])

  return {
    starterDraft,
    spotlightOpen: scope === tour.scope && tour.spotlightOpen,
    pick,
    handled,
    dismiss,
  }
}
