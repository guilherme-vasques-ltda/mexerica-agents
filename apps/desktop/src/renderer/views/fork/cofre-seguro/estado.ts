import type { ConnectionRequest } from '../../marketplace/market-models'

// MX-209: o que o cartão da conversa mostra, tirado só da entrada (sem valor, nunca).
// Os textos da janela segura NÃO vêm daqui: o main os deriva da entrada no Hub.

export type FaseCartao = 'pendente' | 'guardado' | 'cancelado'

// `code` (MX-286): o código de login usa o mesmo pedido seguro e a mesma janela; o valor nunca passa pelo cartão.
export const ehPedidoSeguro = (request: ConnectionRequest) => request.kind === 'login' || request.kind === 'secret' || request.kind === 'code'

export function faseDo(request: ConnectionRequest): FaseCartao {
  return request.status === 'pending' ? 'pendente' : request.status === 'ready' ? 'guardado' : 'cancelado'
}

/** O nome (nunca o valor) com que o login ou a chave fica no cofre. */
export const nomeGuardado = (request: ConnectionRequest) => ({
  login: request.kind === 'login',
  site: request.site ?? request.title,
  campo: request.field ?? request.title,
  conector: request.title,
})
