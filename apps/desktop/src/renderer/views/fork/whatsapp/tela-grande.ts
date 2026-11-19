// whatsapp R5c: Expandir e Recolher animam só a moldura (fundo, borda e sombra) de um retângulo ao outro, com
// `transform`; o texto nunca estica. Puro, para testar sem DOM.

import { PANEL_EASE } from '../painel-direito/curva.ts'

export interface Retangulo { left: number; top: number; width: number; height: number }

/** O transform que põe a moldura (desenhada no destino, origem no canto de cima à esquerda) sobre a origem. */
export function transformeMoldura(de: Retangulo, para: Retangulo): string {
  const sx = para.width ? de.width / para.width : 1
  const sy = para.height ? de.height / para.height : 1
  const r = (n: number) => Math.round(n * 1000) / 1000
  return `translate(${r(de.left - para.left)}px, ${r(de.top - para.top)}px) scale(${r(sx)}, ${r(sy)})`
}

/** Um token de movimento do tema ("0.3s linear(…)", "0.12s cubic-bezier(…)") em ms e curva. Movimento reduzido = 0. */
export function tempoDoToken(valor: string): { ms: number; curva: string } {
  const v = valor.trim()
  const m = /^([\d.]+)(m?s)\s*(.*)$/s.exec(v)
  if (!m) return { ms: 0, curva: 'linear' }
  const n = parseFloat(m[1]!)
  return { ms: m[2] === 'ms' ? n : n * 1000, curva: m[3]!.trim() || 'linear' }
}

/** O retângulo com uma margem por dentro (o centro tem 8 px até a borda, como o cartão de 44 px). */
export const recuo = (r: Retangulo, px: number): Retangulo => ({ left: r.left + px, top: r.top + px, width: Math.max(0, r.width - 2 * px), height: Math.max(0, r.height - 2 * px) })

export type TeclaTela = 'recolher' | 'conversa' | null

/** Na tela grande: Esc = Recolher; ⌘1 (Ctrl+1 fora do Mac) volta à conversa do bot. */
export function teclaTelaGrande(e: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }, mac: boolean): TeclaTela {
  if (e.key === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey) return 'recolher'
  if (e.key === '1' && (mac ? e.metaKey : e.ctrlKey) && !e.altKey && !e.shiftKey) return 'conversa'
  return null
}

/**
 * A4: o voo dura o `--layout` do tema (0,3 s; 0 com movimento reduzido) e usa a curva do fixar do painel (#352,
 * `PANEL_EASE`), não a mola do token.
 */
export function tempoDoVoo(layout: string): { ms: number; curva: string } {
  return { ms: tempoDoToken(layout).ms, curva: PANEL_EASE }
}

/** A1: invertendo no meio (Esc, clicar de novo), o voo parte de onde a moldura está agora, não do destino cheio. */
export const origemDoVoo = (emVoo: Retangulo | null, padrao: Retangulo | null): Retangulo | null => emVoo ?? padrao

/** A2: para onde vai o foco depois de cada troca. */
export type AlvoFoco = 'tela' | 'painel' | 'conversa'
export const SELETOR_FOCO: Record<AlvoFoco, readonly string[]> = {
  // Na tela grande: o Recolher.
  tela: ['.wa-tela .wa-x-recolher'],
  // De volta ao painel: o Expandir; sem ele (Ver desligado, aba sumiu), a aba selecionada.
  painel: ['.rpanel .wa-expand', '.rpanel [role="tab"][aria-selected="true"]'],
  // ⌘1: a aba da conversa principal do bot.
  conversa: ['.ctab.main'],
}

/** O foco só vai quando a troca terminou: nada voando e o painel fora do véu (elemento escondido não recebe foco). */
export const podeFocar = (fase: 'parada' | 'abrindo' | 'fechando', veu: boolean) => fase === 'parada' && !veu

/** A2: durante o voo, o foco fica na moldura; Tab e Shift+Tab não saem da área (a moldura segura a tecla). */
export const seguraTecla = (key: string) => key === 'Tab'
