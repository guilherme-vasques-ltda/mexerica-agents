// MX-217: os anexos SSH da janela, fora do React para serem testáveis (ssh-model.ts liga ao modelo e ao Main).
import type { HostError, HostTransport } from '../../client/host-client'
import type { Computer } from '@shared/models'
import type { SSHState } from '@shared/ssh'
import { LinkTrocavel, LojaSegura, lojasJuntas, sshQuedaTemporaria, type LojaQueda, type Relogio } from '../../store/fork-queda-curta'

/** O túnel de um computador SSH: porta local e token do Hub. */
export interface TunelSSH { baseURL: string; token: string }

/** A parte do AppModel que os anexos usam. */
export interface AlvoSSH<S> {
  store(id: string): S | null
  computers(): readonly Computer[]
  attach(computer: Computer, store: S): void
  detach(id: string): void
  /** A raiz e os spaces do computador: param e voltam juntos. */
  lojasDo(id: string): readonly LojaQueda[]
}

interface Anexo<S> { key: string | null; profileId: string; identity: string; store: S; segura: LojaSegura<TunelSSH> }

/**
 * Anexa cada computador SSH conectado como uma loja loopback. Quando o túnel cai e o Main ainda o reabre, a loja fica
 * (cache, rascunhos, seleção) estacionada por até QUEDA_TOLERADA_MS, e um túnel novo ao mesmo computador retoma a
 * mesma loja. Túnel desligado, perfil removido, falha definitiva ou o fim do prazo desanexam como antes.
 */
export class AnexosSSH<S> {
  private readonly anexos = new Map<string, Anexo<S>>()
  private readonly app: AlvoSSH<S>
  private readonly criar: (computer: Computer, transporte: () => HostTransport) => S
  private readonly abrir: (tunel: TunelSSH) => HostTransport
  private readonly parado: HostTransport
  private readonly caido: () => HostError
  private readonly relogio: Relogio | undefined

  constructor(app: AlvoSSH<S>, criar: (computer: Computer, transporte: () => HostTransport) => S, abrir: (tunel: TunelSSH) => HostTransport,
    parado: HostTransport, caido: () => HostError, relogio?: Relogio) {
    this.app = app
    this.criar = criar
    this.abrir = abrir
    this.parado = parado
    this.caido = caido
    this.relogio = relogio
  }

  sync(state: SSHState) {
    const live = new Map(state.attachments.map((a) => [a.computer.id, a]))
    for (const [id, a] of live) {
      const key = `${a.baseURL}|${a.token}`
      const identity = JSON.stringify([a.profileId, a.computer])
      const own = this.anexos.get(id)
      if (own && this.app.store(id) === own.store && own.identity === identity) {
        if (own.key !== key) {
          own.key = key
          own.segura.voltar({ baseURL: a.baseURL, token: a.token })
        }
        continue
      }
      own?.segura.revogar()
      const cached = this.app.computers().find((c) => c.id === id)
      const computer: Computer = { ...a.computer, urls: [], color: cached?.color ?? null, device: cached?.device ?? null }
      const link = new LinkTrocavel<TunelSSH>({ baseURL: a.baseURL, token: a.token }, this.abrir, this.parado, this.caido)
      const store = this.criar(computer, link.transporte)
      const segura = new LojaSegura(lojasJuntas(() => this.app.lojasDo(id)), link, this.relogio)
      this.anexos.set(id, { key, profileId: a.profileId, identity, store, segura })
      this.app.attach(computer, store)
    }
    for (const [id, own] of [...this.anexos]) {
      if (live.has(id)) continue
      if (this.app.store(id) !== own.store) { own.segura.revogar(); this.anexos.delete(id); continue }
      const perfil = state.profiles.some((p) => p.id === own.profileId)
      if (perfil && sshQuedaTemporaria(state.status[own.profileId])) {
        own.key = null
        own.segura.cair(() => this.soltar(id, own))
      } else this.soltar(id, own)
    }
  }

  /** Em queda dentro do prazo (para a tela e os testes). */
  segurando(id: string) { return this.anexos.get(id)?.segura.caida ?? false }

  parar() {
    for (const [id, own] of [...this.anexos]) this.soltar(id, own)
  }

  private soltar(id: string, own: Anexo<S>) {
    if (this.anexos.get(id) !== own) return
    this.anexos.delete(id)
    own.segura.revogar()
    if (this.app.store(id) === own.store) this.app.detach(id)
  }
}
