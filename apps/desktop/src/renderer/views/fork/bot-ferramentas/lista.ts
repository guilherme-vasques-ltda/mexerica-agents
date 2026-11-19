// MX-84 (retorno do dono, 10/11/2026): a lista de uma aba de Bot → Ferramentas. Puro, sem React: o índice de busca
// é montado uma vez por lista; cada tecla só compara textos já normalizados e a página é uma fatia do que sobrou.

export const POR_PAGINA = 15

export interface ItemDeFerramenta {
  id: string
  /** Como o Hub manda. Num app com mais de uma conta o rótulo já vem no nome ("Stripe · Loja A"). */
  name: string
  detail: string
}

export interface Indexado<T> { item: T; chave: string; palavras: string[] }

/** Sem acento e sem caixa: "calendario" acha "Calendário". */
export const normalizar = (texto: string) => texto.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase().trim()
const emPalavras = (texto: string) => texto.split(/[^\p{L}\p{N}]+/u).filter(Boolean)

export function indexar<T extends ItemDeFerramenta>(itens: readonly T[]): Indexado<T>[] {
  return itens.map((item) => { const chave = normalizar(`${item.name} ${item.detail}`).replace(/\s+/g, ' '); return { item, chave, palavras: emPalavras(chave) } })
}

/**
 * Filtra o conjunto todo (nunca só a página). Acha quando o texto buscado aparece inteiro, em qualquer ponto, ou
 * quando cada palavra da busca COMEÇA uma palavra do nome ou do detalhe. Começo de palavra, e não qualquer pedaço:
 * "Loja B" não pode achar a "Loja A" só porque existe um "b" no meio de "Cobranças".
 */
export function filtrar<T>(indice: readonly Indexado<T>[], busca: string): T[] {
  const frase = normalizar(busca).replace(/\s+/g, ' ')
  const palavras = emPalavras(frase)
  if (!frase) return indice.map((i) => i.item)
  return indice.filter((i) => i.chave.includes(frase) || (palavras.length > 0 && palavras.every((p) => i.palavras.some((w) => w.startsWith(p))))).map((i) => i.item)
}

export interface Pagina<T> { itens: T[]; pagina: number; paginas: number; de: number; ate: number; total: number }

/** A página pedida, presa ao que existe (a lista pode encolher com a busca ou com uma remoção). */
export function paginar<T>(itens: readonly T[], pagina: number, porPagina = POR_PAGINA): Pagina<T> {
  const total = itens.length
  const paginas = Math.max(1, Math.ceil(total / porPagina))
  const atual = Math.min(Math.max(1, Math.trunc(pagina) || 1), paginas)
  const inicio = (atual - 1) * porPagina
  const fatia = itens.slice(inicio, inicio + porPagina)
  return { itens: fatia, pagina: atual, paginas, de: total ? inicio + 1 : 0, ate: inicio + fatia.length, total }
}

/**
 * A seleção nova depois de UM gesto. Parte sempre da seleção inteira do bot e muda só o id tocado: o que está em
 * outra página, escondido pela busca ou fora da lista deste computador continua como estava (condição da frente
 * Navegador, 10/11/2026: `onChange` nunca reconstrói a seleção com os itens visíveis).
 */
export function alternar(selecao: readonly string[], id: string, ligado: boolean): string[] {
  const resto = selecao.filter((x) => x !== id)
  return ligado ? [...resto, id] : resto
}

export const ABAS = ['connectors', 'logins', 'skills'] as const
export type Aba = typeof ABAS[number]

/**
 * MX-95: o que a tela lembra para navegar. Só aba, busca e página. Não há seleção nem permissão aqui: trocar de aba,
 * buscar ou paginar não tem como mudar o que está ligado, porque esse estado nem chega a este reducer.
 */
export interface Navegacao { aba: Aba; busca: Record<Aba, string>; pagina: Record<Aba, number> }
export type AcaoDeNavegacao = { tipo: 'aba'; aba: Aba } | { tipo: 'busca'; texto: string } | { tipo: 'pagina'; pagina: number }

export const navegacaoInicial = (aba: Aba = 'connectors'): Navegacao => ({
  aba, busca: { connectors: '', logins: '', skills: '' }, pagina: { connectors: 1, logins: 1, skills: 1 },
})

/** Trocar de aba e buscar voltam à página 1. A busca de cada aba fica guardada nela. */
export function navegar(estado: Navegacao, acao: AcaoDeNavegacao): Navegacao {
  if (acao.tipo === 'aba') return { ...estado, aba: acao.aba, pagina: { ...estado.pagina, [acao.aba]: 1 } }
  if (acao.tipo === 'busca') return { ...estado, busca: { ...estado.busca, [estado.aba]: acao.texto }, pagina: { ...estado.pagina, [estado.aba]: 1 } }
  return { ...estado, pagina: { ...estado.pagina, [estado.aba]: acao.pagina } }
}

/** Os números do rodapé ('‹ 1 2 3 ›'). Com muitas páginas: primeira, última e vizinhas da atual; `null` vira "…". */
export function numerosDaPaginacao(pagina: number, paginas: number): (number | null)[] {
  if (paginas <= 7) return Array.from({ length: paginas }, (_, i) => i + 1)
  const meio = [pagina - 1, pagina, pagina + 1].filter((n) => n > 1 && n < paginas)
  const lista: (number | null)[] = [1]
  if (meio[0]! > 2) lista.push(null)
  lista.push(...meio)
  if (meio[meio.length - 1]! < paginas - 1) lista.push(null)
  lista.push(paginas)
  return lista
}
