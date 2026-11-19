import type { ForkCloudAttachment } from '@shared/fork-cloud'
import type { Computer } from '@shared/models'
import type { LojaSegura } from '../../../store/fork-queda-curta'

/** A parte do AppModel que anexa lojas (o mesmo attach/detach do SSH). */
export interface AttachTarget<S> {
  store(id: string): unknown
  attach(computer: Computer, store: S): void
  detach(id: string): void
  subscribe(listener: () => void): () => void
}

/** O anexo qualificado do Main, ou null enquanto não houver. */
export interface AttachSource {
  readonly readyAttachment: ForkCloudAttachment | null
  subscribe(listener: () => void): () => void
}

/**
 * MX-217: como a loja da ponte atravessa uma queda curta do canal. `segura` dá o link trocável da loja recém-criada
 * (o `connectionId`); `temporaria` diz se o Main ainda reconecta ao mesmo Hub (store/fork-queda-curta.ts).
 */
export interface QuedaDaPonte<S> {
  segura(store: S, computerId: string): LojaSegura<string>
  temporaria(computerId: string): boolean
}

/** O computador público do anexo: sem endereços, a ponte não usa nenhum. */
export function cloudComputer(a: ForkCloudAttachment): Computer {
  return { id: a.computer.id, name: a.computer.name, signKey: a.computer.signKey, urls: [] }
}

/**
 * Anexa ao AppModel uma loja pela ponte enquanto o Main publica um anexo qualificado. O SSH e o
 * host deste computador vêm primeiro: com o mesmo computador já anexado, nada muda. Só desanexa a
 * loja que ele mesmo criou; uma loja que tomou o lugar dela não é tocada. Outro canal, contexto
 * ou Hub cria outra loja, que lê só o cache do próprio contexto (e o BotSync zera um Hub trocado).
 */
export class CloudAttachmentBinder<S> {
  private own: { key: string | null; ident: string; computerId: string; store: S; segura: LojaSegura<string> | null } | null = null
  private stopped = false
  private readonly app: AttachTarget<S>
  private readonly source: AttachSource
  private readonly make: (attachment: ForkCloudAttachment, computer: Computer) => S
  private readonly queda: QuedaDaPonte<S> | null

  constructor(app: AttachTarget<S>, source: AttachSource, make: (attachment: ForkCloudAttachment, computer: Computer) => S, queda: QuedaDaPonte<S> | null = null) {
    this.app = app
    this.source = source
    this.make = make
    this.queda = queda
  }

  start() {
    const offs = [this.source.subscribe(this.sync), this.app.subscribe(this.sync)]
    this.sync()
    return () => {
      this.stopped = true
      for (const off of offs) off()
      this.release()
    }
  }

  readonly sync = () => {
    if (this.stopped) return
    // Substituída (SSH ou host local anexou o mesmo computador): já foi aposentada por quem a trocou.
    if (this.own && this.app.store(this.own.computerId) !== this.own.store) { this.own.segura?.revogar(); this.own = null }
    const a = this.source.readyAttachment
    const key = a ? JSON.stringify([a.connectionId, a.computerId, a.contextId, a.hostId]) : null
    const own = this.own
    // MX-217: canal caído com o Main ainda reconectando ao mesmo Hub: a loja fica (estacionada) até o prazo.
    // Falha definitiva (acesso encerrado, saída da conta, outro Hub), inclusive no meio do prazo: sai na hora.
    if (own?.segura && !a) {
      if (!this.queda?.temporaria(own.computerId)) return this.release()
      own.key = null
      own.segura.cair(this.expire(own))
      return
    }
    if (own && own.key === key) return
    // MX-217: outro canal ao mesmo computador, contexto e Hub: a mesma loja segue pelo canal novo.
    if (own?.segura && a && own.ident === ident(a)) {
      own.key = key
      own.segura.voltar(a.connectionId)
      return
    }
    this.release()
    if (!a || key === null || this.app.store(a.computerId)) return
    const computer = cloudComputer(a)
    const store = this.make(a, computer)
    this.own = { key, ident: ident(a), computerId: a.computerId, store, segura: this.queda?.segura(store, a.computerId) ?? null }
    this.app.attach(computer, store)
  }

  private expire(own: NonNullable<CloudAttachmentBinder<S>['own']>) {
    return () => { if (this.own === own && !this.stopped) { this.release(); this.sync() } }
  }

  private release() {
    const own = this.own
    this.own = null
    own?.segura?.revogar()
    if (own && this.app.store(own.computerId) === own.store) this.app.detach(own.computerId)
  }
}

const ident = (a: ForkCloudAttachment) => JSON.stringify([a.computerId, a.contextId, a.hostId])
