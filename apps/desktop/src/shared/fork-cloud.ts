/** DTOs do processo principal: nunca transportam JWT, chave, URL ou prova. */
export type ForkCloudKind = 'disabled' | 'unsafeStorage' | 'storageError' | 'signedOut' | 'signOutPending' | 'deploymentChanged' | 'discovering' | 'noHub' | 'needsLink' | 'hubCloudOff' | 'accountMismatch' | 'linking' | 'waitingOwnerApproval' | 'connecting' | 'ready' | 'hubOffline' | 'reconnecting' | 'accessEnded' | 'revocationPending' | 'handshakeRefused' | 'hubUnverified' | 'hubMissing' | 'updateRequired' | 'error' | 'linked' | 'unknown' | 'notSent'
export interface ForkCloudState { readonly kind: ForkCloudKind; readonly epoch: number; readonly attemptId: string; readonly computerId?: string; readonly code?: string; readonly deadline?: number; readonly readUnavailable?: true; readonly secondCode?: true }
export interface PersonalAttemptState {
  readonly attemptId:string; readonly mode:'command'|'device'; readonly phase:'prepared'|'awaitingApproval'|'running'|'unknown'|'receipt'|'stopped'|'closed'
  readonly canApprove:boolean; readonly canCheck:boolean; readonly canReconcile:boolean; readonly computerId?:string
}
/** D430 PR-A: só fatos públicos para conferência do dono; sem bytes assinados, chave privada ou prova. */
export interface PersonalLinkState {
  readonly attemptId: string
  readonly phase: 'confirmRoot' | 'rootComplete' | 'confirmManagement' | 'managed' | 'closed'
  readonly hub?: string; readonly hubFingerprint?: string; readonly device?: string; readonly deviceFingerprint?: string
  readonly reason?: 'originUnavailable' | 'vaultUnavailable' | 'clockUnqualified' | 'quarantine' | 'uncertain' | 'refused' | 'canceled'
}
/** B: apenas fato próprio efêmero ou indisponibilidade LOCAL. Não passa pelo mirror. */
export type PersonalLinkOwnStatus =
  | { readonly version: 1; readonly kind: 'unavailable'; readonly context: string; readonly reason: 'notLinked' | 'vaultUnavailable' | 'connectionUnavailable' | 'refused' | 'expired' }
  | { readonly version: 1; readonly kind: 'ownStatus'; readonly context: string; readonly hub: string; readonly device: string; readonly rights: readonly ['devices.manage', 'link.connect']; readonly state: 'active'; readonly validForMs: number }
export type ForkCloudIntent =
  | { kind: 'personalLinkStatus' | 'personalLinkDisconnect'; sourceId: string }
  | {kind:'personalLinkRoot'|'personalLinkManagement';sourceId:string}
  | {kind:'personalLinkConfirm'|'personalLinkCancel';attemptId:string}
  | {kind:'personalStart';mode:'command'|'device';sourceId:string}
  | {kind:'personalApprove'|'personalCheck'|'personalReconcile'|'personalCancel';attemptId:string}
  | { kind: 'startBootstrap' }
  | { kind: 'checkBootstrap' | 'proveBootstrap' | 'reconcileBootstrap'; bootstrapId: string }
  | { kind: 'state' } | { kind: 'reconcileRegistration' } | { kind: 'reconcileClaim' } | { kind: 'registerDevice'; name: string } | { kind: 'discover' }
  | { kind: 'startLocal'; profileId: string; claim: boolean } | { kind: 'startRemote'; computerId: string }
  | { kind: 'check'; attemptId: string } | { kind: 'confirm'; attemptId: string; code: string }
  | { kind: 'cancel'; attemptId: string } | { kind: 'connect'; computerId: string } | { kind: 'reconcilePin'; computerId: string } | { kind: 'disconnect' }
export interface ForkCloudComputer { readonly computerId: string; readonly name: string }
export interface PersonalBootstrapStatus { readonly profile: 'personal-linux-v1'; readonly wire: 1; readonly available: false; readonly reason: 'updateRequired' | 'helperRequired' }
export interface ForkCloudAttachment { readonly connectionId: string; readonly computerId: string; readonly contextId: string; readonly hostId: string; readonly computer: { readonly id: string; readonly name: string; readonly signKey: string; readonly urls: readonly string[] } }
export type ForkCloudRecoveryPhase = 'request' | 'reveal' | 'approval' | 'publication' | 'claim' | 'claimRequest'
export type ForkBootstrapPhase = 'available' | 'prepared' | 'createUnknown' | 'requested' | 'openingUnknown' | 'challengeUnknown' | 'challenged' | 'proofUnknown' | 'receipt' | 'reconcileUnknown' | 'published' | 'pinUnknown' | 'complete' | 'closed'
export interface ForkBootstrapState { readonly bootstrapId: string; readonly phase: ForkBootstrapPhase; readonly blocksOtherOperations?: boolean; readonly canStart: boolean; readonly canCheck: boolean; readonly canProve: boolean; readonly canReconcile: boolean; readonly computerId?: string; readonly deadline?: number; readonly readUnavailable?: true; readonly terminalReason?: 'canceled' | 'expired' }
export interface ForkCloudResult { readonly personalLinkOwn?: PersonalLinkOwnStatus; readonly personalLink?: PersonalLinkState; readonly personalBootstrap?: PersonalBootstrapStatus; readonly personalAttempt?:PersonalAttemptState; readonly bootstrap?: ForkBootstrapState; readonly recoveryPhase?: ForkCloudRecoveryPhase | null; readonly recovery?: 'registration' | 'enrollment' | null; readonly registration: 'unavailable' | 'unregistered' | 'registered'; readonly state: ForkCloudState; readonly computers: readonly ForkCloudComputer[]; readonly attachment?: ForkCloudAttachment }
export type ForkCloudCallResult = { kind: 'ok'; value: unknown } | { kind: 'err'; status: number; message: string; code?: string } | { kind: 'unknown'; reason: 'closed' | 'timeout' } | { kind: 'notSent'; reason: 'notReady' | 'busy' | 'tooLarge' }
export interface ForkCloudEventsBody { since: number; client: string; spaceId?: string; kind?: 'term'; term?: string }
/** Contrato fechado de assinatura: o terminal conserva o canal protegido existente. */
export function cloudSubscription(raw: unknown): ForkCloudEventsBody | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const b = raw as Record<string, unknown>
  const text = (v: unknown): v is string => typeof v === 'string' && v.length > 0 && v.length <= 128 && !/[\x00-\x1f\x7f]/.test(v)
  if (b.kind === 'term') {
    if (Object.keys(b).some(k => !['since', 'client', 'kind', 'term'].includes(k)) || b.since !== 0 || b.client !== 'login' || typeof b.term !== 'string' || !/^[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(b.term)) return null
    return { since: 0, client: 'login', kind: 'term', term: b.term }
  }
  if (Object.keys(b).some(k => !['since', 'client', 'spaceId'].includes(k)) || typeof b.since !== 'number' || !Number.isSafeInteger(b.since) || b.since < 0 || !text(b.client) || (b.spaceId !== undefined && !text(b.spaceId))) return null
  return { since: b.since, client: b.client, ...(typeof b.spaceId === 'string' ? { spaceId: b.spaceId } : {}) }
}
export interface ForkCloudStreamError { status: number; message: string }

