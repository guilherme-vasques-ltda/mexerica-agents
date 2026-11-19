// MX-122 v3 (dono, 11/11): a faixa do topo da janela. Logo do Mexerica no centro, seletor de Space à esquerda,
// o resto arrasta a janela. Valores aprovados na proposta (docs/reference/design/header-logo).

/** Altura da faixa, em px. Também a altura dos controles do sistema no Windows/Linux (titleBarOverlay). */
export const FAIXA_ALTURA = 29

/** Botões da janela do Mac centrados na faixa: 12 px da borda, e o centro (botões de 12 px) a 14,5 px do topo. */
export const SEMAFOROS = { x: 12, y: 8 } as const

/** Logo plana com nome: 89 × 23 px (exceção da marca para a barra do app; brand/assets/logo/LEIA-ME.txt). */
export const LOGO_NOME_LARGURA = 89
/** Folga mínima entre o fim do que fica à esquerda (seletor de Space) e o começo da logo com nome. */
export const LOGO_FOLGA = 16

/**
 * Nome ou só a fruta: o nome quando há `LOGO_FOLGA` px livres entre o fim do conteúdo à esquerda e o começo
 * da logo, que fica no centro da faixa; senão, só a fruta.
 */
export function logoComNome(larguraFaixa: number, fimEsquerda: number): boolean {
  return larguraFaixa / 2 - LOGO_NOME_LARGURA / 2 - LOGO_FOLGA >= fimEsquerda
}

/** Cores dos controles do sistema (Windows/Linux) que o renderer pede ao main: só #rrggbb. */
export interface CoresControles {
  color: string
  symbolColor: string
}

const HEX = /^#[0-9a-f]{6}$/i

/** Aceita só `{ color, symbolColor }` em #rrggbb; o resto é recusado (vem do renderer). */
export function coresControles(valor: unknown): CoresControles | null {
  if (!valor || typeof valor !== 'object') return null
  const { color, symbolColor } = valor as Record<string, unknown>
  if (typeof color !== 'string' || typeof symbolColor !== 'string' || !HEX.test(color) || !HEX.test(symbolColor)) return null
  return { color: color.toLowerCase(), symbolColor: symbolColor.toLowerCase() }
}

/**
 * Uma cor CSS calculada (`rgb(...)`/`rgba(...)`) em #rrggbb, composta sobre `fundo` quando é translúcida
 * (o vidro da faixa com fundo de tela). `null` se não for rgb.
 */
export function hexOpaco(cor: string, fundo: string = 'rgb(255, 255, 255)'): string | null {
  const c = rgba(cor)
  if (!c) return null
  const f = rgba(fundo) ?? [255, 255, 255, 1]
  const mix = (i: number) => Math.round(c[i]! * c[3]! + f[i]! * (1 - c[3]!))
  return `#${[0, 1, 2].map((i) => mix(i).toString(16).padStart(2, '0')).join('')}`
}

function rgba(cor: string): [number, number, number, number] | null {
  const m = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,\s/]+([\d.]+%?))?\s*\)$/.exec(cor.trim())
  if (!m) return null
  const alfa = m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : parseFloat(m[4])
  const canal = (v: string) => Math.max(0, Math.min(255, parseFloat(v)))
  return [canal(m[1]!), canal(m[2]!), canal(m[3]!), Math.max(0, Math.min(1, alfa))]
}
