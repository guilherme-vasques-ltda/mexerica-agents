import type { HostClient } from '../../../client/host-client'

// Planos de código (Conexões de IA, grupo «Planos»): a chave do plano é colada, o Hub a guarda cifrada no cofre deste
// computador, testa antes de salvar e nunca a devolve. O OpenCode é o motor. A tela «Adicionar conexão» só lista.

export interface ModeloDoPlano {
  id: string
  name: string
}

export interface PlanoDeCodigo {
  id: string
  nome: string
  grupo: 'planos'
  /** Códigos dos avisos que a tela mostra junto do plano (ex.: `saldo_extra`, `so_conversa`). */
  avisos: string[]
  /** O fornecedor só permite o plano em conversa da própria pessoa: rotinas e tarefas agendadas não o usam. */
  soConversa: boolean
  configurada: boolean
  /** O programa do OpenCode está instalado neste computador. */
  programaInstalado: boolean
  modelos: ModeloDoPlano[] | null
  conferidoEm: number | null
}

/**
 * `modelos_no_catalogo`: o fornecedor respondeu à chave com a lista, o que não prova saldo nem permissão de conversar.
 * `acesso_nao_confirmado`: o fornecedor não tem lista para testar; a chave foi guardada pelo formato e os modelos são os da
 * documentação, e só a primeira conversa confirma o acesso.
 * `recusada`: o fornecedor recusou a chave. `nao_verificado`: falha que não diz nada sobre a chave.
 */
export type AcessoDoPlano =
  | { ok: true; estado: 'modelos_no_catalogo' | 'acesso_nao_confirmado'; modelos: ModeloDoPlano[] }
  | { ok: false; erro: string; estado: 'recusada' | 'nao_verificado' }

const PRAZO_MS = 60_000

export const listarPlanos = async (client: HostClient) =>
  (await client.call<{ planos: PlanoDeCodigo[] }>('planosCodigo', {}, 15_000)).planos

export const testarPlano = (client: HostClient, plano: string) =>
  client.call<AcessoDoPlano>('planoCodigoTestar', { plano }, PRAZO_MS)

/** A chave sai do estado da tela antes de qualquer espera de rede: `limparCampo` roda primeiro. */
export async function salvarPlano(client: HostClient, plano: string, digitada: string, limparCampo: () => void): Promise<AcessoDoPlano> {
  let chave = digitada.trim()
  limparCampo()
  try {
    return await client.call<AcessoDoPlano>('planoCodigoSalvar', { plano, chave }, PRAZO_MS)
  } finally {
    chave = ''
  }
}

export const removerPlano = (client: HostClient, plano: string) =>
  client.call<{ ok: boolean }>('planoCodigoRemover', { plano }, PRAZO_MS)
