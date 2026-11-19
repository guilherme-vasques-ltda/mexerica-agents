// Entrega 3: a janela da lista virtualizada do seletor. Só as linhas à vista (e uma folga) vão para a tela; todas têm
// a mesma altura (seletor-modelo.css). Funções puras, testadas em virtual.test.ts.

/** Altura de toda linha da lista: modelo, "Padrão do motor" e nome de motor na busca (`.seletor-linha`). */
export const ALTURA_LINHA = 32
/** Linhas à vista: a lista nunca passa de 8 (`.seletor-lista`). */
export const LINHAS_A_VISTA = 8
/** Linhas a mais, acima e abaixo, para a rolagem não mostrar vazio. */
export const FOLGA_LINHAS = 4

/** As linhas a desenhar (`inicio` incluso, `fim` excluso) com a rolagem em `topo` px a partir do começo do bloco. */
export function janela(topo: number, total: number): { inicio: number; fim: number } {
  const primeira = Math.max(0, Math.min(Math.floor(topo / ALTURA_LINHA), total - LINHAS_A_VISTA))
  return { inicio: Math.max(0, primeira - FOLGA_LINHAS), fim: Math.min(total, primeira + LINHAS_A_VISTA + 1 + FOLGA_LINHAS) }
}

/**
 * A rolagem que deixa a linha `indice` inteira à vista, mexendo o mínimo: `rolagem` é a atual, `altura` a da área
 * visível e `antes` o que vem antes do bloco dentro da lista (px).
 */
export function rolagemPara(indice: number, rolagem: number, altura: number, antes = 0): number {
  const cima = antes + indice * ALTURA_LINHA
  if (cima < rolagem) return cima
  const baixo = cima + ALTURA_LINHA
  return baixo > rolagem + altura ? baixo - altura : rolagem
}

/**
 * A próxima linha que o teclado alcança a partir de `de` (`-1` = da busca), dando a volta; `-1` se nenhuma serve.
 * `serve` diz se a linha recebe foco (nome de motor e linha desativada não recebem).
 */
export function proximaLinha(de: number, passo: 1 | -1, total: number, serve: (indice: number) => boolean): number {
  if (de < 0) { de = -1; passo = 1 }
  for (let n = 1; n <= total; n++) {
    const i = (((de + passo * n) % total) + total) % total
    if (serve(i)) return i
  }
  return -1
}
