import type { SpaceHello } from '../../shared/fork-space-hello.ts'

const metodos = {
  functionModels: 'spaceBackgroundContract', checkFunctionModel: 'spaceBackgroundContract',
  saveFunctionModel: 'spaceBackgroundContract', restoreFunctionModel: 'spaceBackgroundContract',
  restoreFunctionModels: 'spaceBackgroundContract', backgroundSpaceSelections: 'spaceBackgroundContract',
  saveBackgroundSpaceSelection: 'spaceBackgroundContract', backgroundHistory: 'spaceBackgroundContract',
  donnaCostReport: 'spaceUsageContract', usage: 'spaceUsageContract', donnaUsageHistory: 'spaceUsageContract',
  donnaStorageReport: 'spaceStorageContract',
  donnaEngineAccounts: 'spaceEngineAccountsContract', donnaEngineAccess: 'spaceEngineAccountsContract',
  donnaEngineAccountCreate: 'spaceEngineAccountsContract', donnaEngineAccountRename: 'spaceEngineAccountsContract',
  donnaEngineAccountModelVisibility: 'spaceEngineAccountsContract', donnaEngineAccountDisconnect: 'spaceEngineAccountsContract',
  donnaEngineAccountRemove: 'spaceEngineAccountsContract', donnaEngineAccountUsage: 'spaceEngineAccountsContract',
  donnaEngineFallback: 'spaceEngineAccountsContract', donnaEngineFallbackSave: 'spaceEngineAccountsContract', donnaEngineFallbackAction: 'spaceEngineAccountsContract',
  donnaEngineSetupStart: 'spaceEngineAccountsContract', donnaEngineSetupAction: 'spaceEngineAccountsContract',
  engineGoKeySave: 'spaceEngineAccountsContract', agentAuthenticate: 'spaceEngineAccountsContract',
  agentSetup: 'spaceEngineAccountsContract', setAgentEnv: 'spaceEngineAccountsContract',
  memoryProfile: 'spaceMemoryProfileContract', saveMemoryProfile: 'spaceMemoryProfileContract',
  donnaDelegationRules: 'spaceDelegationContract', donnaDelegationRulesSave: 'spaceDelegationContract',
} as const satisfies Record<string, keyof SpaceHello>

export type MetodoSpace = keyof typeof metodos
export type CapacidadeSpace = keyof SpaceHello
export function capacidadeSpace(hello: SpaceHello | null, capacidade: CapacidadeSpace): number {
  const valor = hello?.[capacidade]
  return typeof valor === 'number' && Number.isSafeInteger(valor) && valor > 0 ? valor : 0
}

interface Link { call<T>(metodo: string, params: unknown): Promise<T> }
interface Store {
  readonly computer: { id: string }
  readonly spaceId?: string
  readonly hello: (SpaceHello & { hostId: string }) | null
  readonly spaces: { personalSpaceId: string } | null
  readonly connectedClient: Link | null
  readonly principalEpoch: number
}
interface Navegacao {
  readonly selection: { computerId: string } | null
  readonly spaceMirrors: {
    readonly spaceEpoch: number
    readonly active: { computerId: string; hostId: string | null; spaceId: string } | null
    root(computerId: string): Store | null
    current(computerId: string): Store | null
  }
}
type Resultado<T> = { kind: 'ok'; valor: T } | { kind: 'indisponivel' } | { kind: 'descartado' }
const legado = (metodo: MetodoSpace) => metodos[metodo] === 'spaceMemoryProfileContract' || metodos[metodo] === 'spaceDelegationContract'

/** Token capturado por método: a validade consulta a janela viva, não só o ID. */
export interface SpaceAtivo {
  readonly spaceId: string
  readonly modo: 'moderno' | 'legado'
  readonly client: Link
  readonly vigente: () => boolean
}

