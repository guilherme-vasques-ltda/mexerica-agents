/** 19.1a-P1: DTOs descritivos, sem RPC/invoke ou capacidade ativa. */
export type CoreVersion = 1
/** UUID canônico não nulo, validado no Host; nunca label/Computer.id/rawSID. */
export type CoreId = string
/** Inteiro positivo <= Number.MAX_SAFE_INTEGER; não equivale a timestamp. */
export type CoreRevision = number
export type Audience =
  | { kind: 'principal' }
  | { kind: 'additional' }
  | { kind: 'group'; groupId: CoreId }
  | { kind: 'activity'; activityId: CoreId }
/** Forma válida não comprova existência, ACL, origem humana ou autoridade. */
export interface ContextRef {
  version: CoreVersion
  hubId: CoreId
  spaceId: CoreId
  conversationId: CoreId
  botId: CoreId
  audience: Audience
  teamId: CoreId | null
  privacyDomainId: CoreId
  domainRevision: CoreRevision
  conversationRevision: CoreRevision
}
export interface ExecutionRef {
  version: CoreVersion
  executionId: CoreId
  activityId: CoreId | null
  attemptId: CoreId
  turnId: CoreId
  generation: CoreRevision
  hubBootId: CoreId
  fence: CoreRevision
  contextRef: ContextRef
}
export type ActivityState = 'queued' | 'starting' | 'running' | 'waiting_input'
  | 'waiting_approval' | 'cancelling' | 'recovering' | 'succeeded' | 'failed'
  | 'cancelled' | 'interrupted'
/** typed_owner é procedência descrita, nunca prova de autorização humana. */
export type OriginKind = 'typed_owner' | 'delegated' | 'routine' | 'webhook'
  | 'group' | 'other_bot' | 'voice' | 'unknown'
export interface OriginRef {
  version: CoreVersion
  kind: OriginKind
  sourceRef: CoreId | null
  authenticatedCarrierRef: CoreId | null
  appReported: { kind: 'unknown' }
    | { kind: 'segmented'; hasPasted: boolean; segmentsRevision: CoreRevision }
}
export type RouteDestination =
  | { kind: 'retained' }
  | { kind: 'conversation'; contextRef: ContextRef }
  | { kind: 'group'; contextRef: ContextRef }
  | { kind: 'owner_inbox'; ownerId: CoreId }
export interface RouteRef {
  hubId: CoreId
  version: CoreVersion
  originContext: ContextRef | null
  originEntryId: CoreId | null
  requesterExecutionId: CoreId | null
  destination: RouteDestination
  routeRevision: CoreRevision
  policyRevision: CoreRevision
}
export interface PlanVersionRef {
  domain: 'workflow' | 'group' | 'routine'
  definitionId: CoreId
  versionId: CoreId
  /** SHA256 de plano imutável, 64 hex minúsculos; sem digest de segredo. */
  digest: string
}
export interface StepRef {
  version: CoreVersion
  rootActivityId: CoreId
  stepExecutionId: CoreId
  /** Chave do plano: 1–128 bytes ASCII alfanuméricos, ponto, hífen ou sublinhado. */
  logicalStepKey: string
  planVersionRef: PlanVersionRef
}
export type RequestOwnerRef =
  | { kind: 'agent_activity'; activityId: CoreId; stepRef: StepRef | null }
  | { kind: 'deterministic_step'; rootActivityId: CoreId; stepRef: StepRef }
/** Raiz/StepRef estrutural não é item extra, atividade nem vaga. */
export type SummaryItemRef = { kind: 'activity'; activityId: CoreId }
  | { kind: 'human_request'; requestId: CoreId }
export type CoreError = 'storage' | 'store_busy' | 'store_poisoned' | 'nested_transaction'
  | 'rollback_only' | 'statement_forbidden' | 'transaction_limit' | 'scope_unavailable'
