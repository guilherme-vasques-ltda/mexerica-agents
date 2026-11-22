import { createContext, useContext } from 'react'
import { PERSONAL_SPACE_ID, type OnboardingStatusPayload } from '../../../../shared/fork-onboarding.ts'

export interface OnboardingControllerValue {
  status: OnboardingStatusPayload | null
  hasReadyAi: boolean
  openStep3: () => void
  openPermissions: (() => void) | null
  markStep4Completed: () => void
  storageKey: string
  computerId: string | null
  hubId: string | null
  spaceId: string | null
}

const defaultController: OnboardingControllerValue = {
  status: null,
  hasReadyAi: false,
  openStep3: () => {},
  openPermissions: null,
  markStep4Completed: () => {},
  storageKey: `mexerica.onboarding.ui.v1.local.${PERSONAL_SPACE_ID}`,
  computerId: null,
  hubId: null,
  spaceId: null,
}

export const OnboardingContext = createContext<OnboardingControllerValue>(defaultController)

export function useOnboarding() {
  return useContext(OnboardingContext)
}
