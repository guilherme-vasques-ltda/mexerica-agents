import { createContext, useContext } from 'react'
import { type OnboardingStatusPayload } from '../../../../shared/fork-onboarding.ts'

export interface OnboardingControllerValue {
  status: OnboardingStatusPayload | null
  hasReadyAi: boolean
  connectAi: () => void
  openPermissions: (() => void) | null
  navigationKey: string
  computerId: string | null
  hubId: string | null
  spaceId: string | null
  firstChatDismissed: boolean
  dismissFirstChat: () => void
}

const defaultController: OnboardingControllerValue = {
  status: null,
  hasReadyAi: false,
  connectAi: () => {},
  openPermissions: null,
  navigationKey: '',
  computerId: null,
  hubId: null,
  spaceId: null,
  firstChatDismissed: false,
  dismissFirstChat: () => {},
}

export const OnboardingContext = createContext<OnboardingControllerValue>(defaultController)

export function useOnboarding() {
  return useContext(OnboardingContext)
}
