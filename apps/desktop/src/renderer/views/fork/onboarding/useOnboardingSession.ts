import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  isLocalPersonalContext,
  onboardingNavigationKey,
  PERSONAL_SPACE_ID,
  type OnboardingStatusPayload,
  type WhereChoice,
} from '../../../../shared/fork-onboarding.ts'
import type { HostClient } from '../../../client/host-client'
import { useAccount } from '../../../store/account'
import type { AppModel } from '../../../store/app-model'
import { useModel, useSelector } from '../../../lib/observable'
import { liveClient } from '../../settings/parts'
import { fetchOnboardingStatus } from './adapter'
import type { OnboardingControllerValue } from './OnboardingContext'
import { aoMudarChaves } from '../contas-ia/api-direta'
import { settingsScreen } from '../ajustes/tela'
import { esquecerModelos } from '../seletor-modelo/catalogo'

export function useOnboardingSession(
  app: AppModel,
  hostState: AppModel['host']['state'],
  allowLocalInstallGuide: boolean,
  openPermissions: (() => void) | null,
) {
  const account = useAccount()
  const accountUserId = account.isReady && !account.isBusy && !account.isSigningOut && !account.isExitUnresolved
    ? account.user?.userId ?? null : null
  const revision = account.sessionRevision
  const activeSpace = app.spaceMirrors.active
  const activeSpaceId = activeSpace && activeSpace.computerId === app.local?.computer.id ? activeSpace.spaceId : null
  const isPersonalContext = isLocalPersonalContext({
    activeComputerId: activeSpace?.computerId ?? null,
    localComputerId: app.local?.computer.id ?? null,
    activeSpaceId,
  }) && (!app.selection || app.selection.computerId === app.local?.computer.id)
  const spaceId = activeSpaceId ?? PERSONAL_SPACE_ID
  const activeStore = useModel(app.local)
  const navigationKey = onboardingNavigationKey(revision, accountUserId, activeSpace?.computerId ?? activeStore?.computer.id ?? null, activeStore?.hello?.hostId ?? null, spaceId)
  const [choices, setChoices] = useState<Record<string, WhereChoice>>({})
  const whereChoice = choices[navigationKey] ?? 'local'

  const screenEpoch = activeStore?.screenEpoch
  const bergaId = useSelector(activeStore?.principal, (principal) => principal?.id)
  const berga = bergaId ? activeStore?.bots.get(bergaId) : undefined
  const engine = useSelector(activeStore?.sessions, (sessions) => {
    const engine = bergaId ? sessions?.of(bergaId)?.find((session) => session.principal)?.engine : null
    return engine ? JSON.stringify(engine) : null
  })
  const localClient: HostClient | null = activeStore && !activeStore.mismatch ? liveClient(activeStore) : null
  const client = whereChoice === 'local' && isPersonalContext ? localClient : null

  const [status, setStatus] = useState<{ scope: object; value: OnboardingStatusPayload } | null>(null)
  const [explicitStep3, setExplicitStep3] = useState(false)
  const [dismissedInSession, setDismissedInSession] = useState(false)
  const [step3Held, setStep3Held] = useState(false)
  const [dismissal, setDismissal] = useState<{ userId: string; revision: number; deferred: boolean } | null>(null)
  const loadGen = useRef(0)
  const bridge = window.codync.host.onboardingDismissal
  const marker = accountUserId && dismissal?.userId === accountUserId && dismissal.revision === revision ? dismissal : null
  const dismissalScope = accountUserId ? JSON.stringify([accountUserId, revision]) : null
  const currentDismissalScope = useRef(dismissalScope)
  currentDismissalScope.current = dismissalScope
  const loadScope = useMemo(() => ({}), [localClient, isPersonalContext, navigationKey, revision, dismissalScope])
  const currentLoadScope = useRef(loadScope)
  currentLoadScope.current = loadScope
  const visibleStatus = isPersonalContext && status?.scope === loadScope ? status.value : null

  // Isola o estado visual ao trocar conta ou space sem reaproveitar rascunho de outra sessao.
  useEffect(() => {
    setStatus(null)
    setExplicitStep3(false)
    setDismissedInSession(false)
    setStep3Held(false)
  }, [navigationKey])

  useEffect(() => {
    let active = true
    if (accountUserId && bridge) {
      void bridge(accountUserId, 'read').catch(() => null).then((deferred) => {
        if (deferred !== null && active && currentDismissalScope.current === dismissalScope) {
          setDismissal({ userId: accountUserId, revision, deferred })
        }
      })
    }
    return () => { active = false }
  }, [accountUserId, revision, bridge, dismissalScope])

  useEffect(() => {
    loadGen.current += 1
    setStatus(null)
  }, [localClient, isPersonalContext, navigationKey])

  const reload = useCallback(async () => {
    if (!localClient || !isPersonalContext) return
    const gen = ++loadGen.current
    try {
      const next = await fetchOnboardingStatus(localClient, spaceId)
      if (gen === loadGen.current && currentLoadScope.current === loadScope) setStatus({ scope: loadScope, value: next })
    } catch {
      // Mantem ultimo estado real conhecido; nunca fabrica prontidao.
    }
  }, [localClient, isPersonalContext, spaceId, loadScope])

  useEffect(() => {
    void reload()
  }, [reload, screenEpoch, bergaId, berga?.backend, berga?.model, engine])

  useEffect(() => {
    if (!localClient || !isPersonalContext) return
    const onChange = () => { if (currentLoadScope.current === loadScope) void reload() }
    const stopKeys = aoMudarChaves(() => {
      if (activeStore && currentLoadScope.current === loadScope) esquecerModelos(activeStore.computer.id, 'mexerica')
      onChange()
    })
    let wasOpen = settingsScreen.open
    const stopSettings = settingsScreen.subscribe(() => {
      const closed = wasOpen && !settingsScreen.open
      wasOpen = settingsScreen.open
      if (closed) onChange()
    })
    return () => { stopKeys(); stopSettings() }
  }, [activeStore, localClient, isPersonalContext, loadScope, reload])

  const dismissalLoaded = !accountUserId || !bridge || marker !== null
  const autoStep3Pending = dismissalLoaded && !marker?.deferred && visibleStatus !== null && !visibleStatus.ai.hasReadyAi && !dismissedInSession

  useEffect(() => {
    if (autoStep3Pending) setStep3Held(true)
  }, [autoStep3Pending])

  const localInstallPending = dismissalLoaded && !marker?.deferred && allowLocalInstallGuide && app.computers.length === 0 && !localClient && !dismissedInSession
    && ['notInstalled', 'missingBinary', 'failed', 'starting'].includes(hostState.kind)
  const showStep3 = isPersonalContext && (explicitStep3 || (!marker?.deferred && step3Held) || localInstallPending || autoStep3Pending)

  const closeStep3 = useCallback(
    () => {
      setExplicitStep3(false)
      setDismissedInSession(true)
      setStep3Held(false)
      if (isPersonalContext && accountUserId && bridge) {
        void bridge(accountUserId, 'defer').then((deferred) => {
          if (deferred === true && currentDismissalScope.current === dismissalScope) {
            setDismissal({ userId: accountUserId, revision, deferred })
          }
        }).catch(() => {})
      }
    },
    [isPersonalContext, accountUserId, bridge, revision, dismissalScope],
  )

  const controller: OnboardingControllerValue = useMemo(
    () => ({
      status: visibleStatus,
      hasReadyAi: visibleStatus?.ai.hasReadyAi ?? false,
      openStep3: () => {
        setExplicitStep3(true)
        void reload()
      },
      openPermissions,
      navigationKey,
      computerId: activeStore?.computer.id ?? null,
      hubId: activeStore?.hello?.hostId ?? null,
      spaceId,
    }),
    [visibleStatus, openPermissions, navigationKey, activeStore, spaceId, reload],
  )

  return {
    client,
    whereChoice,
    spaceId,
    status: whereChoice === 'local' ? visibleStatus : null,
    showStep3,
    explicitStep3,
    selectWhere: (where: WhereChoice) => {
      setChoices((previous) => ({ ...previous, [navigationKey]: where }))
      setStep3Held(true)
    },
    dismissForComputers: closeStep3,
    closeStep3,
    controller,
  }
}
