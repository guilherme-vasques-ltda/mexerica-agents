import type { HostClient } from '../../../client/host-client'
import type {
  AiTestOutcome,
  OnboardingDetectPayload,
  OnboardingStatusPayload,
  WhereChoice,
} from '../../../../shared/fork-onboarding.ts'
import { mapAiTestOutcome } from '../../../../shared/fork-onboarding.ts'
import { chavesMudaram, salvarChave } from '../contas-ia/api-direta'
import { entrarComOpenRouter } from '../contas-ia/openrouter-login'

export type OnboardingProviderId = 'anthropic' | 'openai' | 'google' | 'xai' | 'openrouter'

export const ONBOARDING_KEY_PROVIDERS: readonly OnboardingProviderId[] = [
  'anthropic',
  'openai',
  'google',
  'xai',
  'openrouter',
] as const

export interface AiActionResult {
  outcome: AiTestOutcome
  modelsCount: number
}

/**
 * Contrato isolado de instalacao/pareamento para o passo 3.
 * Em build oficial le apenas os bridges reais (`window.codync.host` e `window.codync.ssh`);
 * testes e fixtures podem injetar um adaptador substituto sem registrar RPC ficticia no Hub.
 */
export interface OnboardingInstallPort {
  installLocal(): void
  restartLocal(): void
}

export const liveInstallPort: OnboardingInstallPort = {
  installLocal: () => window.codync.host.install(),
  restartLocal: () => window.codync.host.restart(),
}

export async function fetchOnboardingStatus(
  client: HostClient,
  context: { accountKey: string; spaceId: string },
): Promise<OnboardingStatusPayload> {
  return client.call<OnboardingStatusPayload>(
    'donnaOnboardingStatus',
    { accountKey: context.accountKey, spaceId: context.spaceId },
    10_000,
  )
}

export async function saveOnboardingState(
  client: HostClient,
  payload: {
    accountKey: string
    spaceId: string
    step?: 1 | 2 | 3 | 4
    where?: WhereChoice
    aiSkipped?: boolean
    deferredBeforeHub?: true
    completed?: boolean
  },
): Promise<OnboardingStatusPayload> {
  return client.call<OnboardingStatusPayload>('donnaOnboardingSave', payload, 10_000)
}

export async function detectOnboardingReadOnly(client: HostClient): Promise<OnboardingDetectPayload> {
  return client.call<OnboardingDetectPayload>('donnaOnboardingDetect', {}, 15_000)
}

export async function saveAndTestApiKey(
  client: HostClient,
  provider: string,
  rawKey: string,
  clearField: () => void,
): Promise<AiActionResult> {
  const result = await salvarChave(client, provider, rawKey, clearField)
  chavesMudaram()
  const modelsCount = result.modelos?.length ?? 0
  return {
    outcome: mapAiTestOutcome({
      ok: result.ok,
      estado: result.estado,
      erro: result.ok ? undefined : result.erro,
      modelosCount: modelsCount,
    }),
    modelsCount,
  }
}

export function startOpenRouterSignIn(
  client: HostClient,
  spaceId: string,
  onCompleting: () => void,
): { promise: Promise<AiActionResult>; cancel: () => void } {
  const ouvinte = window.codync?.app?.loginRetorno
  if (!ouvinte) {
    return {
      promise: Promise.resolve({ outcome: 'nao_verificado', modelsCount: 0 }),
      cancel: () => {},
    }
  }
  const { fim, cancelar } = entrarComOpenRouter(client, ouvinte, spaceId, undefined, onCompleting)
  const promise = fim
    .then((res): AiActionResult => {
      chavesMudaram()
      const modelsCount = res.modelos?.length ?? 0
      return {
        outcome: mapAiTestOutcome({
          ok: res.ok,
          estado: res.estado,
          erro: res.ok ? undefined : res.erro,
          modelosCount: modelsCount,
        }),
        modelsCount,
      }
    })
    .catch((): AiActionResult => ({ outcome: 'nao_verificado', modelsCount: 0 }))
  return { promise, cancel: cancelar }
}
