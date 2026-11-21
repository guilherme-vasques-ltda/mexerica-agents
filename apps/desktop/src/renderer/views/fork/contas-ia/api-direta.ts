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

/** `estado`: modelos_no_catalogo, acesso_nao_confirmado, nao_verificado ou recusada. `id`: endereço próprio salvo. */
export type Acesso =
  | { ok: true; modelos: ModeloDaChave[]; estado?: string; id?: string }
  | { ok: false; erro: string; status?: number; estado?: string; salvo?: boolean; id?: string; modelos?: ModeloDaChave[] }

export type FormatoProprio = 'chat-completions' | 'responses' | 'anthropic-messages'
export const FORMATOS: readonly FormatoProprio[] = ['chat-completions', 'responses', 'anthropic-messages']

/** Um endereço próprio cadastrado: tudo menos a chave, que nunca sai do Hub. */
export interface EnderecoProprio {
  id: string
  nome: string
  url: string
  formato: FormatoProprio
  local: boolean
  configurada: boolean
  modelos: ModeloDaChave[] | null
  conferidoEm: number | null
}

export interface NovoEndereco {
  nome: string
  url: string
  formato: FormatoProprio
  local: boolean
  chave: string
  modelo: string
}

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
  sem_lista: 'contas.apiDireta.erro.semLista',
  destino_recusado: 'contas.apiDireta.erro.destinoRecusado',
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
  if (acesso.estado === 'acesso_nao_confirmado') return 'acesso_nao_confirmado'
  if (acesso.ok) return acesso.modelos.length > 0 ? 'modelos_no_catalogo' : 'conta_reconhecida'
  if (acesso.erro === 'chave_invalida') return 'recusada'
  if (acesso.erro === 'limite' || acesso.erro === 'sem_lista') return 'acesso_nao_confirmado'
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

export const listarConexoes = async (client: HostClient) => {
  const r = await client.call<{ fornecedores: FornecedorDaChave[]; enderecos?: EnderecoProprio[] }>('motorChaves', {}, 15_000)
  return { fornecedores: r.fornecedores, enderecos: r.enderecos ?? [] }
}

export const listarChaves = async (client: HostClient) => (await listarConexoes(client)).fornecedores

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

/** Endereço próprio: a chave sai do estado da tela antes da espera, como em `salvarChave`. */
export async function salvarEndereco(client: HostClient, dados: NovoEndereco, limparChave: () => void): Promise<Acesso> {
  const pedido = { nome: dados.nome.trim(), url: dados.url.trim(), formato: dados.formato, local: dados.local, chave: dados.chave.trim(), modelo: dados.modelo.trim() || undefined }
  limparChave()
  try {
    return await client.call<Acesso>('motorEnderecoSalvar', pedido, PRAZO_MS)
  } finally {
    pedido.chave = ''
  }
}

/** O formulário só envia o que o Hub aceitaria; a regra de verdade (e o portão de rede) fica no Hub e no motor. */
export function enderecoPronto(d: NovoEndereco): boolean {
  if (!d.nome.trim() || !d.chave.trim()) return false
  try {
    const url = new URL(d.url.trim())
    return url.protocol === 'https:' || (d.local && url.protocol === 'http:')
  } catch {
    return false
  }
}

/** O formulário do endereço próprio só fecha com o acesso confirmado: sem confirmação, o aviso fica na tela em
 * vez de sumir junto com o formulário. */
export function fechaDepoisDeSalvar(r: Acesso): boolean {
  return r.ok && r.estado !== 'acesso_nao_confirmado'
}
