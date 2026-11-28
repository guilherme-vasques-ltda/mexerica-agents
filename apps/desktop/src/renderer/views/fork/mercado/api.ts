// As chamadas do Marketplace ao Hub (corpo e tempo de espera). Sem regra: só envia a intenção e devolve a resposta.
import type { HostClient } from '../../../client/host-client.ts'
import type { AppGoogle, AppProprio, Catalogo, Conta, FerramentasDaConta, PedidoDefinirVinculo, PedidoLigarFerramenta, ProximoPasso, ResultadoDesconectar, VinculosDoBot } from './tipos.ts'

const s = (n: number) => n * 1000

export interface PedidoConectar {
  servico: string
  tipo: string
  spaceId?: string
  nome?: string
  nomePrimeira?: string
  liberacoes?: string[]
  aceitaCredencialComEscrita?: true
}

/** O MCP do próprio usuário (`marketPersonalizado`): link ou comando; a chave segue no cabeçalho e vai direto ao cofre. */
export interface PedidoPersonalizado {
  name: string
  url?: string
  command?: string
  headers?: Record<string, string>
}

export const mercadoApi = {
  catalogo: (c: HostClient) => c.call<Catalogo>('marketCatalogo', {}, s(30)),
  contas: async (c: HostClient) => (await c.call<{ items: Conta[] }>('marketContas', {}, s(30))).items,
  conectar: (c: HostClient, pedido: PedidoConectar) =>
    c.call<{ connector: Conta; proximoPasso: ProximoPasso }>('marketConectar', pedido, s(60)),
  renomear: async (c: HostClient, id: string, nome: string) => (await c.call<{ connector: Conta }>('marketContaRenomear', { id, nome }, s(30))).connector,
  personalizado: (c: HostClient, pedido: PedidoPersonalizado) =>
    c.call<{ connector: Conta; proximoPasso: 'entrar' | 'verificar' }>('marketPersonalizado', pedido, s(60)),
  desconectar: (c: HostClient, id: string) => c.call<ResultadoDesconectar>('marketContaDesconectar', { id }, s(60)),
  /** Só o ID volta; a chave secreta entra uma vez e fica no cofre do Hub. */
  salvarAppGoogle: (c: HostClient, clientId: string, clientSecret: string) => c.call<AppGoogle>('marketGoogleAppSalvar', { clientId, clientSecret }, s(30)),
  removerAppGoogle: (c: HostClient) => c.call<AppGoogle>('marketGoogleAppRemover', {}, s(30)),
  salvarAppProprio: (c: HostClient, alvo: Pick<PedidoConectar, 'servico' | 'tipo' | 'spaceId'>, clientId: string, clientSecret: string) =>
    c.call<AppProprio>('marketAppProprioSalvar', { ...alvo, clientId, clientSecret }, s(30)),
  removerAppProprio: (c: HostClient, alvo: Pick<PedidoConectar, 'servico' | 'tipo' | 'spaceId'>) =>
    c.call<AppProprio>('marketAppProprioRemover', alvo, s(30)),
  botContas: (c: HostClient, botId: string) => c.call<VinculosDoBot>('marketBotContas', { botId }, s(30)),
  definirBotContas: (c: HostClient, pedido: PedidoDefinirVinculo) => c.call<VinculosDoBot>('marketBotContasDefinir', pedido, s(30)),
  ferramentas: (c: HostClient, botId: string, conta: string) => c.call<FerramentasDaConta>('marketFerramentas', { botId, conta }, s(30)),
  ligarFerramenta: (c: HostClient, pedido: PedidoLigarFerramenta) => c.call<FerramentasDaConta>('marketFerramentaLigar', pedido, s(30)),
  desligarFerramenta: (c: HostClient, botId: string, conta: string, ferramenta: string) => c.call<FerramentasDaConta>('marketFerramentaDesligar', { botId, conta, ferramenta }, s(30)),
}
