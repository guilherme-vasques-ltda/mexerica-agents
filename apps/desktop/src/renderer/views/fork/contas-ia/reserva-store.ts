import type { HostClient } from '../../../client/host-client'
import type { AccountId, Engine, FallbackAction, FallbackSave, FallbackState } from '../../../../shared/fork-engine-accounts.ts'
import { Observable } from '../../../lib/observable.ts'

export interface ReservaApi {
  fallback(client: HostClient, spaceId?: string): Promise<FallbackState>
  fallbackSave(client: HostClient, request: FallbackSave): Promise<FallbackState>
  fallbackAction(client: HostClient, request: FallbackAction): Promise<FallbackState>
}

/**
 * A ordem de reserva e o descanso das contas do Space aberto, lidos direto do Hub (fora do mirror). Um pedido por vez;
 * o estado novo vem sempre na resposta do Hub, nunca é montado aqui.
 */
export class ReservaStore extends Observable {
  state: FallbackState | null = null
  loading = false
  busy = false
  /** O erro cru do Hub; a tela o traduz com `errorMessage(erro, idioma)`. */
  error: unknown = null
  private reads = 0

  private readonly client: () => Promise<HostClient>
  private readonly api: ReservaApi

  constructor(client: () => Promise<HostClient>, api: ReservaApi) {
    super()
    this.client = client
    this.api = api
  }

  async load(spaceId?: string) {
    const read = ++this.reads
    this.loading = true
    this.changed()
    try {
      const state = await this.api.fallback(await this.client(), spaceId)
      if (read !== this.reads) return
      this.state = state
      this.error = null
    } catch (error) {
      if (read === this.reads) this.error = error
    } finally {
      if (read === this.reads) {
        this.loading = false
        this.changed()
      }
    }
  }

  save(backend: Engine, accounts: AccountId[], spaceId?: string) {
    return this.run((client) => this.api.fallbackSave(client, { backend, accounts, ...(spaceId ? { spaceId } : {}) }))
  }

  act(action: FallbackAction['action'], engineAccountId: AccountId, spaceId?: string) {
    return this.run((client) => this.api.fallbackAction(client, { action, engineAccountId, ...(spaceId ? { spaceId } : {}) }))
  }

  private async run(request: (client: HostClient) => Promise<FallbackState>) {
    // textos-ok: código interno (`request_pending`), traduzido na tela por errorMessage.
    if (this.busy) throw new Error('request_pending: busy')
    this.busy = true
    this.error = null
    this.changed()
    try {
      this.reads++
      this.state = await request(await this.client())
    } catch (error) {
      this.error = error
      throw error
    } finally {
      this.busy = false
      this.changed()
    }
  }
}

const stores = new WeakMap<object, ReservaStore>()

/** Um por Hub, compartilhado entre os Detalhes da conta e o aviso de parada. */
export function reservaFor(hub: object, client: () => Promise<HostClient>, api: ReservaApi) {
  let store = stores.get(hub)
  if (!store) {
    store = new ReservaStore(client, api)
    stores.set(hub, store)
  }
  return store
}