/** Recusa escopo ausente no contrato moderno e tokens de uma geração encerrada. */
export function somenteDoSpace(ativo: SpaceAtivo | null, resposta: unknown): boolean {
  if (!ativo?.vigente()) return false
  const id = typeof resposta === 'object' && resposta !== null ? (resposta as { spaceId?: unknown }).spaceId : undefined
  return id === ativo.spaceId || (id === undefined && ativo.modo === 'legado')
}

function modoSpace(hello: SpaceHello | null, metodo: MetodoSpace, spaceId: string, pessoal: string) {
  if (capacidadeSpace(hello, metodos[metodo]) >= 1) return 'moderno'
  return legado(metodo) && spaceId === pessoal ? 'legado' : null
}

/** Uma instância por montagem de tela; dispose invalida pedidos mesmo sem troca de Space. */
export class ContratoSpace {
  private geracao = 0
  private encerrado = false
  readonly store: Store
  private app: Navegacao
  constructor(store: Store, app: Navegacao) { this.store = store; this.app = app }
  dispose() { if (!this.encerrado) { this.encerrado = true; this.geracao++ } }
  private origem() {
    const { store, app } = this
    const root = app.spaceMirrors.root(store.computer.id)
    const ativo = app.spaceMirrors.active
    const pessoal = root?.spaces?.personalSpaceId
    const spaceId = store.spaceId ?? pessoal
    if (this.encerrado || !root || !pessoal || !spaceId || !store.hello?.hostId || !store.connectedClient
      || app.spaceMirrors.current(store.computer.id) !== store
      || (ativo && (ativo.computerId !== store.computer.id || ativo.hostId !== store.hello.hostId || ativo.spaceId !== spaceId))
      || (app.selection && app.selection.computerId !== store.computer.id)) return null
    return { spaceId, pessoal, hostId: store.hello.hostId, client: store.connectedClient, epoch: store.principalEpoch, geracao: this.geracao, geracaoSpace: app.spaceMirrors.spaceEpoch }
  }
  /** Capture antes da espera; reutilize o token no guard ao receber a resposta. */
  capturar(metodo: MetodoSpace): SpaceAtivo | null {
    const origem = this.origem()
    if (!origem) return null
    const modo = modoSpace(this.store.hello, metodo, origem.spaceId, origem.pessoal)
    if (!modo) return null
    const capacidade = capacidadeSpace(this.store.hello, metodos[metodo])
    return { spaceId: origem.spaceId, client: origem.client, modo, vigente: () => {
      const atual = this.origem()
      return !!atual && atual.spaceId === origem.spaceId && atual.pessoal === origem.pessoal && atual.hostId === origem.hostId
        && atual.client === origem.client && atual.epoch === origem.epoch && atual.geracao === origem.geracao
        && atual.geracaoSpace === origem.geracaoSpace && capacidadeSpace(this.store.hello, metodos[metodo]) === capacidade
    } }
  }
  /** Eventos não guardam geração de pedido: são conferidos no contexto atual. */
  eventoDoSpace(metodo: MetodoSpace, evento: unknown): boolean {
    return somenteDoSpace(this.capturar(metodo), evento)
  }
  async chamarComSpace<T>(metodo: MetodoSpace, params: Record<string, unknown> = {}): Promise<Resultado<T>> {
    const ativo = this.capturar(metodo)
    if (!ativo) return { kind: 'indisponivel' }
    // O chamador não escolhe o escopo, inclusive no protocolo legado.
    const { spaceId: _ignorado, ...corpo } = params
    try {
      const valor = await ativo.client.call<T>(metodo, ativo.modo === 'moderno' ? { ...corpo, spaceId: ativo.spaceId } : corpo)
      return somenteDoSpace(ativo, valor) ? { kind: 'ok', valor } : { kind: 'descartado' }
    } catch (erro) {
      if (!ativo.vigente()) return { kind: 'descartado' }
      throw erro
    }
  }
}

export function chamarComSpace<T>(store: ContratoSpace, metodo: MetodoSpace, params: Record<string, unknown> = {}) {
  return store.chamarComSpace<T>(metodo, params)
}
