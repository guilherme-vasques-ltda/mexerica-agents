// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { HostClient } from '../../client/host-client'
import { Observable, useModel } from '../../lib/observable'
import type { BotStore } from '../../store/bot-store'
import type { InstalledSkillV2 } from '../../store/fork-skills'
import { ProjecaoSkills } from '../fork/catalogo-skills/consumidores'
import { lerRetornoLogin, type RetornoLogin } from '../fork/meus-mcp/retorno-login'
import { LeituraConectores, mesmaOrigem, origemDe, resultadoIncerto, type EstadoLista, type Origem } from '../fork/meus-mcp/estado'
import { errorText, market, needsSignIn, sleep, type InstalledConnector } from './market-models'

/**
 * Whether a new add/import may start (D175 F2): `free`, or `pending` after one the Hub never answered,
 * until a successful read of this Hub's list comes after it. `readError` is that re-read's failure, if
 * it failed; `moved` says the link changed since that write.
 */
export type WriteGate = { state: 'free' } | { state: 'pending'; readError: string | null; moved: boolean }

/** What's installed on one computer, for the Marketplace and bot settings (kit BotStore's plugin state). */
export class Plugins extends Observable {
  /** D175: the connectors list, held to the Hub, link and generation that answered (views/fork/meus-mcp). */
  private readonly connectors = new LeituraConectores<HostClient>()
  /** D166 R8: skills pelo protocolo forte, presas ao Hub, ao link e à geração que responderam. */
  private readonly skills = new ProjecaoSkills()
  /** D175 F2: an add/import with no answer: where it went, and the read mark taken when it settled. */
  private unconfirmedWrite: { mark: number; origin: Origem<HostClient> | null } | null = null

  constructor(readonly store: BotStore) {
    super()
  }

  /** The current origin's connectors; another origin's are never shown. */
  get installedConnectors(): InstalledConnector[] {
    return this.connectors.itens(this.store)
  }

  /** Meus MCP: every native configuration, catalog-era ones included. */
  get nativeConnectors(): InstalledConnector[] {
    return this.installedConnectors
  }

  /** Reading, read, failed to read (never shown as empty) or no link to the Hub. */
  get connectorsState(): EstadoLista {
    return this.connectors.estado(this.store)
  }

  get connectorsError(): string | null {
    return this.connectors.erro(this.store)
  }

  /** D175 F2: a published error never frees it; only a successful read after the unanswered write does. */
  get writeGate(): WriteGate {
    const write = this.unconfirmedWrite
    if (!write) return { state: 'free' }
    return { state: 'pending', readError: this.connectors.erroDepois(this.store, write.mark), moved: !mesmaOrigem(this.store, write.origin) }
  }

  /** The current hub's installed skills; another hub's are never shown. */
  get installedSkills(): InstalledSkillV2[] {
    return this.skills.itens(this.store)
  }

  async refresh() {
    await this.read(false)
  }

  /**
   * The add sheet's Reler a lista: reads, nothing else. The user has been told the link changed, so a
   * successful read of the Hub answering now also frees a write that went to another one.
   */
  async reread() {
    await this.read(true)
  }

  private async read(movedSeen: boolean) {
    await Promise.allSettled([this.connectors.ler(this.store, market.connectors), this.skills.ler(this.store)])
    // The unanswered write is reconciled only by a successful read of the list asked for after it; a read
    // nobody asked for frees it only on the same Hub it went to, never quietly on another.
    const write = this.unconfirmedWrite
    if (write && this.connectors.confirmadaDepois(this.store, write.mark) && (movedSeen || write.origin?.hub === this.store.hello?.hostId)) this.unconfirmedWrite = null
    this.changed()
  }

  /** A write, then a fresh read of the list either way: an unclear outcome is never taken as done or undone. */
  private async after<T>(op: Promise<T>) {
    try {
      return await op
    } finally {
      await this.refresh()
    }
  }

  /**
   * An add/import (D175 F2). With no answer (status 0 or none) it may have been saved, so new adds wait in
   * `writeGate` for a successful read asked for after it settled. Nothing is retried here, and the list is
   * read again either way.
   */
  private async write<T>(op: (client: HostClient) => Promise<T>) {
    const client = await this.store.ready()
    const origin = origemDe(this.store)
    try {
      return await op(client)
    } catch (e) {
      // The mark comes after the failure: a read already under way may have been answered before the write landed.
      if (resultadoIncerto(e)) this.unconfirmedWrite = { mark: this.connectors.marca(), origin }
      throw e
    } finally {
      await this.refresh()
    }
  }

