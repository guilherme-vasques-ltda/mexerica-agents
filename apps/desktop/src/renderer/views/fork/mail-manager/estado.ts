// Mail Manager: a leitura e os pedidos do dono ao Hub (`donnaMailManager` e companhia). Uma instância por Hub;
// uma resposta antiga (outro cliente, outra leitura) nunca sobrescreve a atual. Erro de leitura ≠ lista vazia.

import { Observable } from '../../../lib/observable.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { caixasPreviaFaltantes, conferirLeitura, erroDoHub, SEM_CONEXAO_MSG, type LeituraMailManager, type PermissaoMail } from './regras.ts'

export interface Chamador { call<T>(method: string, body?: unknown, timeoutMs?: number): Promise<T> }

export class EstadoMailManager extends Observable {
  leitura: LeituraMailManager | null = null
  carregando = false
  /** Falha ao ler: a tela mostra o erro, nunca uma lista vazia. Guardada como dado, traduzida ao desenhar. */
  erroLeitura: Mensagem | null = null
  /** Falha do último pedido; a leitura anterior continua na tela. */
  erroPedido: Mensagem | null = null
  salvando = false
  #geracao = 0
  readonly #cliente: () => Chamador | null

  constructor(cliente: () => Chamador | null) { super(); this.#cliente = cliente }

  async ler() {
    const geracao = ++this.#geracao
    const c = this.#cliente()
    if (!c) { this.leitura = null; this.erroLeitura = SEM_CONEXAO_MSG; this.changed(); return }
    this.carregando = true
    this.changed()
    try {
      const leitura = conferirLeitura(await c.call('donnaMailManager', {}))
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

  salvarPlugin(enabled: boolean) { return this.#pedir('donnaMailManagerSave', { enabled }) }

  conectar(label: string, addresses: string[]) { return this.#pedir('donnaMailManagerConnect', { label, addresses }) }

  desconectar(mailboxId: string) { return this.#pedir('donnaMailManagerDisconnect', { mailboxId }) }

  alterarPermissao(mailboxId: string, botId: string, permission: PermissaoMail, allowed: boolean) {
    return this.#pedir('donnaMailManagerGrant', { mailboxId, participantKind: 'bot', participantId: botId, permission, allowed })
  }

  /**
   * A prévia: conecta em sequência só as caixas fictícias que faltam, sem conceder acesso a nenhum bot.
   * Uma falha para a sequência e fica na tela; o dono tenta de novo.
   */
  async carregarPrevia(rotulos: { suporte: string; operacoes: string }) {
    for (const caixa of caixasPreviaFaltantes(this.leitura)) {
      await this.conectar(rotulos[caixa.chave], [caixa.endereco])
      if (this.erroPedido) break
    }
  }

  /**
   * `true` só quando o pedido completou e a resposta dele é a leitura vigente;
   * `false` quando foi ignorado (outro pedido em andamento, sem cliente, resposta antiga) ou falhou.
   */
  async #pedir(method: string, body: unknown): Promise<boolean> {
    const c = this.#cliente()
    if (this.salvando) return false
    if (!c) { this.erroPedido = SEM_CONEXAO_MSG; this.changed(); return false }
    const geracao = ++this.#geracao
    this.salvando = true
    this.erroPedido = null
    this.changed()
    try {
      const leitura = conferirLeitura(await c.call<Record<string, unknown>>(method, body))
      if (geracao !== this.#geracao) return false
      this.leitura = leitura
      return true
    } catch (e) {
      // Erro do pedido sempre aparece; só a leitura antiga é descartada.
      this.erroPedido = erroDoHub(e)
      return false
    } finally {
      this.salvando = false
      this.changed()
    }
  }
}

const porDono = new WeakMap<object, EstadoMailManager>()

/** Uma leitura compartilhada por loja do app (um Hub), como a do WhatsApp: ligar aqui muda lá na hora. */
export function estadoMailDe(dono: object, cliente: () => Chamador | null): EstadoMailManager {
  let e = porDono.get(dono)
  if (!e) porDono.set(dono, (e = new EstadoMailManager(cliente)))
  return e
}
