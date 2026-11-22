import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  isOnboardingStep3Pending,
  isLocalPersonalContext,
  resolveAccountContextKey,
  type OnboardingStatusPayload,
  type WhereChoice,
} from '../../../../shared/fork-onboarding.ts'
import type { HostClient } from '../../../client/host-client'
import { useAccount } from '../../../store/account'
import type { AppModel } from '../../../store/app-model'
import { useModel, useSelector } from '../../../lib/observable'
import { liveClient } from '../../settings/parts'
import { fetchOnboardingStatus, saveOnboardingState } from './adapter'
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
  const contextKey = useMemo(
    () => resolveAccountContextKey(account.user?.userId, activeSpaceId),
    [account.user?.userId, activeSpaceId],
  )
  const [whereChoice, setWhereChoice] = useState<WhereChoice>('local')

  const activeStore = useModel(app.local)
  const screenEpoch = activeStore?.screenEpoch
  const bergaId = activeStore?.principal.id
  const berga = bergaId ? activeStore?.bots.get(bergaId) : undefined
  const engine = useSelector(activeStore?.sessions, (sessions) => {
    const engine = bergaId ? sessions?.of(bergaId)?.find((session) => session.principal)?.engine : null
    return engine ? JSON.stringify(engine) : null
  })
  const localClient: HostClient | null = activeStore && !activeStore.mismatch ? liveClient(activeStore) : null
  const client = whereChoice === 'local' && isPersonalContext ? localClient : null

  const [status, setStatus] = useState<OnboardingStatusPayload | null>(null)
  const [explicitStep3, setExplicitStep3] = useState(false)
  const [dismissedInSession, setDismissedInSession] = useState(false)
  const [step3Held, setStep3Held] = useState(false)
  const [dismissal, setDismissal] = useState<{ userId: string; revision: number; deferred: boolean } | null>(null)
  const loadGen = useRef(0)
  const whereTouched = useRef<string | null>(null)
  const visibleStatus = whereChoice === 'local' && isPersonalContext ? status : null
  const bridge = window.codync.host.onboardingDismissal
  const marker = accountUserId && dismissal?.userId === accountUserId && dismissal.revision === revision ? dismissal : null
  const dismissalScope = accountUserId ? JSON.stringify([accountUserId, revision]) : null
  const currentDismissalScope = useRef(dismissalScope)
  currentDismissalScope.current = dismissalScope
  const loadScope = useMemo(() => ({}), [localClient, isPersonalContext, contextKey.storageKey, revision, dismissalScope])
  const currentLoadScope = useRef(loadScope)
  currentLoadScope.current = loadScope

  // Isola o estado visual ao trocar conta ou space sem reaproveitar rascunho de outra sessao.
  useEffect(() => {
    setStatus(null)
    setExplicitStep3(false)
    setDismissedInSession(false)
    setStep3Held(false)
    setWhereChoice('local')
  }, [contextKey.storageKey])

  useEffect(() => {
    let active = true
    if (accountUserId && bridge) {
      void bridge(accountUserId, 'read').catch(() => null).then((deferred) => {
        if (active && currentDismissalScope.current === dismissalScope) {
          setDismissal({ userId: accountUserId, revision, deferred: deferred === true })
        }
      })
    }
    return () => { active = false }
  }, [accountUserId, revision, bridge, dismissalScope])

  useEffect(() => {
    if (whereTouched.current === contextKey.storageKey) return
    if (status?.persisted?.whereChoice) setWhereChoice(status.persisted.whereChoice)
  }, [status?.persisted?.whereChoice, contextKey.storageKey])

  useEffect(() => {
    loadGen.current += 1
    setStatus(null)
  }, [localClient, isPersonalContext, contextKey.storageKey])

  const reload = useCallback(async () => {
    if (!localClient || !isPersonalContext) return
    const gen = ++loadGen.current
    try {
      const next = await fetchOnboardingStatus(localClient, {
        accountKey: contextKey.accountKey,
        spaceId: contextKey.spaceId,
      })
      if (gen === loadGen.current && currentLoadScope.current === loadScope) setStatus(next)
    } catch {
      // Mantem ultimo estado real conhecido; nunca fabrica prontidao.
    }
  }, [localClient, isPersonalContext, contextKey.accountKey, contextKey.spaceId, loadScope])

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

  useEffect(() => {
    if (!marker?.deferred || !accountUserId || !bridge || !localClient || !isPersonalContext) return
    let active = true
    const generation = loadGen.current
    void saveOnboardingState(localClient, {
      accountKey: accountUserId,
      spaceId: contextKey.spaceId,
      deferredBeforeHub: true,
    }).then(async (next) => {
      if (!active || currentDismissalScope.current !== dismissalScope || currentLoadScope.current !== loadScope) return
      if (generation === loadGen.current) {
        loadGen.current += 1
        setStatus(next)
      }
      if (await bridge(accountUserId, 'clear') === false && active && currentDismissalScope.current === dismissalScope && currentLoadScope.current === loadScope) {
        setDismissal({ userId: accountUserId, revision, deferred: false })
      }
    }).catch(() => {})
    return () => { active = false }
  }, [marker, accountUserId, bridge, localClient, isPersonalContext, contextKey.spaceId, revision, dismissalScope, loadScope])

  const persist = useCallback(
    async (patch: { step?: 1 | 2 | 3 | 4; where?: WhereChoice; aiSkipped?: boolean; completed?: boolean }, targetClient = localClient) => {
      if (!targetClient || !isPersonalContext) return
      const generation = ++loadGen.current
      try {
        const next = await saveOnboardingState(targetClient, {
          accountKey: contextKey.accountKey,
          spaceId: contextKey.spaceId,
          ...patch,
        })
        if (generation === loadGen.current && currentLoadScope.current === loadScope) setStatus(next)
      } catch {
        // Sem mascara de sucesso em caso de erro de gravacao.
      }
    },
    [localClient, isPersonalContext, contextKey.accountKey, contextKey.spaceId, loadScope],
  )

  const dismissalLoaded = !accountUserId || !bridge || marker !== null
  const autoStep3Pending = dismissalLoaded && !marker?.deferred && visibleStatus !== null && isOnboardingStep3Pending({
    persisted: visibleStatus.persisted,
    hasReadyAi: visibleStatus.ai.hasReadyAi,
    dismissedInSession,
  })

  useEffect(() => {
    if (autoStep3Pending) setStep3Held(true)
  }, [autoStep3Pending])

  const localInstallPending = dismissalLoaded && !marker?.deferred && allowLocalInstallGuide && app.computers.length === 0 && !localClient && !dismissedInSession
    && ['notInstalled', 'missingBinary', 'failed', 'starting'].includes(hostState.kind)
  const showStep3 = isPersonalContext && (explicitStep3 || step3Held || localInstallPending || (whereChoice === 'local' && autoStep3Pending))

  const closeStep3 = useCallback(
    () => {
      setExplicitStep3(false)
      setDismissedInSession(true)
      setStep3Held(false)
      if (!localClient && isPersonalContext && accountUserId && bridge) {
        void bridge(accountUserId, 'defer').then((deferred) => {
          if (deferred === true && currentDismissalScope.current === dismissalScope) {
            setDismissal({ userId: accountUserId, revision, deferred })
          }
        }).catch(() => {})
      }
      void persist({ step: 4, aiSkipped: true })
    },
    [persist, localClient, isPersonalContext, accountUserId, bridge, revision, dismissalScope],
  )

  const controller: OnboardingControllerValue = useMemo(
    () => ({
      status: visibleStatus,
      hasReadyAi: visibleStatus?.ai.hasReadyAi ?? false,
      openStep3: () => setExplicitStep3(true),
      openPermissions,
      markStep4Completed: () => {
        if (visibleStatus?.persisted?.completed) return
        void persist({ step: 4, completed: true })
      },
      storageKey: contextKey.storageKey,
      computerId: activeStore?.computer.id ?? null,
      hubId: activeStore?.hello?.hostId ?? null,
      spaceId: contextKey.spaceId,
    }),
    [visibleStatus, openPermissions, persist, contextKey.storageKey, activeStore, contextKey.spaceId],
  )

  return {
    client,
    whereChoice,
    contextKey,
    status: visibleStatus,
    showStep3,
    explicitStep3,
    selectWhere: (where: WhereChoice) => {
      whereTouched.current = contextKey.storageKey
      setWhereChoice(where)
      if (where === 'local' || where === 'remote') setStep3Held(true)
      void persist({ step: 3, where })
    },
    dismissForComputers: closeStep3,
    closeStep3,
    controller,
  }
}
