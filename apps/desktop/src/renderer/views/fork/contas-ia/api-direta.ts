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

/**
 * Os quatro estados do «Testar acesso». O teste só lê a lista de modelos do fornecedor e nunca gera texto, então
 * «tudo certo» nunca é afirmado: uma lista não prova saldo, assinatura nem permissão de cada modelo.
 * - `conta_reconhecida`: o fornecedor aceitou a chave, mas não listou nada;
 * - `modelos_no_catalogo`: o fornecedor aceitou a chave e listou modelos;
 * - `acesso_nao_confirmado`: o fornecedor respondeu, mas limite ou crédito impedem confirmar;
 * - `nao_verificado`: sem resposta, resposta quebrada ou sem permissão de listar (não é chave inválida).
 * A recusa explícita do fornecedor (`recusada`) é a única que dá a chave como inválida.
 */
export type EstadoDoTeste = 'conta_reconhecida' | 'modelos_no_catalogo' | 'acesso_nao_confirmado' | 'nao_verificado' | 'recusada'

export function estadoDoTeste(acesso: Acesso): EstadoDoTeste {
  if (acesso.ok) return acesso.modelos.length > 0 ? 'modelos_no_catalogo' : 'conta_reconhecida'
  if (acesso.erro === 'chave_invalida') return 'recusada'
  if (acesso.erro === 'limite') return 'acesso_nao_confirmado'
  return 'nao_verificado'
}

/** A frase de um erro do teste; código desconhecido cai na falha genérica do fornecedor. */
export const chaveDoErro = (erro: string): ChaveTexto => (Object.hasOwn(ERROS, erro) ? ERROS[erro]! : 'contas.apiDireta.erro.fornecedorFalhou')

/** Os primeiros nomes da lista de modelos, para a linha de resultado; o resto vira contagem. */
export function amostraDeModelos(modelos: readonly ModeloDaChave[], limite = 4): { nomes: string; restantes: number } {
  return { nomes: modelos.slice(0, limite).map((m) => m.name).join(', '), restantes: Math.max(0, modelos.length - limite) }
}

const PRAZO_MS = 60_000

// Chave salva, trocada, removida ou testada: quem mostra a lista de chaves (a seção da página e a tela «Adicionar conexão») relê.
const ouvintesDeChaves = new Set<() => void>()
export const chavesMudaram = () => { for (const ouvinte of ouvintesDeChaves) ouvinte() }
export const aoMudarChaves = (ouvinte: () => void) => {
  ouvintesDeChaves.add(ouvinte)
  return () => void ouvintesDeChaves.delete(ouvinte)
}

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

// O Hub faz uma operação de chave por vez: remover pode esperar um teste em andamento terminar.
export const removerChave = (client: HostClient, fornecedor: string) =>
  client.call<{ ok: boolean }>('motorChaveRemover', { fornecedor }, PRAZO_MS)