  async addConnector(body: Parameters<typeof market.addConnector>[1]) {
    return this.write((client) => market.addConnector(client, body))
  }

  async importConnectors(config: string) {
    return this.write((client) => market.importConnectors(client, config))
  }

  async finishConnectorSignIn(retorno: RetornoLogin, cliente = () => this.store.ready()) {
    return this.after(market.finishConnectorSignIn(await cliente(), retorno))
  }

  /** Waits while the user signs in in the computer's browser (the host catches the return). */
  async waitForSignIn(id: string, cliente?: () => Promise<HostClient>) {
    for (let i = 0; i < 150; i++) {
      await sleep(2000)
      if (cliente) await cliente()
      await this.refresh()
      if (cliente) await cliente()
      const c = this.installedConnectors.find((c) => c.id === id)
      if (c && !needsSignIn(c)) return
    }
  }

  async removeConnector(id: string) {
    await this.after(market.removeConnector(await this.store.ready(), id))
  }

  /**
   * Signs in to a remote connector (host/src/market/oauth.rs). With an `app` callback the system
   * browser returns `codync://oauth?…` and the code goes to the host; with `host` the browser
   * returns to the host directly and we wait for the connector to flip to signed in; with `desktop`
   * (a Hub on another computer, reached through a tunnel) the browser returns to this app's own loopback
   * listener (main/fork-oauth-loopback.ts) and the code goes to the Hub over the authenticated channel.
   * Returns quietly when the user cancels.
   */
  async signIn(id: string, cliente?: () => Promise<HostClient>) {
    const retorno = window.codync.app.loginRetorno
    // O ouvinte do app só abre para o login que o Hub preparou para ele; sem porta livre, segue pelo caminho de antes.
    const ouvinte = await retorno.abrir().catch(() => null)
    let plan: Awaited<ReturnType<typeof market.connectorSignIn>>
    try {
      plan = await market.connectorSignIn(await (cliente ? cliente() : this.store.ready()), id, ouvinte?.porta)
      if (cliente) await cliente()
    } catch (e) {
      if (ouvinte) retorno.fechar(ouvinte.tentativa)
      throw e
    }
    const usaOuvinte = ouvinte !== null && plan.callback === 'desktop'
    // Hub antigo (não conhece a porta e responde pelo caminho de antes) ou endereço inválido: o ouvinte não serve.
    if (ouvinte && (!usaOuvinte || !URL.canParse(plan.url))) retorno.fechar(ouvinte.tentativa)
    if (!URL.canParse(plan.url)) return
    if (usaOuvinte) return this.finishWithAppListener(ouvinte.tentativa, plan.url, cliente)
    if (plan.callback !== 'app') {
      window.codync.app.openExternal(plan.url)
      await this.waitForSignIn(id, cliente)
      return
    }
    let back: string
    try {
      back = await window.codync.app.authenticate(plan.url, window.codync.appScheme)
    } catch {
      return
    }
    // M3-B: o Hub confere o emissor; aqui só se recusa retorno ambíguo.
    await this.finishConnectorSignIn(lerRetornoLogin(back), cliente)
  }

  /** O navegador volta ao ouvinte do app; o Hub confere state, PKCE e emissor, e o navegador vê o desfecho. */
  private async finishWithAppListener(tentativa: number, url: string, cliente?: () => Promise<HostClient>) {
    const retorno = window.codync.app.loginRetorno
    let back: string
    try {
      back = await retorno.esperar(tentativa, url)
    } catch {
      retorno.fechar(tentativa)
      return
    }
    try {
      const conector = await this.finishConnectorSignIn(lerRetornoLogin(back), cliente)
      retorno.responder(tentativa, { ok: true, nome: conector?.name ?? '' })
    } catch (e) {
      retorno.responder(tentativa, { ok: false, detalhe: errorText(e) })
      throw e
    }
  }
}

const byStore = new WeakMap<BotStore, Plugins>()

export function pluginsFor(store: BotStore) {
  let p = byStore.get(store)
  if (!p) byStore.set(store, (p = new Plugins(store)))
  return p
}

/** The store's plugin state, re-rendering on its changes. */
export const usePlugins = (store: BotStore) => useModel(pluginsFor(store))
