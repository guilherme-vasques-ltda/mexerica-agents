// Rota-02: RPC administrativo fora do mirror; nenhuma seleção de execução.
import type { ManagedAccount } from './fork-engine-accounts'
export interface RoutingSelection { backend: string; model: string | null; modelOptions: Record<string, string> }
export interface RoutingReserve { accountId: string; accountRevision: number; selection: RoutingSelection }
export interface RoutingPolicy { mode: 'fixed' | 'auto'; guide: string; reserves: RoutingReserve[] }
export interface RoutingDocument<T> { schemaVersion: 1; revision: number; value: T }
export interface RoutingView {
  hubId: string
  botId: string
  spaceId: string
  policy: RoutingDocument<RoutingPolicy>
  generalGuide: RoutingDocument<string>
  effectiveGuide: string
  auto: { available: false; reason: 'executor_and_scope_unproven' }
  reservesExecution: 'unavailable'
  execution: 'existing_fixed'
  fixedCheck: { status: 'checked' | 'unavailable'; reason: 'scope_unproven' | 'legacy_preserved' | 'catalog_only' | 'combination_unproven' }
  accounts: ManagedAccount[]
}
export interface RoutingSave {
  botId: string
  expectedRevision: number
  expectedGeneralRevision: number
  policy: RoutingPolicy
  generalGuide: string
}
/** `donnaRoutingReceipt`: recibo de um turno, só leitura e só do dono. Sem token, binding, carimbo nem selo. */
/** `conversationId`: o chat da entrada (na principal do Pessoal, o próprio bot). */
export interface RoutingReceiptRequest { conversationId: string; sessionId: string; entryId: string }
export interface RoutingReceiptSelection { accountId: string; selection: RoutingSelection }
export type RoutingUnattested = 'requiredFactNotAttested'
export interface RoutingReceipt {
  schemaVersion: 1
  destination: { botId: string; spaceId: string; session: { id: string; chatId: string } }
  requested: RoutingReceiptSelection
  admitted: RoutingReceiptSelection
  decidedBy: 'fixo' | 'auto' | 'reserva' | 'dono'
  reason: 'fixedSelection' | 'explicitSelection' | 'frozenSessionSelection'
  requiredFacts: { coverageBefore: RoutingUnattested; coverageAfter: RoutingUnattested; accountIdentityGenerationCatalog: RoutingUnattested }
}
/** `pending`: ainda na fila; `notRouted`: enviada sem recibo (não roteada ou anterior ao Roteamento). */
export type RoutingReceiptResponse = { hubId: string } & ({ state: 'routed'; receipt: RoutingReceipt } | { state: 'notRouted' | 'pending'; receipt: null })
export const ROUTING_PERSONAL = '00000000-0000-4000-8000-000000000001'
