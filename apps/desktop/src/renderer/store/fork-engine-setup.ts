import type { HostClient } from '../client/host-client'
import type { AccountId, AccountMutation, AccountUsage, FallbackAction, FallbackSave, FallbackState, CreateAccount, EngineAccounts, EngineAccountsQuery, ManagedAccount, ModelVisibility, SetAccountEnabled } from '../../shared/fork-engine-accounts'
import type { SetupAction, SetupOperation, StartSetup } from '../../shared/fork-engine-setup'

/** Intenções tipadas pelo transporte existente. Nenhuma operação SSH no renderer. */
export const engineSetup = {
  accounts: (client: HostClient, query: EngineAccountsQuery = {}) => client.call<EngineAccounts>('donnaEngineAccounts', query, 90_000),
  create: (client: HostClient, request: CreateAccount) => client.call<ManagedAccount>('donnaEngineAccountCreate', request),
  rename: (client: HostClient, request: AccountMutation & { label: string }) => client.call<ManagedAccount>('donnaEngineAccountRename', request),
  modelVisibility: (client: HostClient, request: ModelVisibility) => client.call<ManagedAccount>('donnaEngineAccountModelVisibility', request),
  usage: (client: HostClient, engineAccountId: AccountId, spaceId?: string) =>
    client.call<AccountUsage>('donnaEngineAccountUsage', { engineAccountId, ...(spaceId ? { spaceId } : {}) }),
  // MEX-1391/F6-B: ordem de reserva, "Voltar agora" e "Permitir uso extra" (o Hub confere tudo de novo).
  fallback: (client: HostClient, spaceId?: string) => client.call<FallbackState>('donnaEngineFallback', spaceId ? { spaceId } : {}),
  fallbackSave: (client: HostClient, request: FallbackSave) => client.call<FallbackState>('donnaEngineFallbackSave', request),
  fallbackAction: (client: HostClient, request: FallbackAction) => client.call<FallbackState>('donnaEngineFallbackAction', request),
  start: (client: HostClient, request: StartSetup) => client.call<SetupOperation>('donnaEngineSetupStart', request),
  action: (client: HostClient, request: SetupAction) => client.call<SetupOperation>('donnaEngineSetupAction', request),
  disconnect: (client: HostClient, request: AccountMutation) => client.call<ManagedAccount>('donnaEngineAccountDisconnect', request),
  setEnabled: (client: HostClient, request: SetAccountEnabled) => client.call<ManagedAccount>('donnaEngineAccountSetEnabled', request),
  remove: (client: HostClient, request: AccountMutation) => client.call<Record<string, never>>('donnaEngineAccountRemove', request),
  // Segredo explícito do formulário existente; sem retorno da chave e sem alterar slot legado.
  setApiEnv: (client: HostClient, request: AccountMutation & { backend: string; vars: Record<string, string> }) => client.call<unknown>('setAgentEnv', request, 300_000),
}
