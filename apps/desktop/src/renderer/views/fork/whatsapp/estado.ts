import { Observable } from '../../../lib/observable.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { conferirFila, type ItemFila } from './fila.ts'
import { conferirLeitura, erroDoHub, LOGOUT_FALHOU, SEM_CONEXAO_MSG, type LeituraWhatsapp, type Logout, type Modo, type PatchBot } from './regras.ts'

// whatsapp R5: a leitura e os pedidos do dono ao Hub (RPCs do R1 e do número, R2). Uma instância por Hub; uma resposta
// antiga (outro cliente, outra leitura) nunca sobrescreve a atual. Erro de leitura ≠ lista vazia.

export interface Chamador { call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T> }

/** O Hub espera até 20 s pelo primeiro código/QR antes de responder ao pedido de pareamento. */
const ESPERA_PAREAR_MS = 35_000
/** Desconectar espera o wacli parar e avisar o WhatsApp. */
const ESPERA_DESCONECTAR_MS = 40_000

export class EstadoWhatsapp extends Observable {
  leitura: LeituraWhatsapp | null = null
  carregando = false
  /** Falha ao ler: a tela mostra o erro, nunca uma lista vazia. Guardada como dado (D507), traduzida ao desenhar. */
  erroLeitura: Mensagem | null = null
  /** Falha do último pedido; a leitura anterior continua na tela. */
  erroPedido: Mensagem | null = null
  salvando = false
  /** Resultado do último Desconectar: o aviso ao WhatsApp falhou → o dono tira o aparelho no celular. */
  avisoLogout: { botId: string; mensagem: Mensagem } | null = null
  /** MX-18: a fila de saída por bot (pendentes, seguradas, recentes enviadas e incertas). */
  readonly filas = new Map<string, { items: ItemFila[] | null; erro: Mensagem | null; carregando: boolean }>()
  #geracao = 0
  #filaGeracao = new Map<string, number>()
  readonly #cliente: () => Chamador | null

  constructor(cliente: () => Chamador | null) { super(); this.#cliente = cliente }

  async ler() {
    const geracao = ++this.#geracao
    const c = this.#cliente()
    if (!c) { this.leitura = null; this.erroLeitura = SEM_CONEXAO_MSG; this.changed(); return }
    this.carregando = true
    this.changed()
    try {
      const leitura = conferirLeitura(await c.call('donnaWhatsapp', {}))
      if (geracao !== this.#geracao) return
      this.leitura = leitura
      this.erroLeitura = null
    } catch (e) {
      if (geracao !== this.#geracao) return
      this.leitura = null
      this.erroLeitura = erroDoHub(e)
    } finally {
      if (geracao === this.#geracao) { this.carregando = false; this.changed() }
    }
  }

  salvarHub(patch: { enabled?: boolean; mode?: Modo }) { return this.#pedir('donnaWhatsappSave', patch) }

  salvarBot(botId: string, settings: PatchBot) { return this.#pedir('donnaWhatsappBotSave', { botId, settings }) }

  parear(botId: string, method: 'code' | 'qr', phone?: string) {
    return this.#pedir('donnaWhatsappPair', method === 'code' ? { botId, method, phone } : { botId, method }, ESPERA_PAREAR_MS)
  }

  cancelarPareamento(botId: string) { return this.#pedir('donnaWhatsappPairCancel', { botId }, undefined, true) }

  desconectar(botId: string, erase: boolean) {
    this.avisoLogout = null
    return this.#pedir('donnaWhatsappDisconnect', { botId, erase }, ESPERA_DESCONECTAR_MS)
  }

  /** R7: depois de uma falha, o Hub só baixa o wacli de novo quando o dono pede. */
  tentarWacli() { return this.#pedir('donnaWhatsappWacliRetry', {}) }

  /** MX-18: relê a fila de saída do bot; erro fica na tela, nunca lista vazia. */
  async lerFila(botId: string) {
    const c = this.#cliente()
    const geracao = (this.#filaGeracao.get(botId) ?? 0) + 1
    this.#filaGeracao.set(botId, geracao)
    const antes = this.filas.get(botId)
    if (!c) {
      this.filas.set(botId, { items: antes?.items ?? null, erro: SEM_CONEXAO_MSG, carregando: false })
      return this.changed()
    }
    this.filas.set(botId, { items: antes?.items ?? null, erro: null, carregando: true })
    this.changed()
    try {
      const items = conferirFila(await c.call('donnaWhatsappQueue', { botId }))
      if (this.#filaGeracao.get(botId) === geracao) this.filas.set(botId, { items, erro: null, carregando: false })
    } catch (e) {
      if (this.#filaGeracao.get(botId) === geracao) this.filas.set(botId, { items: null, erro: erroDoHub(e), carregando: false })
    }
    this.changed()
  }

  /** MX-18: o dono decide o incerto ("enviou"/"não saiu") ou cancela o que espera. O Hub devolve a fila nova. */
  async decidirFila(botId: string, itemId: string, decision: 'sent' | 'cancelled') {
    const c = this.#cliente()
    if (!c) { this.erroPedido = SEM_CONEXAO_MSG; this.changed(); return }
    try {
      const items = conferirFila(await c.call('donnaWhatsappQueueDecide', { botId, itemId, decision }))
      this.filas.set(botId, { items, erro: null, carregando: false })
    } catch (e) {
      this.erroPedido = erroDoHub(e)
    }
    this.changed()
  }

  /** `forcar`: cancelar passa mesmo com outro pedido em curso (o pareamento segura o pedido por até 20 s). */
  async #pedir(method: string, body: unknown, timeoutMs?: number, forcar = false) {
    const c = this.#cliente()
    if (this.salvando && !forcar) return
    if (!c) { this.erroPedido = SEM_CONEXAO_MSG; this.changed(); return }
    const geracao = ++this.#geracao
    this.salvando = true
    this.erroPedido = null
    this.changed()
    try {
      const resposta = await c.call<Record<string, unknown>>(method, body, timeoutMs)
      const leitura = conferirLeitura(resposta)
      const logout = resposta.disconnect as Logout | undefined
      // O aviso vale mesmo se uma leitura nova (evento do Hub) passou na frente.
      if (logout?.logout === 'failed') this.avisoLogout = { botId: (body as { botId: string }).botId, mensagem: logout.warning ? { texto: logout.warning } : LOGOUT_FALHOU }
      if (geracao === this.#geracao) this.leitura = leitura
    } catch (e) {
      // Erro do pedido sempre aparece; só a leitura antiga é descartada.
      this.erroPedido = erroDoHub(e)
    } finally {
      this.salvando = false
      this.changed()
    }
  }
}

const porDono = new WeakMap<object, EstadoWhatsapp>()

/**
 * D350: uma leitura compartilhada por loja do app (um Hub). Ligar ou desligar o plugin em Ajustes → Plugins muda na
 * hora a mesma leitura que as Configurações do bot usam para mostrar ou esconder a seção WhatsApp.
 */
export function estadoDe(dono: object, cliente: () => Chamador | null): EstadoWhatsapp {
  let e = porDono.get(dono)
  if (!e) porDono.set(dono, (e = new EstadoWhatsapp(cliente)))
  return e
}
