// DON-865, fatia 1: regras puras do assistente de criar bot (views/criar-bot). Sem DOM nem React.
import { FUNCOES_EXEMPLO, funcoesProprias, type Funcao } from './fork-bot-funcao.ts'

export const PASSOS = ['papel', 'motor', 'permissoes', 'ferramentas'] as const
export type Passo = (typeof PASSOS)[number]

export const proximo = (passo: Passo): Passo | null => PASSOS[PASSOS.indexOf(passo) + 1] ?? null
export const anterior = (passo: Passo): Passo | null => PASSOS[PASSOS.indexOf(passo) - 1] ?? null
export const ultimo = (passo: Passo) => proximo(passo) === null

/**
 * D366: o bot principal é fixo e é o líder. A criação não oferece o papel nem o nome dele (a trava de verdade
 * é do Hub); as 30 funções continuam inteiras na biblioteca. D444/D445: o nome reservado passa a ser Berga, como
 * no Hub (`donna_fixa::reserved`); "Donna" fica livre para bots novos.
 */
export const NOME_RESERVADO = 'Berga'
export const FUNCOES_DA_CRIACAO: readonly Funcao[] = FUNCOES_EXEMPLO.filter((f) => f.id !== 'exemplo:donna')

/**
 * "Suas funções" na criação: as funções próprias dos bots, sem a principal (D366). A função
 * personalizada do Berga não vira opção para outro bot. A principal é a do Hub (`principalId`) ou,
 * enquanto ele não responde, a que tem a gestão da equipe.
 */
export function funcoesDaCriacao<B extends Parameters<typeof funcoesProprias>[0][number] & { id: string; manageTeam?: boolean | null }>(
  bots: readonly B[], principalId: string | null,
): Funcao[] {
  return funcoesProprias(bots.filter((b) => b.id !== principalId && b.manageTeam !== true))
}

/**
 * Nomes e valores que o catálogo do motor traz em inglês (opções do modelo, como o raciocínio). A tela mostra a
 * chave `botCreate.catalogo.<chave>` do idioma escolhido e preserva o ID; o que não é conhecido fica como veio.
 */
const CATALOGO = {
  reasoning: 'reasoning', 'reasoning effort': 'reasoning', effort: 'effort', thinking: 'reasoning', 'thought level': 'reasoning',
  fast: 'fast', 'fast mode': 'fast', mode: 'mode',
  none: 'none', minimal: 'minimal', low: 'low', medium: 'medium', high: 'high', xhigh: 'xhigh', 'extra high': 'xhigh',
  max: 'max', on: 'on', off: 'off', true: 'on', false: 'off', auto: 'auto', default: 'default',
} as const
export type ChaveCatalogo = (typeof CATALOGO)[keyof typeof CATALOGO]
export function chaveDoCatalogo(texto: string): ChaveCatalogo | null {
  return (CATALOGO as Record<string, ChaveCatalogo>)[texto.trim().toLowerCase().replace(/[_-]+/g, ' ')] ?? null
}

/** Os sete papéis comuns do passo 1, por ID da biblioteca; os 30 continuam em "Ver as 30 funções". */
export const PAPEIS_COMUNS = ['qa', 'dev', 'research', 'escritor', 'social', 'seo', 'suporte'] as const
export const papeisComuns = (): Funcao[] =>
  PAPEIS_COMUNS.map((id) => FUNCOES_EXEMPLO.find((f) => f.id === `exemplo:${id}`)).filter((f): f is Funcao => !!f)

/**
 * Nomes no estilo dos bots da biblioteca (uma palavra, do ofício), nunca de pessoas reais. São dados,
 * não texto de interface: não se traduzem. O primeiro de cada lista é o nome da própria biblioteca.
 */
const NOMES: Readonly<Record<string, readonly string[]>> = {
  qa: ['Crivo', 'Lupa', 'Prumo', 'Filtro', 'Peneira'],
  dev: ['Byte', 'Compila', 'Script', 'Commit', 'Bit'],
  research: ['Fonte', 'Pista', 'Índice', 'Bússola', 'Faro'],
  escritor: ['Prosa', 'Pena', 'Verso', 'Rascunho', 'Linha'],
  social: ['Feed', 'Story', 'Trend', 'Post', 'Eco'],
  seo: ['Rank', 'Busca', 'Topo', 'Snippet', 'Indexa'],
  suporte: ['Ajuda', 'Acolhe', 'Balcão', 'Atende', 'Socorro'],
}
const NOMES_GERAIS = ['Faísca', 'Pipoca', 'Rádio', 'Trilha', 'Brisa', 'Pulso', 'Farol', 'Bolha']

/** As opções do dado para uma função: as do papel comum, o nome da biblioteca, ou as gerais. */
export function nomesDoPapel(funcao: Funcao | null | undefined): readonly string[] {
  if (!funcao || funcao.propria) return NOMES_GERAIS
  const id = funcao.id.replace(/^exemplo:/, '')
  return (NOMES[id] ?? [funcao.nomeDoBot, ...NOMES_GERAIS]).filter((n) => !reservado(n))
}

const reservado = (nome: string) => nome.trim().toLocaleLowerCase('pt-BR') === NOME_RESERVADO.toLocaleLowerCase('pt-BR')

/** O dado: outro nome da lista, nunca o que já está no campo. `sorte` em [0, 1). */
export function sortearNome(opcoes: readonly string[], atual: string, sorte: number = Math.random()): string {
  const validas = opcoes.filter((n) => !reservado(n))
  const outros = validas.filter((n) => n !== atual.trim())
  const lista = outros.length ? outros : validas.length ? validas : NOMES_GERAIS
  return lista[Math.min(lista.length - 1, Math.floor(sorte * lista.length))] ?? NOMES_GERAIS[0]!
}

/**
 * Escolher Liberar na criação pede confirmação (D365, pergunta 1). Pedir vale na hora; Liberar só
 * fica marcado depois de `confirmar`.
 */
export type Autonomia = 'ask' | 'auto'
export function pedirAutonomia(atual: string, pedida: Autonomia): { aplicar: Autonomia | null; confirmar: boolean } {
  if (pedida === 'auto' && atual !== 'auto') return { aplicar: null, confirmar: true }
  return { aplicar: pedida, confirmar: false }
}
