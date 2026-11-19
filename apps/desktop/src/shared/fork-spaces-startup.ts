/** S2a: metadados somente. Nenhum invoke/geração/seleção de conta. */
import type { CoreId, CoreRevision, CoreVersion } from './fork-atividades.ts'
/** Chave não confiável; não é OperationRef, receipt ou grant. */
export interface StartupLaunchRef {
  id: CoreId
  requestDigest: string
}
export interface StartupExpectedContext {
  hubId: string
  spaceId: CoreId
  spaceRevision: CoreRevision
  /** ID nativo obtido de TeamSettings, sem inferir nome/primeiro bot/UUID. */
  principalBotId: string
  primaryConversationRevision: CoreRevision
}
export interface StartupOpenInput {
  version: CoreVersion
  spaceId: CoreId
  launchRef: StartupLaunchRef
  expectedContext: StartupExpectedContext
}
export interface StartupLookupInput {
  version: CoreVersion
  spaceId: CoreId
  launchRef: StartupLaunchRef
  expectedContext: StartupExpectedContext
}
/** Envelope canônico: dispatch valida hubId e o remove antes do parse Rust. */
export interface StartupTransportEnvelope { hubId: string }
export type StartupOpenRequest = StartupOpenInput & StartupTransportEnvelope
export type StartupLookupRequest = StartupLookupInput & StartupTransportEnvelope
export type StartupConversationResult = {
  kind: 'found'
  version: CoreVersion
  hubId: string
  launchRef: StartupLaunchRef
  expectedContext: StartupExpectedContext
  conversationId: CoreId
  conversationRevision: CoreRevision
  closed: boolean
} | {
  /** Leitura da fonte atual; não prova ausência final após timeout/restore. */
  kind: 'not_found'
  version: CoreVersion
  hubId: string
  launchRef: StartupLaunchRef
  expectedContext: StartupExpectedContext
}
/** Códigos novos; guardas herdadas/erros de transporte mantêm contrato próprio. */
export type StartupError = 'startup_invalid_contract' | 'startup_context_changed'
  | 'startup_scope_unavailable' | 'startup_intent_conflict'
  | 'startup_conversation_limit' | 'startup_outcome_unknown'
