// Marketplace (fatia 1b): o estado do app sobre o que o Hub diz. A tela só renderiza e chama estes métodos; cada
// escrita é uma intenção enviada ao Hub, e a lista é relida depois (nunca se presume o resultado).
import type { HostClient } from '../../../client/host-client'
import { Observable, useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { market } from '../../marketplace/market-models'
import { pluginsFor } from '../../marketplace/plugins'
import { mesmaOrigem, origemDe, type EstadoLista, type Origem } from '../meus-mcp/estado'
import { mercadoApi, type PedidoPersonalizado } from './api'
import { FluxoConectar } from './fluxo'
import { FluxoGoogle } from './google-fluxo'
import { cofreTrancado, ehDoGoogle } from './regras'
import { isOpenDesktop } from '../../../../shared/fork-capacidades'
import type { AppGoogle, Catalogo, Conta, FerramentasDaConta, PedidoDefinirVinculo, PedidoLigarFerramenta, ResultadoDesconectar, Servico, TipoConexao, VinculosDoBot } from './tipos'

const mensagem = (e: unknown) => (e instanceof Error ? e.message : String(e))

// ───────────────────────────── O Marketplace de um computador/Space ─────────────────────────────

// A Open não tem o executor isolado da conferência do Hub: o login só chega aqui com o fim confirmado pelo Hub, e a chave guardada fica "salva, não verificada" no fluxo.
const conferirConta = (c: HostClient, id: string) =>
  isOpenDesktop(globalThis.window?.codync) ? Promise.resolve({ status: 'ready' }) : market.verifyConnector(c, id)

export class Mercado extends Observable {
  private lido: { origem: Origem<HostClient>; catalogo: Catalogo; contas: Conta[]; contasErro: string | null } | null = null
  private falha: { origem: Origem<HostClient>; texto: string } | null = null
  private pedido = 0
  /** A aba de Conectores aberta: o catálogo ou "Minhas contas" (o link da tela do bot abre esta). */
  aba: 'catalogo' | 'contas' = 'catalogo'

  constructor(readonly store: BotStore) {
    super()
  }

  /** Só o que a origem de agora respondeu: dado de outro Hub ou outro link nunca aparece. */
  get catalogo(): Catalogo | null { return this.lido && mesmaOrigem(this.store, this.lido.origem) ? this.lido.catalogo : null }
  get contas(): Conta[] { return this.lido && mesmaOrigem(this.store, this.lido.origem) ? this.lido.contas : [] }
  /** Por que as contas não vieram (o cofre trancado, por exemplo); o catálogo, que é dado público, vem do mesmo jeito. */
  get contasErro(): string | null { return this.lido && mesmaOrigem(this.store, this.lido.origem) ? this.lido.contasErro : null }
  /** O Hub disse que o cofre está trancado (`contas: null` no catálogo); outras falhas de `contasErro` não têm causa dita. */
  get cofreTrancado(): boolean { return cofreTrancado(this.catalogo) }
  get erro(): string | null { return this.falha && mesmaOrigem(this.store, this.falha.origem) ? this.falha.texto : null }
  get estado(): EstadoLista {
    if (!origemDe(this.store)) return 'semConexao'
    if (this.erro !== null) return 'erro'
    return this.lido && mesmaOrigem(this.store, this.lido.origem) ? 'lida' : 'lendo'
  }

  async ler(): Promise<void> {
    const origem = origemDe(this.store)
    if (!origem) { this.changed(); return }
    const meu = ++this.pedido
    const vale = () => meu === this.pedido && mesmaOrigem(this.store, origem)
    try {
      const [catalogo, contas] = await Promise.allSettled([mercadoApi.catalogo(origem.client),
        mercadoApi.contas(origem.client)])
      if (!vale()) return
      if (catalogo.status === 'rejected') throw catalogo.reason
      this.lido = { origem, catalogo: catalogo.value, contas: contas.status === 'fulfilled' ? contas.value : [], contasErro: contas.status === 'rejected' ? mensagem(contas.reason) : null }
      this.falha = null
    } catch (e) {
      if (!vale()) return
      this.falha = { origem, texto: mensagem(e) }
    }
    this.changed()
  }

  /** O conector próprio que acabou de ser adicionado: "Minhas contas" o anuncia uma vez (a tela v3 02b). */
  recemAdicionado: string | null = null

  abrirAba(aba: 'catalogo' | 'contas') { if (this.aba !== aba || this.recemAdicionado !== null) { this.aba = aba; if (aba !== 'contas') this.recemAdicionado = null; this.changed() } }

  /**
   * MCP do próprio usuário: o Hub cria a conta (nasce desligada em todos os bots); se o servidor pede login, o login
   * abre aqui e depois vem a conferência. Um login que não termina não desfaz a conta: ela aparece em Minhas contas
   * pedindo para reconectar.
   */
  async adicionarPersonalizado(pedido: PedidoPersonalizado): Promise<void> {
    const cliente = await this.store.ready()
    const r = await mercadoApi.personalizado(cliente, pedido)
    try {
      if (r.proximoPasso === 'entrar') await pluginsFor(this.store).signIn(r.connector.id)
      await conferirConta(cliente, r.connector.id)
    } catch { /* a conta existe; a tela de contas mostra o estado e o botão Reconectar */ } finally {
      await this.ler()
    }
  }

  servico(id: string): Servico | null { return this.catalogo?.servicos.find((s) => s.id === id) ?? null }
  conta(id: string): Conta | null { return this.contas.find((c) => c.id === id) ?? null }

  /** Uma escrita e, de qualquer jeito, uma leitura nova: um resultado incerto nunca é tomado por feito ou desfeito. */
  private async depois<T>(op: Promise<T>): Promise<T> {
    try { return await op } finally { await this.ler() }
  }

  async renomear(id: string, nome: string): Promise<void> {
    await this.depois(this.store.ready().then((c) => mercadoApi.renomear(c, id, nome)))
  }

  /** Desconecta só esta conta; o Hub lista os bots que a usavam e diz que não revogou no fornecedor. */
  async desconectar(id: string): Promise<ResultadoDesconectar> {
    return this.depois(this.store.ready().then((c) => mercadoApi.desconectar(c, id)))
  }

  /** Reconectar uma conta expirada: o mesmo login de sempre, depois a conferência. */
  async reconectar(id: string): Promise<void> {
    await pluginsFor(this.store).signIn(id)
    await this.depois(this.store.ready().then((c) => conferirConta(c, id)))
  }

  /** Liga estas contas nos bots marcados (nasce tudo desligado); um bot por vez, parando no primeiro erro. */
  async usarNosBots(contaId: string, botIds: readonly string[]): Promise<void> {
    const cliente = await this.store.ready()
    try {
      for (const botId of botIds) await mercadoApi.definirBotContas(cliente, { botId, ligar: [contaId] })
    } finally {
      await this.ler()
    }
  }

  /** O app Google do Space (só o ID); `null` com o cofre trancado ou em Hub antigo. */
  get appGoogle(): AppGoogle | null { return this.catalogo?.appsProprios?.google ?? null }

  /** Conectar os serviços do Google com o app da própria pessoa: o guia (sem app), a escolha e o login em fila. */
  fluxoGoogle(): FluxoGoogle | null {
    const catalogo = this.catalogo
    if (!catalogo) return null
    return new FluxoGoogle(catalogo.servicos.filter(ehDoGoogle), this.appGoogle, {
      cliente: () => this.store.ready(),
      reler: () => this.ler(),
      criarFluxo: (servico, tipo) => this.fluxo(servico, tipo),
    })
  }

  fluxo(servico: Servico, tipo: TipoConexao): FluxoConectar {
    const catalogo = this.catalogo
    const cadastro = catalogo && tipo.auth.modo === 'app_proprio' && tipo.auth.configuracao
      ? { app: catalogo.appsProprios?.[tipo.id] ?? null, spaceId: catalogo.spaceId, redirects: catalogo.oauthRedirectUris ?? [] }
      : undefined
    const origem = origemDe(this.store)
    const cliente = async () => {
      if (!cadastro) return this.store.ready()
      if (origem && mesmaOrigem(this.store, origem)) return origem.client
      throw new Error('ficha_indisponivel:')
    }
    return new FluxoConectar(servico, tipo, {
      cliente,
      entrar: (id) => pluginsFor(this.store).signIn(id, cadastro ? cliente : undefined),
      verificar: (id) => cliente().then((c) => conferirConta(c, id)),
      guardarChave: (id, campos) => cliente().then((c) => market.updateConnectorCredentials(c, id, campos) as Promise<{ verified?: boolean; aviso?: string | null } | null>),
      reler: () => this.ler(),
    }, cadastro)
  }
}

const porStore = new WeakMap<BotStore, Mercado>()

export function mercadoPara(store: BotStore): Mercado {
  let m = porStore.get(store)
  if (!m) porStore.set(store, (m = new Mercado(store)))
  return m
}

export const useMercado = (store: BotStore) => useModel(mercadoPara(store))

/** O pedido de ligar contas em bots, para quem o monta fora do fluxo (tela 05). */
export const definirNoBot = (store: BotStore, pedido: PedidoDefinirVinculo): Promise<VinculosDoBot> => store.ready().then((c) => mercadoApi.definirBotContas(c, pedido))
export const lerDoBot = (store: BotStore, botId: string): Promise<VinculosDoBot> => store.ready().then((c) => mercadoApi.botContas(c, botId))

/** Ações perigosas (fatia 2): ler e escrever as liberações de UM bot; quem decide é o Hub, que confere o digest e a confirmação. */
export const ferramentasDoBot = (store: BotStore, botId: string, conta: string): Promise<FerramentasDaConta> => store.ready().then((c) => mercadoApi.ferramentas(c, botId, conta))
export const ligarFerramenta = (store: BotStore, pedido: PedidoLigarFerramenta): Promise<FerramentasDaConta> => store.ready().then((c) => mercadoApi.ligarFerramenta(c, pedido))
export const desligarFerramenta = (store: BotStore, botId: string, conta: string, ferramenta: string): Promise<FerramentasDaConta> => store.ready().then((c) => mercadoApi.desligarFerramenta(c, botId, conta, ferramenta))
