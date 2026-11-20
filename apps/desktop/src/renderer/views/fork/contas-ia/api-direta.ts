import type { HostClient } from '../../../client/host-client'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

// API direta (Conexões de IA): a chave de API de cada fornecedor para os bots do Motor Mexerica. O Hub guarda a
// chave cifrada no cofre deste computador, testa antes de salvar e nunca a devolve; aqui só ficam as regras da tela.

export interface ModeloDaChave {
  id: string
  name: string
}

export interface FornecedorDaChave {
  id: string
  nome: string
  configurada: boolean
  modelos: ModeloDaChave[] | null
  conferidoEm: number | null
}

export type Acesso = { ok: true; modelos: ModeloDaChave[] } | { ok: false; erro: string; status?: number }

/** O Hub mostra a seção quando o motor está ligado e o app fala com o Hub deste mesmo computador (métodos locais). */
export const mostrarApiDireta = (backends: readonly { id: string }[], isLoopback: boolean) =>
  isLoopback && backends.some((b) => b.id === 'mexerica')

const ERROS: Readonly<Record<string, ChaveTexto>> = {
  chave_invalida: 'contas.apiDireta.erro.chaveInvalida',
  sem_permissao: 'contas.apiDireta.erro.semPermissao',
  limite: 'contas.apiDireta.erro.limite',
  fornecedor_falhou: 'contas.apiDireta.erro.fornecedorFalhou',
  sem_resposta: 'contas.apiDireta.erro.semResposta',
  resposta_invalida: 'contas.apiDireta.erro.respostaInvalida',
  sem_chave: 'contas.apiDireta.erro.semChave',
}

/** A frase de um erro do teste; código desconhecido cai na falha genérica do fornecedor. */
export const chaveDoErro = (erro: string): ChaveTexto => ERROS[erro] ?? 'contas.apiDireta.erro.fornecedorFalhou'

/** Os primeiros nomes da lista de modelos, para a linha de resultado; o resto vira contagem. */
export function amostraDeModelos(modelos: readonly ModeloDaChave[], limite = 4): { nomes: string; restantes: number } {
  return { nomes: modelos.slice(0, limite).map((m) => m.name).join(', '), restantes: Math.max(0, modelos.length - limite) }
}

const PRAZO_MS = 60_000

export const listarChaves = async (client: HostClient) =>
  ((await client.call<{ fornecedores: FornecedorDaChave[] }>('motorChaves', {}, 15_000)).fornecedores)

export const testarChave = (client: HostClient, fornecedor: string) =>
  client.call<Acesso>('motorChaveTestar', { fornecedor }, PRAZO_MS)

/** A chave sai do estado da tela antes de qualquer espera de rede: `limparCampo` roda primeiro. */
export async function salvarChave(client: HostClient, fornecedor: string, digitada: string, limparCampo: () => void): Promise<Acesso> {
  let chave = digitada.trim()
  limparCampo()
  try {
    return await client.call<Acesso>('motorChaveSalvar', { fornecedor, chave }, PRAZO_MS)
  } finally {
    chave = ''
  }
}

export const removerChave = (client: HostClient, fornecedor: string) =>
  client.call<{ ok: boolean }>('motorChaveRemover', { fornecedor }, 15_000)
