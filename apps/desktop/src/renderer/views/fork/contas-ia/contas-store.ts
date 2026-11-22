import type { HostClient } from '../../../client/host-client'
import type { AccountMutation, AccountUsage, CreateAccount, EngineAccounts, EngineAccountsQuery, ManagedAccount, ModelVisibility, SetAccountEnabled } from '../../../../shared/fork-engine-accounts.ts'
import { Observable } from '../../../lib/observable.ts'
import { errorCode, type AccountKind } from './regras.ts'
import type { OperationRef } from './setup-session.ts'

/** Quanto tempo a leitura de uso de uma conta vale no app (o Hub guarda a dele pelo mesmo tempo). */
export const USO_VALIDADE_MS = 60_000

/** O que a lista usa do `engineSetup` (o wrapper real em produção; um falso nos testes). */
export interface ContasApi {
  accounts(client: HostClient, query?: EngineAccountsQuery): Promise<EngineAccounts>
  create(client: HostClient, request: CreateAccount): Promise<ManagedAccount>
  rename(client: HostClient, request: AccountMutation & { label: string }): Promise<ManagedAccount>
  usage(client: HostClient, engineAccountId: string, spaceId?: string): Promise<AccountUsage>
  /** MX-6: oculta ou mostra um modelo da conta no Hub (`donnaEngineAccountModelVisibility`). */
  modelVisibility(client: HostClient, request: ModelVisibility): Promise<ManagedAccount>
  /** Pausar/reativar sem apagar credencial (`donnaEngineAccountSetEnabled`). */
  setEnabled(client: HostClient, request: SetAccountEnabled): Promise<ManagedAccount>
  disconnect(client: HostClient, request: AccountMutation): Promise<ManagedAccount>
  remove(client: HostClient, request: AccountMutation): Promise<Record<string, never>>
  setApiEnv(client: HostClient, request: AccountMutation & { backend: string; vars: Record<string, string> }): Promise<unknown>
}

const isManagedAccount = (value: unknown, id: string): value is ManagedAccount =>
  typeof value === 'object' && value !== null && (value as ManagedAccount).id === id && typeof (value as ManagedAccount).revision === 'number'

/** Gerenciadas primeiro (pelo nome), depois os perfis antigos do Hub. */
const ordered = (accounts: ManagedAccount[]) => [...accounts].sort((a, b) =>
  Number(a.origin === 'legacy') - Number(b.origin === 'legacy') || a.label.localeCompare(b.label, 'pt-BR'))

/**
 * As contas de IA de um Hub, lidas direto dele (fora do mirror e de qualquer cache local).
 * Guarda em memória só o tipo das contas criadas nesta sessão e a referência do login em
 * andamento de cada conta, para o painel retomar a mesma operação.
 */
export class ContasStore extends Observable {
  accounts: ManagedAccount[] | null = null
  loading = false
  /** DON-917: o erro cru do Hub (estado é dado); a tela o traduz com `errorMessage(erro, idioma)`. */
  error: unknown = null
  /** Conta (ou `create`) com pedido em andamento. */
  busy: string | null = null

  private kinds = new Map<string, AccountKind>()
  private operations = new Map<string, OperationRef>()
  private reads = 0
  private usos = new Map<string, { at: number; leitura: Promise<AccountUsage> }>()

  private readonly client: () => Promise<HostClient>
  private readonly api: ContasApi

  constructor(client: () => Promise<HostClient>, api: ContasApi) {
    super()
    this.client = client
    this.api = api
  }

  account(id: string) {
    return this.accounts?.find((a) => a.id === id) ?? null
  }

  kindOf(id: string): AccountKind | undefined {
    return this.kinds.get(id)
  }

  /** O Hub disse o tipo (ex.: `api_form_required` ao tentar login numa conta de chave). */
  learnKind(id: string, kind: AccountKind) {
    this.kinds.set(id, kind)
    this.changed()
  }

  operationFor(id: string) {
    return this.operations.get(id) ?? null
  }

  rememberOperation(id: string, ref: OperationRef | null) {
    const before = this.operations.get(id)
    if (ref) this.operations.set(id, ref)
    else this.operations.delete(id)
    if (!before !== !ref || before?.operationId !== ref?.operationId) this.changed()
  }

  /** `refresh` força a sondagem no Hub (somente a capacidade engine.accounts permite). */
  async load(refresh = false) {
    const read = ++this.reads
    this.loading = true
    this.changed()
    try {
      const client = await this.client()
      const { accounts } = await this.api.accounts(client, refresh ? { refresh: true } : {})
      if (read !== this.reads) return
      this.accounts = ordered(accounts)
      this.error = null
      // Conta que saiu da lista não tem mais login para retomar.
      for (const id of [...this.operations.keys()]) if (!accounts.some((a) => a.id === id)) this.operations.delete(id)
    } catch (error) {
      if (read !== this.reads) return
      this.error = error
    } finally {
      if (read === this.reads) {
        this.loading = false
        this.changed()
      }
    }
  }

