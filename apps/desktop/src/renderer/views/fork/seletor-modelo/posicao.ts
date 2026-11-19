// D473: onde a lista de modelos (e o menu da mãozinha) abre, presa ao chip e sempre dentro da janela. Função pura
// (testada em posicao.test.ts):
// a largura cabe na janela; a borda esquerda segue a do chip e encosta na margem quando faltaria espaço; abre do lado
// (acima ou abaixo) com mais espaço, preferindo acima quando a altura inteira cabe; a altura máxima é o espaço desse
// lado, e o seletor rola por dentro.
export const LARGURA_SELETOR = 580
/** Altura fixa do seletor: 8 linhas de modelo à vista (seletor-modelo.css). */
export const ALTURA_SELETOR = 418
/** Distância mínima até a borda da janela. */
export const MARGEM = 8
/** Distância entre o chip e a lista. */
export const FOLGA = 6

export type Ancora = { top: number; bottom: number; left: number }
export type Posicao = { left: number; width: number; maxHeight: number } & ({ bottom: number } | { top: number })

export type Tamanho = { largura: number; altura: number }
/** O menu da mãozinha (ApprovalModeMenu): 340px de largura; a altura é a do conteúdo, até a sobra da janela. */
export const TAMANHO_MENU_APROVACAO: Tamanho = { largura: 340, altura: 480 }

export function posicaoLista(chip: Ancora, janela: Tamanho, painel: Tamanho = { largura: LARGURA_SELETOR, altura: ALTURA_SELETOR }): Posicao {
  const width = Math.max(0, Math.min(painel.largura, janela.largura - 2 * MARGEM))
  const left = Math.max(MARGEM, Math.min(chip.left, janela.largura - width - MARGEM))
  const acimaLivre = Math.max(0, chip.top - FOLGA - MARGEM)
  const abaixoLivre = Math.max(0, janela.altura - chip.bottom - FOLGA - MARGEM)
  const acima = acimaLivre >= painel.altura || acimaLivre >= abaixoLivre
  const maxHeight = Math.min(painel.altura, acima ? acimaLivre : abaixoLivre)
  return acima
    ? { left, width, maxHeight, bottom: janela.altura - chip.top + FOLGA }
    : { left, width, maxHeight, top: chip.bottom + FOLGA }
}

/**
 * A âncora de um botão da linha de baixo do campo (D473): o topo é o da caixa do campo inteira (a irmã anterior da
 * linha, como o balão do contexto, D378), para o painel não cobrir o texto digitado; fora dela, o próprio botão.
 */
export function ancoraNoCampo(botao: HTMLElement): Ancora {
  const rect = botao.getBoundingClientRect()
  const caixa = botao.closest('.ctx-barra')?.previousElementSibling?.querySelector('.composer-surface')
  return { top: Math.min(rect.top, caixa?.getBoundingClientRect().top ?? rect.top), bottom: rect.bottom, left: rect.left }
}

/** O estilo do menu da mãozinha preso ao chip dela, com rolagem interna se a janela for baixa. */
export function estiloMenuAprovacao(botao: HTMLElement) {
  const { left, width, maxHeight, ...lado } = posicaoLista(ancoraNoCampo(botao), { largura: window.innerWidth, altura: window.innerHeight }, TAMANHO_MENU_APROVACAO)
  return { ...lado, left, width, maxHeight, overflowY: 'auto' as const, transformOrigin: 'top' in lado ? 'top left' : 'bottom left' }
}
