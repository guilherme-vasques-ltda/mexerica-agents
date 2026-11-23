import { useCallback, useEffect, useRef, useState } from 'react'
import type { ComposerDrafts, DraftFile } from '../../../../shared/composer-drafts.ts'
import type { OriginDraft } from '../../../../shared/guard-hub.ts'

type Applied = { origin: OriginDraft; files: DraftFile[] }
export type StarterDraft = { prompt: string; previous: () => OriginDraft | undefined }

type StarterTour = {
  scope: string | null
  paused: boolean
  drafts: ComposerDrafts
  botId: string
  starterDraft: StarterDraft | null
  applied: Applied | null
  spotlightOpen: boolean
}

export function useStarterTour(scope: string | null, paused: boolean, drafts: ComposerDrafts, botId: string) {
  const [tour, setTour] = useState<StarterTour>(() => ({ scope, paused, drafts, botId, starterDraft: null, applied: null, spotlightOpen: false }))
  const same = scope === tour.scope && drafts === tour.drafts && botId === tour.botId
  if (!same) setTour({ scope, paused, drafts, botId, starterDraft: null, applied: null, spotlightOpen: false })
  else if (tour.paused !== paused) setTour({ ...tour, paused, starterDraft: null })
  const starterDraft = same && !paused && !tour.paused ? tour.starterDraft : null
  const appliedRef = useRef<Applied | null>(null)
  const valid = useCallback((applied: Applied | null) => applied !== null && drafts.origin(botId) === applied.origin && drafts.files(botId) === applied.files, [drafts, botId])
  useEffect(() => {
    const invalidate = () => {
      const applied = appliedRef.current
      if (!applied || valid(applied)) return
      appliedRef.current = null
      setTour(current => current.applied === applied ? { ...current, applied: null, spotlightOpen: false } : current)
    }
    const stop = drafts.subscribe(invalidate)
    invalidate()
    return stop
  }, [drafts, valid])

  const pick = useCallback((prompt: string) => {
    if (scope === null || paused) return
    const applied = appliedRef.current
    setTour(current => current.scope === scope && current.drafts === drafts && current.botId === botId && !current.paused
      ? { ...current, starterDraft: { prompt, previous: () => applied === appliedRef.current && applied === current.applied && valid(applied) ? applied?.origin : undefined }, spotlightOpen: false } : current)
  }, [scope, paused, drafts, botId, valid])
  const handled = useCallback((origin?: OriginDraft) => {
    if (!starterDraft) return
    const applied = origin && drafts.origin(botId) === origin ? { origin, files: drafts.files(botId) } : null
    if (applied) appliedRef.current = applied
    setTour(current => starterDraft && current.scope === scope && !current.paused && current.starterDraft === starterDraft
      ? { ...current, starterDraft: null, applied: applied ?? current.applied, spotlightOpen: applied !== null } : current)
  }, [scope, starterDraft, drafts, botId])
  const dismiss = useCallback(() => {
    setTour(current => current.scope === scope && current.spotlightOpen ? { ...current, spotlightOpen: false } : current)
  }, [scope])

  return {
    starterDraft,
    spotlightOpen: same && !paused && tour.spotlightOpen && valid(tour.applied),
    pick,
    handled,
    dismiss,
  }
}