  /**
   * Testar: pede ao Hub para verificar as contas agora (`refresh`: `auth status`/`account/read`, sem mensagem a modelo e
   * sem custo) e devolve esta conta como o Hub a vê. Falha de rede ou conta que sumiu viram erro com código do Hub.
   */
  async verify(account: ManagedAccount): Promise<ManagedAccount> {
    return this.mutate(account.id, async (client) => {
      // Mesmo contador do `load`: só a leitura mais nova grava na lista; uma antiga só devolve a conta.
      const read = ++this.reads
      const { accounts } = await this.api.accounts(client, { refresh: true })
      const fresh = accounts.find((a) => a.id === account.id)
      if (read === this.reads) {
        this.accounts = ordered(accounts)
        this.loading = false
      }
      if (!fresh) {
        // textos-ok: código interno, traduzido na tela por errorMessage.
        throw new Error('account_not_found: conta não encontrada.')
      }
      return fresh
    })
  }

  async create(request: CreateAccount): Promise<ManagedAccount> {
    return this.mutate('create', async (client) => {
      const account = await this.api.create(client, request)
      this.kinds.set(account.id, request.kind)
      this.put(account)
      return account
    })
  }

  async rename(account: ManagedAccount, label: string) {
    return this.mutate(account.id, async (client) => {
      this.put(await this.api.rename(client, { engineAccountId: account.id, expectedRevision: account.revision, label: label.trim() }))
    })
  }

  async disconnect(account: ManagedAccount) {
    return this.mutate(account.id, async (client) => {
      this.put(await this.api.disconnect(client, { engineAccountId: account.id, expectedRevision: account.revision }))
      this.operations.delete(account.id)
    }, true)
  }

  /**
   * Pausar ou reativar sem tocar credencial. Pausar tira a conta de todo uso novo (o Hub a
   * deixa `disconnected` e mantém chave/login). Reativar volta sem prova — a resposta chega
   * `unverified`, então a leitura seguinte é disparada aqui: é ela que confirma pelo cofre ou
   * sonda o CLI e devolve a conta no estado de verdade. Em qualquer falha o cadastro é relido:
   * o estado no Hub é o que vale, mesmo se o pedido não concluiu.
   */
  async setEnabled(account: ManagedAccount, enabled: boolean) {
    return this.mutate(account.id, async (client) => {
      // Leituras começadas antes da mutação não podem gravar o estado de cima depois dela.
      this.reads += 1
      this.loading = false
      const saved = await this.api.setEnabled(client, { engineAccountId: account.id, expectedRevision: account.revision, enabled })
      this.put(saved)
      if (!enabled) {
        this.operations.delete(account.id)
        return saved
      }
      const contas = await this.reload(client)
      return contas.find((a) => a.id === account.id) ?? saved
    }, true)
  }

  async remove(account: ManagedAccount) {
    return this.mutate(account.id, async (client) => {
      await this.api.remove(client, { engineAccountId: account.id, expectedRevision: account.revision })
      this.accounts = this.accounts?.filter((a) => a.id !== account.id) ?? null
      this.kinds.delete(account.id)
      this.operations.delete(account.id)
    }, true)
  }

  /** Chave de API pelo formulário existente, no cofre do Hub e só nesta conta. Os valores não ficam aqui. */
  async saveKey(account: ManagedAccount, vars: Record<string, string>) {
    return this.mutate(account.id, async (client) => {
      const saved = await this.api.setApiEnv(client, { engineAccountId: account.id, expectedRevision: account.revision, backend: account.backend, vars })
      this.kinds.set(account.id, 'api')
      // O Hub devolve a conta atualizada (conectada, API paga); sem ela, relê a lista.
      if (isManagedAccount(saved, account.id)) this.put(saved)
      else await this.reload(client)
    })
  }

  /**
   * Entrega 3: este Hub guarda os modelos ocultos? Só quando a resposta de contas traz `hiddenModelIds` (mesmo vazio).
   * Hub antigo não manda o campo: a tela desativa "Ocultar" e pede para atualizar o Hub. Conta antiga (não
   * gerenciada) nunca oculta: o Hub devolveria `legacy_preserved`.
   */
  canHideModels(account: ManagedAccount) {
    return account.origin === 'managed' && account.hiddenModelIds !== undefined
  }

