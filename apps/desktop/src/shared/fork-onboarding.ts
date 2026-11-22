// Regras puras do assistente de primeira instalação: 4 passos, retomada isolada por conta/contexto,
// capacidade real por SO e sem disparo automatico pago, sem sondagem silenciosa nem TCC na abertura.

export const TOTAL_ONBOARDING_STEPS = 4
export const PERSONAL_SPACE_ID = '00000000-0000-4000-8000-000000000001'
export function isLocalPersonalContext(input: {
  activeComputerId: string | null
  localComputerId: string | null
  activeSpaceId: string | null
}): boolean {
  if (input.activeComputerId && input.activeComputerId !== input.localComputerId) return false
  return !input.activeSpaceId || input.activeSpaceId === PERSONAL_SPACE_ID
}

export type OnboardingStep = 1 | 2 | 3 | 4
export type WhereChoice = 'local' | 'remote'

export interface OnboardingCapabilities {
  motorMexerica: boolean
  localKeysAndOAuth: boolean
  engineAccounts: boolean
  computerControl: boolean
  voice: boolean
}

export interface OnboardingReadinessItem {
  id: 'hub' | 'ai' | 'voice' | 'computer'
  ready: boolean
  available: boolean
}

export interface PersistedOnboardingState {
  step?: number | null
  whereChoice?: WhereChoice | null
  aiSkipped?: boolean
  completed?: boolean
  updatedAt?: number | null
}

export interface OnboardingStatusPayload {
  hubReady: boolean
  hostOs: string
  spaceContext?: {
    currentSpaceId: string
    personalSpaceId: string | null
  }
  capabilities: OnboardingCapabilities
  ai: {
    hasReadyAi: boolean
    connectedAccounts: number
    configuredKeys: number
    providers: string[]
  }
  voice: {
    configured: boolean
  }
  computerAccess: {
    available: boolean
    enabled: boolean
    connected: boolean
    capture: boolean
    input: boolean
    ready: boolean
  }
  readiness: {
    readyCount: number
    totalCount: number
    items: OnboardingReadinessItem[]
  }
  persisted: PersistedOnboardingState | null
}

export interface DetectedEngineSummary {
  id: string
  name: string
  installed: boolean
  canInstall: boolean
  signedIn?: boolean | null
  node?: { available: boolean; version?: string | null } | null
  connectedAccounts: number
}

export interface OnboardingDetectPayload {
  readOnly: true
  extractedCredentials: false
  engines: DetectedEngineSummary[]
  configuredKeyProviders: string[]
  ollama: {
    installed: boolean
    reachable: boolean
    modelsCount: number
  }
}

export type LocalInstallStage =
  | { kind: 'ready' }
  | { kind: 'starting' }
  | { kind: 'needs_consent' }
  | { kind: 'needs_install' }
  | { kind: 'failed'; message: string }
  | { kind: 'blocked_missing_binary' }
  | { kind: 'blocked_platform_disabled' }

export interface LocalInstallInputs {
  hostKind: 'missingBinary' | 'notInstalled' | 'starting' | 'running' | 'failed' | 'disabled'
  hostMessage?: string
  localHostFeature: boolean
  macLocalHubFeature: boolean
  devHost: boolean
  localHubSupported: boolean
  platform: 'darwin' | 'linux' | 'win32'
  consentGiven: boolean
}

/**
 * Traduz o estado real do HostController e das flags de build para a UX do passo 3 ("Usar neste computador").
 * Jamais fabrica instalacao concluida quando o binario nao existe ou o recurso esta desligado.
 */
export function resolveLocalInstallStage(input: LocalInstallInputs): LocalInstallStage {
  if (input.hostKind === 'disabled') return { kind: 'blocked_platform_disabled' }
  if (input.hostKind === 'running') return { kind: 'ready' }
  if (input.hostKind === 'starting') return { kind: 'starting' }
  if (input.hostKind === 'failed') return { kind: 'failed', message: input.hostMessage ?? '' }
  if (!input.localHubSupported && !input.devHost) return { kind: 'blocked_platform_disabled' }
  const allowedByFeature = input.localHostFeature || (input.macLocalHubFeature && input.platform === 'darwin')
  if (!allowedByFeature) return { kind: 'blocked_platform_disabled' }
  if (input.hostKind === 'missingBinary') return { kind: 'blocked_missing_binary' }
  return input.consentGiven ? { kind: 'needs_install' } : { kind: 'needs_consent' }
}

export type AiTestOutcome =
  | 'modelos_no_catalogo'
  | 'acesso_nao_confirmado'
  | 'nao_verificado'
  | 'recusada'

export interface AiTestInput {
  ok: boolean
  estado?: string
  erro?: string
  modelosCount?: number
}

