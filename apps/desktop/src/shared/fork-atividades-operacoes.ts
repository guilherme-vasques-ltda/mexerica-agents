/** 19.1a-P2a: dados descritivos, sem RPC, grant, ledger ou consumidor ativo. */
import type { ContextRef, CoreId, CoreRevision, CoreVersion } from './fork-atividades.ts'
export type OperationKind = 'prepare_review' | 'decide_review'
/** Nonce do gesto; não é OperationRef emitida pelo core. */
export type ClientNonce = CoreId
export interface OperationSelector {
  version: CoreVersion
  operationId: CoreId
  kind: OperationKind
}
/** SHA256 v1; validado no Host. Nunca autentica pessoa ou autoriza efeito. */
export type SemanticDigest = string
export type ReviewAction = 'save' | 'accept' | 'accept_with_edit' | 'reject' | 'delete' | 'copy' | 'bind' | 'release'
export type ReviewObjectKind = 'candidate' | 'memory' | 'dataset' | 'skill' | 'update' | 'effect'
  | 'account' | 'destination' | 'attachment' | 'source' | 'binding'
export interface ReviewObjectRef {
  contextRef: ContextRef
  kind: ReviewObjectKind
  objectId: CoreId
  revision: CoreRevision
  digest: SemanticDigest
}
/** Descritor não confiável; bool/Local/bearer não o promovem a prova humana. */
export interface CallerDeviceDescriptor {
  principalId: CoreId
  deviceId: CoreId
  sessionId: CoreId
  principalEpoch: CoreRevision
}
export interface ExactEffect {
  /** After exato mostrado no gesto, UTF-8 até32KiB. Não executar. */
  body: string
  account: ReviewObjectRef | null
  destination: ReviewObjectRef | null
  attachments: ReviewObjectRef[]
}
export interface ReviewBinding {
  version: CoreVersion
  contextRef: ContextRef
  callerDevice: CallerDeviceDescriptor
  recoveryEpoch: CoreId
  policyRevision: CoreRevision
  action: ReviewAction
  target: ReviewObjectRef
  before: ReviewObjectRef | null
  effect: ExactEffect
  sources: ReviewObjectRef[]
  bindings: ReviewObjectRef[]
}
/** Nonces distintas, capturadas antesawait; preparar não decide. */
export interface PrepareReviewInput {
  version: CoreVersion
  prepareNonce: ClientNonce
  decisionNonce: ClientNonce
  binding: ReviewBinding
}
export interface DecideReviewInput {
  version: CoreVersion
  decisionOperation: OperationSelector
  reviewId: CoreId
  decisionNonce: ClientNonce
  binding: ReviewBinding
}
/** Instantes do Hub, diferença exata120000ms; não TTL persistido no cliente. */
export interface ReviewWindow { issuedAt: CoreRevision; expiresAt: CoreRevision }
/** Projeção não confiável. ReviewIntent/ResolvedScope não são DTOs de entrada. */
export interface ReviewDescriptor {
  version: CoreVersion
  reviewId: CoreId
  prepareOperation: OperationSelector
  prepareNonce: ClientNonce
  decisionOperation: OperationSelector
  decisionNonce: ClientNonce
  binding: ReviewBinding
  semanticDigest: SemanticDigest
  window: ReviewWindow
}
export type OperationError = 'invalid_contract' | 'nonces_must_differ' | 'operation_mismatch'
  | 'binding_mismatch' | 'digest_mismatch' | 'review_mismatch' | 'invalid_window' | 'not_yet_issued' | 'expired'