  /**
   * Entrega 3: oculta ou mostra um modelo desta conta no seletor (vale para todos os aparelhos; o catálogo fica).
   * Sempre no Hub, nunca só neste aparelho: sem suporte do Hub, recusa antes de qualquer pedido.
   */
  async setModelHidden(account: ManagedAccount, modelId: string, hidden: boolean) {
    return this.mutate(account.id, async (client) => {
      // textos-ok: código interno, traduzido na tela por errorMessage.
      if (!this.canHideModels(account)) throw new Error('operation_forbidden: hidden models unsupported')
      // Um gesto = um modelo; `hidden` é o estado novo (idempotente no Hub). A revisão da conta não muda ao ocultar,
      // e a resposta já é a conta atualizada: entra na lista sem reler.
      const pedir = (conta: ManagedAccount) => this.api.modelVisibility(client, { engineAccountId: conta.id, expectedRevision: conta.revision, modelId, hidden })
      try {
        this.put(await pedir(account))
      } catch (error) {
        // A conta mudou em outro lugar (revisão): relê as contas e tenta de novo, uma vez, com a revisão de agora.
        if (errorCode(error) !== 'account_conflict') throw error
        const atual = (await this.reload(client)).find((a) => a.id === account.id)
        if (!atual || !this.canHideModels(atual)) throw error
        this.put(await pedir(atual))
      }
    })
  }

  /**
   * Leitura por conta E por Space (`spaceId` ausente vai ao Pessoal do Hub). Um pedido por conta e Space: quem pede
   * durante a leitura recebe a mesma promessa, e a resposta vale por 60 s (o Hub guarda a dele pelo mesmo tempo), então
   * trocar de conversa ou de idioma não sobe um processo do CLI por linha. Falha não fica guardada. Só o id basta: a
   * linha do bot pede o uso sem a lista global de contas carregada.
   */
  usage(accountId: string, spaceId?: string, now = Date.now()): Promise<AccountUsage> {
    const key = `${accountId}\u0000${spaceId ?? ''}`
    const guardada = this.usos.get(key)
    if (guardada && now - guardada.at < USO_VALIDADE_MS) return guardada.leitura
    const leitura = this.client().then((client) => this.api.usage(client, accountId, spaceId))
    this.usos.set(key, { at: now, leitura })
    leitura.catch(() => { if (this.usos.get(key)?.leitura === leitura) this.usos.delete(key) })
    return leitura
  }

  private put(account: ManagedAccount) {
    const others = (this.accounts ?? []).filter((a) => a.id !== account.id)
    this.accounts = ordered([...others, account])
  }

  /** Relê as contas e devolve a leitura; a lista compartilhada só muda se nenhuma leitura mais nova começou. */
  private async reload(client: HostClient) {
    // Mesmo contador do `load`: uma leitura começada antes não sobrescreve a lista relida aqui.
    const read = ++this.reads
    try {
      const { accounts } = await this.api.accounts(client, {})
      if (read === this.reads) this.accounts = ordered(accounts)
      return accounts
    } finally {
      if (read === this.reads) this.loading = false
    }
  }

  /** Um pedido por vez; conflito de revisão relê a lista para a pessoa conferir. */
  private async mutate<T>(key: string, run: (client: HostClient) => Promise<T>, reloadOnAnyError = false): Promise<T> {
    // textos-ok: código interno (`request_pending`), traduzido na tela por errorMessage.
    if (this.busy) throw new Error('request_pending: busy')
    this.busy = key
    this.error = null
    this.changed()
    try {
      const client = await this.client()
      try {
        return await run(client)
      } catch (error) {
        // Operações que gravam antes das conferências (desconectar, remover) relêem em qualquer
        // falha: a saída do programa pode não ter sido confirmada e a lista já mostra o estado real.
        // As outras relêem nos códigos em que o Hub mudou o que a tela tem na mão.
        const known = ['account_conflict', 'account_invalid', 'account_not_found', 'logout_failed', 'cli_unavailable', 'profile_invalid'].includes(errorCode(error) ?? '')
        if (reloadOnAnyError || known) await this.reload(client).catch(() => {})
        throw error
      }
    } finally {
      // Conectou, saiu ou trocou a conta: a leitura de antes não vale mais.
      this.usos.clear()
      this.busy = null
      this.changed()
    }
  }
}

const stores = new WeakMap<object, ContasStore>()

/** Uma lista por Hub, compartilhada entre a página e o atalho do Motor do bot. */
export function contasFor(hub: object, client: () => Promise<HostClient>, api: ContasApi) {
  let store = stores.get(hub)
  if (!store) {
    store = new ContasStore(client, api)
    stores.set(hub, store)
  }
  return store
}
