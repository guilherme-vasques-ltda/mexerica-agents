// AC: resposta de donnaEngineAccounts({}), consultada diretamente no Hub conectado.
// Fora do mirror/cache. Contas de motor não são logins D82.
export interface EngineAccountsQuery {
  // Somente a capacidade engine.accounts pode forçar a sondagem; o Hub mantém cache por cinco minutos.
  refresh?: boolean
}

export interface EngineAccountModel {
  id: string
  options: {
    id: string
    values: string[]
    default: string | null
  }[]
}

export interface EngineAccount {
  id: string
  label: string
  brand: 'Anthropic' | 'OpenAI' | 'xAI'
  backend: 'claude' | 'codex' | 'grok'
  billing: 'subscription' | 'paid_api' | 'unverified'
  state: 'connected' | 'unverified' | 'unavailable' | 'disconnected'
  // Vazio significa catálogo não comprovado; nunca completar pela marca ou pelo modelo do bot.
  models: EngineAccountModel[]
  // MX-6: ids de modelo ocultados pelo dono, global no Hub. `models` segue completo — o app
  // filtra a listagem/escolha nova; modelo oculto em uso continua valendo. Ausente = Hub antigo
  // sem o recurso ("não suportado"); tratar como lista vazia sem oferecer Ocultar/Mostrar.
  hiddenModelIds?: string[]
  // `true` = conexão pausada pelo dono, com a credencial preservada (não é o
  // desconectar, que sai da conta). Ausente = Hub antigo sem o recurso: nada de pausar/reativar.
  paused?: boolean
}

export interface EngineAccounts {
  accounts: ManagedAccount[]
}

// D164: identificador opaco gerado pelo Hub, independente do titular/plano.
export type Engine = 'codex' | 'claude' | 'grok'
export type AccountId = string

export interface ManagedAccount extends EngineAccount {
  revision: number
  origin: 'legacy' | 'managed'
  botIds: string[]
}

export interface CreateAccount {
  backend: Engine
  label: string
  kind: 'cli' | 'api'
  clientRequestId: string
}

export interface AccountMutation {
  engineAccountId: AccountId
  expectedRevision: number
}

// MX-6: um gesto = um modelo; `hidden` é o estado novo. Idempotente no Hub.
export interface ModelVisibility extends AccountMutation {
  modelId: string
  hidden: boolean
}

// Pausar (`false`) preserva a credencial; reativar (`true`) volta sem prova —
// o Hub só reconecta depois de confirmar a mesma conta.
export interface SetAccountEnabled extends AccountMutation {
  enabled: boolean
}

// MEX-1294/F6-A: uso lido no perfil da própria conta, por Space. `unavailable` = sem leitura
// comprovada — nunca 0%. `session`/`week` são as janelas normalizadas; `resetsText` é o texto
// cru do provedor quando o Hub não tem o instante. `nearLimit` (>=90%) marca a janela apertada.
export interface AccountUsage {
  engineAccountId: AccountId
  spaceId?: string
  state: 'available' | 'unavailable'
  source: 'cli' | 'agent' | 'sessions' | null
  updatedAt: number | null
  windows: { id: string; percent: number; resetsAt: number | null; resetsText?: string | null; nearLimit?: boolean }[]
}

// MEX-1391/F6-B: fallback entre contas do mesmo motor (`donnaEngineFallback*`, capacidade engine.accounts). A ordem de
// reserva é do Space e do motor (a conta do bot é o passo 0); só assinatura comprovada entra. O app mostra e envia
// intenções; quem decide a troca é o Hub.
export type MotivoDescanso = 'perto_do_limite' | 'esgotada'
export interface FallbackRest {
  engineAccountId: AccountId
  /** Quando a conta volta a valer; `null` com bloqueio de 1 h do destino que falhou ou uso extra já permitido. */
  restUntil: number | null
  reason: MotivoDescanso | null
  /** Uso extra permitido pelo dono até este instante (o reset informado). */
  extraUntil: number | null
  blockedUntil: number | null
}
export interface FallbackSession {
  sessionId: string
  botId: string
  originAccountId: AccountId
  currentAccountId: AccountId
  since: number
  reason: MotivoDescanso | 'voltou'
  originBackAt: number | null
}
export interface FallbackState {
  spaceId: string
  orders: { backend: Engine; accounts: AccountId[] }[]
  botOrders: { botId: string; backend: Engine; accounts: AccountId[] }[]
  resting: FallbackRest[]
  sessions: FallbackSession[]
}
export interface FallbackSave { spaceId?: string; backend: Engine; botId?: string; accounts: AccountId[] | null }
export interface FallbackAction { spaceId?: string; action: 'backNow' | 'allowExtra'; engineAccountId: AccountId }