/**
 * Unifica os 4+1 desfechos do teste de IA para a tela do passo 3 sem inventar sucesso quando a verificacao falhou.
 */
export function mapAiTestOutcome(input: AiTestInput): AiTestOutcome {
  if (input.ok) {
    return (input.modelosCount ?? 0) > 0 ? 'modelos_no_catalogo' : 'acesso_nao_confirmado'
  }
  if (input.estado === 'recusada' || input.erro === 'chave_invalida') return 'recusada'
  if (input.estado === 'acesso_nao_confirmado' || input.erro === 'sem_lista') return 'acesso_nao_confirmado'
  return 'nao_verificado'
}

export type StarterTaskId =
  | 'organize_week'
  | 'research_summary'
  | 'emails_posts'
  | 'reply_clients'
  | 'spreadsheets'
  | 'coding'

export interface StarterTaskSpec {
  id: StarterTaskId
  icon: string
  titleKey: `onboarding.passo4.tarefas.${StarterTaskId}.titulo`
  descKey: `onboarding.passo4.tarefas.${StarterTaskId}.desc`
  promptKey: `onboarding.passo4.tarefas.${StarterTaskId}.prompt`
}

export const STARTER_TASKS: readonly StarterTaskSpec[] = [
  {
    id: 'organize_week',
    icon: 'list.bullet.rectangle',
    titleKey: 'onboarding.passo4.tarefas.organize_week.titulo',
    descKey: 'onboarding.passo4.tarefas.organize_week.desc',
    promptKey: 'onboarding.passo4.tarefas.organize_week.prompt',
  },
  {
    id: 'research_summary',
    icon: 'doc.richtext',
    titleKey: 'onboarding.passo4.tarefas.research_summary.titulo',
    descKey: 'onboarding.passo4.tarefas.research_summary.desc',
    promptKey: 'onboarding.passo4.tarefas.research_summary.prompt',
  },
  {
    id: 'emails_posts',
    icon: 'square.and.pencil',
    titleKey: 'onboarding.passo4.tarefas.emails_posts.titulo',
    descKey: 'onboarding.passo4.tarefas.emails_posts.desc',
    promptKey: 'onboarding.passo4.tarefas.emails_posts.prompt',
  },
  {
    id: 'reply_clients',
    icon: 'bubble.left.and.bubble.right',
    titleKey: 'onboarding.passo4.tarefas.reply_clients.titulo',
    descKey: 'onboarding.passo4.tarefas.reply_clients.desc',
    promptKey: 'onboarding.passo4.tarefas.reply_clients.prompt',
  },
  {
    id: 'spreadsheets',
    icon: 'chart.bar',
    titleKey: 'onboarding.passo4.tarefas.spreadsheets.titulo',
    descKey: 'onboarding.passo4.tarefas.spreadsheets.desc',
    promptKey: 'onboarding.passo4.tarefas.spreadsheets.prompt',
  },
  {
    id: 'coding',
    icon: 'wrench.and.screwdriver',
    titleKey: 'onboarding.passo4.tarefas.coding.titulo',
    descKey: 'onboarding.passo4.tarefas.coding.desc',
    promptKey: 'onboarding.passo4.tarefas.coding.prompt',
  },
] as const


export function resolveAccountContextKey(accountUserId: string | null | undefined, spaceId: string | null | undefined): {
  accountKey: string
  spaceId: string
  storageKey: string
} {
  const accountKey = accountUserId?.trim() || 'local'
  const space = spaceId?.trim() || PERSONAL_SPACE_ID
  return {
    accountKey,
    spaceId: space,
    storageKey: `mexerica.onboarding.ui.v1.${accountKey}.${space}`,
  }
}

export function summarizeReadiness(status: OnboardingStatusPayload | null): {
  readyCount: number
  totalCount: number
  items: OnboardingReadinessItem[]
} {
  if (!status) return { readyCount: 0, totalCount: 0, items: [] }
  return {
    readyCount: status.readiness.readyCount,
    totalCount: status.readiness.totalCount,
    items: status.readiness.items,
  }
}

export function shouldShowFirstChatOnboarding(input: {
  isPrincipalBot: boolean
  isGroup: boolean
  hasUserMessages: boolean
  persistedCompleted: boolean
  dismissedLocally: boolean
}): boolean {
  if (!input.isPrincipalBot || input.isGroup) return false
  if (input.hasUserMessages || input.persistedCompleted || input.dismissedLocally) return false
  return true
}

export function isOnboardingStep3Pending(input: {
  persisted: PersistedOnboardingState | null
  hasReadyAi: boolean
  dismissedInSession: boolean
}): boolean {
  if (input.dismissedInSession) return false
  if (input.persisted?.completed) return false
  if (input.persisted?.aiSkipped) return false
  if (input.hasReadyAi) return false
  return true
}
