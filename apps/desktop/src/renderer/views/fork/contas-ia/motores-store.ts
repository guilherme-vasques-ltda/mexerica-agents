import type { Backend } from '../../../../shared/models.ts'
import { Observable } from '../../../lib/observable.ts'

// D169/D164 (components/ContasIa, "Motores"): quando a seção de motores relê o Hub. Usa a ação existente do store
// (`refreshBackends`, que atualiza `hello.backends`), só ao abrir a página ou por pedido, e cerca cada leitura pelo Hub,
// a conexão e a geração da conta: resposta de outra origem não marca nada como atualizado e, se o Hub mudou no meio,
// relê uma vez para o atual (a ação do store grava no hello vigente). Sair da página cancela.

/** O que a seção lê do store de um computador (BotStore): nunca escrito aqui. */
export interface MotoresSource {
  readonly hello: { hostId: string; backends: Backend[] } | null
  readonly connectedClient: object | null
  readonly principalEpoch: number
  refreshBackends(): Promise<boolean>
}

interface Origin { hostId: string; client: object; epoch: number }
const originOf = (s: MotoresSource): Origin | null => (s.hello?.hostId && s.connectedClient ? { hostId: s.hello.hostId, client: s.connectedClient, epoch: s.principalEpoch } : null)
const same = (a: Origin, b: Origin | null) => !!b && a.hostId === b.hostId && a.client === b.client && a.epoch === b.epoch

export class MotoresStore extends Observable {
  /** idle: nada lido nesta abertura; ready: leitura da origem atual; error: a leitura falhou (a lista é a última do Hub). */
  state: 'idle' | 'loading' | 'ready' | 'error' = 'idle'
  asOf: number | null = null
  private gen = 0
  private running: Promise<void> | null = null

  /** Uma leitura por vez: outro pedido enquanto ela corre não chama o Hub de novo. */
  refresh(source: MotoresSource): Promise<void> {
    if (this.running) return this.running
    const run = this.read(source, true).finally(() => { if (this.running === run) this.running = null })
    this.running = run
    return run
  }

  /** A página fechou (ou mudou de Hub): a resposta em voo não vale mais. */
  cancel() {
    this.gen++
    this.running = null
    if (this.state === 'loading') this.state = 'idle'
    this.changed()
  }

  private async read(source: MotoresSource, again: boolean): Promise<void> {
    const origin = originOf(source)
    if (!origin) return
    const gen = ++this.gen
    this.state = 'loading'
    this.changed()
    const ok = await source.refreshBackends()
    if (gen !== this.gen) return
    const now = originOf(source)
    if (!same(origin, now)) {
      // A resposta era de outro Hub, conexão ou conta: o hello atual pode ter recebido a lista antiga.
      if (again && now) return this.read(source, false)
      this.state = 'idle'
      return this.changed()
    }
    this.state = ok ? 'ready' : 'error'
    if (ok) this.asOf = Date.now()
    this.changed()
  }
}

const stores = new WeakMap<object, MotoresStore>()

/** Uma leitura por Hub (por store), compartilhada entre aberturas da página. */
export function motoresFor(hub: object) {
  let store = stores.get(hub)
  if (!store) {
    store = new MotoresStore()
    stores.set(hub, store)
  }
  return store
}
