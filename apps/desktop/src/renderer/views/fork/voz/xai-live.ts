import type { CallOperator } from '../../call/call-operator'
import { confereSessaoXai, iniciarEsperaXai, pedidoXai, sessaoXai, XAI_WS, type PedidoXai, type SelecaoXai } from '../../../../shared/fork-voz-xai.ts'
import { falhaDaCategoria } from './falha-segura'
import { TempoRealWS, type CredencialWS, type PortasTempoReal } from './tempo-real-ws'

export type PortasXai = PortasTempoReal

export class XaiLive extends TempoRealWS {
  constructor(selecao: SelecaoXai, credencial: (p: PedidoXai) => Promise<CredencialWS>, op: CallOperator, portas: PortasXai = {}) {
    const pedido: PedidoXai = { ...selecao, instructions: `${op.instructions}\nResponda no idioma ${selecao.language}; para pt-br, fale português do Brasil.` }
    super({
      provider: 'xai', url: XAI_WS, protocolos: secret => [`xai-client-secret.${secret}`],
      confere: sessao => confereSessaoXai(sessao, pedido), sessaoInicial: sessaoXai(pedido),
      iniciarEspera: () => iniciarEsperaXai(pedido.instructions),
    }, async () => {
      if (!pedidoXai(pedido)) throw falhaDaCategoria('invalida')
      return credencial({ ...pedido })
    }, op, portas)
  }
}
