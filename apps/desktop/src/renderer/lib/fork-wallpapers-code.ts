import { ptBR } from './fork-i18n/pt-BR.ts'

// DON-909: fundos desenhados em código (docs/reference/redesenho/papeis-codigo.md). Cada fundo é um SVG
// montado aqui e entregue como data URL, então entra pelo mesmo caminho dos JPGs (lib/appearance.ts):
// `--wp-image`, miniatura, prévia, Desfoque, Suavizar e a medição do piso por canvas. Estático, sem
// dependências, sem movimento. As receitas e a composição Petróleo da Mexerica (#0C706E/#2DD4BF, docs/guides/marca.md) são deste fork.
//
// `boundsOf` dá a caixa de cor de cada variante pela própria receita (toda camada é composta sobre a caixa
// das anteriores; bordas e transparências entram como alfa 0), um limite que vale para qualquer raster.
// O teste confere que o piso cadastrado cobre essa caixa; a medição real por canvas só pode apertá-lo.

import type { Bounds, RGB } from './legibility'

export type CodeWallpaperId = 'aurora' | 'carbono' | 'geada' | 'nascer-do-sol' | 'donna-petroleo'

/** Tamanho intrínseco do SVG (a janela típica): `cover` quase não escala, e a retícula fica nítida. */
export const W = 1440
export const H = 900

type Stop = [offset: number, color: string, alpha?: number]
type Paint =
  | { kind: 'linear'; angle: number; stops: Stop[] }
  /** CSS `radial-gradient(circle at cx cy, …)`, raio até o canto mais distante. */
  | { kind: 'radial'; cx: number; cy: number; stops: Stop[] }
/**
 * Mancha desfocada do original: a caixa Tailwind (left/top/w/h em % da área) vira uma elipse com queda
 * suave. `alpha` é o `opacity-*` do original.
 */
interface Bloom { left: number; top: number; w: number; h: number; color: string; alpha: number }
interface Texture { kind: 'dots' | 'grid'; color: string; alpha: number; size: number }
interface Variant {
  base: Paint
  blooms?: Bloom[]
  /** Camadas translúcidas por cima das manchas (holofote, vinheta), na ordem do original. */
  overlays?: Paint[]
  textures?: Texture[]
}
interface Spec { id: CodeWallpaperId; labelKey: `aparencia.fundo.${CodeWallpaperId}`; light: Variant; dark: Variant }

/** Converte `right`/`bottom` do original em `left`/`top`. */
const box = (b: { left?: number; right?: number; top?: number; bottom?: number; w: number; h: number }, color: string, alpha: number): Bloom => ({
  left: b.left ?? 100 - b.right! - b.w, top: b.top ?? 100 - b.bottom! - b.h, w: b.w, h: b.h, color, alpha,
})
const vignette = (from: number, alpha: number, color = '#000000'): Paint => ({ kind: 'radial', cx: 50, cy: 50, stops: [[from, color, 0], [100, color, alpha]] })
const dots = (color: string, alpha: number, size: number): Texture => ({ kind: 'dots', color, alpha, size })
const grid = (color: string, alpha: number, size: number): Texture => ({ kind: 'grid', color, alpha, size })

export const CODE_WALLPAPER_SPECS: Spec[] = [
  {
    // Dark Aurora: carvão com manchas teal, ciano, esmeralda e céu; retícula de 4 px.
    id: 'aurora', labelKey: 'aparencia.fundo.aurora',
    dark: {
      base: { kind: 'linear', angle: 180, stops: [[0, '#12151A'], [55, '#0A0C0F'], [100, '#06080A']] },
      blooms: [
        box({ left: -12, top: -18, w: 68, h: 68 }, '#2DD4BF', 0.22),
        box({ right: -10, top: 8, w: 62, h: 62 }, '#22D3EE', 0.18),
        box({ left: 18, bottom: -28, w: 70, h: 70 }, '#34D399', 0.16),
        box({ right: 8, bottom: -16, w: 56, h: 56 }, '#38BDF8', 0.14),
      ],
      textures: [dots('#FFFFFF', 0.04, 4)],
    },
    light: {
      base: { kind: 'linear', angle: 180, stops: [[0, '#F6FAFA'], [55, '#EEF5F5'], [100, '#E7F0F1']] },
      blooms: [
        box({ left: -12, top: -18, w: 68, h: 68 }, '#2DD4BF', 0.26),
        box({ right: -10, top: 8, w: 62, h: 62 }, '#22D3EE', 0.2),
        box({ left: 18, bottom: -28, w: 70, h: 70 }, '#34D399', 0.2),
        box({ right: 8, bottom: -16, w: 56, h: 56 }, '#38BDF8', 0.16),
      ],
      textures: [dots('#0F172A', 0.03, 4)],
    },
  },
  {
    // Dark Carbon Spotlight: holofote cinza no topo, vinheta e retícula.
    id: 'carbono', labelKey: 'aparencia.fundo.carbono',
    dark: {
      base: { kind: 'radial', cx: 50, cy: 12, stops: [[0, '#1C1C1C'], [36, '#121212'], [68, '#0C0C0C'], [100, '#050505']] },
      overlays: [
        { kind: 'radial', cx: 50, cy: 0, stops: [[0, '#FFFFFF', 0.07], [46, '#FFFFFF', 0]] },
        vignette(42, 0.55),
      ],
      textures: [dots('#FFFFFF', 0.021, 4)],
    },
    light: {
      base: { kind: 'radial', cx: 50, cy: 12, stops: [[0, '#FFFFFF'], [36, '#F4F4F3'], [68, '#ECECEA'], [100, '#E2E2E0']] },
      overlays: [
        { kind: 'radial', cx: 50, cy: 0, stops: [[0, '#FFFFFF', 0.6], [46, '#FFFFFF', 0]] },
        vignette(42, 0.08),
      ],
      textures: [dots('#000000', 0.025, 4)],
    },
  },
  {
    // Frost Mesh: branco frio, grades, ciano, esmeralda e teal nos cantos.
    id: 'geada', labelKey: 'aparencia.fundo.geada',
    light: {
      base: { kind: 'linear', angle: 135, stops: [[0, '#FAFCFD'], [50, '#F2F8FA'], [100, '#EAF4F6']] },
      textures: [grid('#94A3B8', 0.11, 20), grid('#94A3B8', 0.048, 4)],
      blooms: [
        box({ left: -12, top: -18, w: 52, h: 52 }, '#A5F3FC', 0.35),
        box({ right: -14, top: 10, w: 48, h: 48 }, '#6EE7B7', 0.28),
        box({ left: 24, bottom: -20, w: 46, h: 46 }, '#99F6E4', 0.22),
      ],
    },
    dark: {
      base: { kind: 'linear', angle: 135, stops: [[0, '#0E1518'], [50, '#0B1114'], [100, '#080D10']] },
      textures: [grid('#94A3B8', 0.04, 20), grid('#94A3B8', 0.015, 4)],
      blooms: [
        box({ left: -12, top: -18, w: 52, h: 52 }, '#A5F3FC', 0.12),
        box({ right: -14, top: 10, w: 48, h: 48 }, '#6EE7B7', 0.1),
        box({ left: 24, bottom: -20, w: 46, h: 46 }, '#99F6E4', 0.08),
      ],
    },
  },
  {
    // Sunrise Horizon: céu creme, faixas pêssego, sol baixo no centro, teal no rodapé. No original as três
    // manchas não aparecem (o contêiner delas não tem tamanho); segue-se o que se vê.
    id: 'nascer-do-sol', labelKey: 'aparencia.fundo.nascer-do-sol',
    light: {
      base: { kind: 'linear', angle: 180, stops: [[0, '#FFF8F3'], [42, '#FFE8D8'], [68, '#FFD9C8'], [100, '#C8E8EE']] },
      blooms: [box({ left: 45, bottom: 15, w: 10, h: 16 }, '#FFF1DC', 0.9)],
      textures: [dots('#000000', 0.04, 5)],
    },
    dark: {
      base: { kind: 'linear', angle: 180, stops: [[0, '#0D0F1A'], [42, '#1E1520'], [68, '#2C1B20'], [100, '#0E2629']] },
      blooms: [box({ left: 45, bottom: 15, w: 10, h: 16 }, '#FFB38A', 0.4)],
      textures: [dots('#FFFFFF', 0.03, 5)],
    },
  },
  {
    // Petróleo: o verde da marca no claro (#0C706E) como fundo profundo, com a piscadinha discreta.
    id: 'donna-petroleo', labelKey: 'aparencia.fundo.donna-petroleo',
    dark: {
      base: { kind: 'linear', angle: 160, stops: [[0, '#0A2827'], [55, '#071C1C'], [100, '#041111']] },
      blooms: [
        box({ left: -14, top: -20, w: 72, h: 72 }, '#0C706E', 0.55),
        box({ right: -10, bottom: -24, w: 60, h: 60 }, '#0C706E', 0.4),
        box({ right: 18, top: 10, w: 26, h: 26 }, '#2DD4BF', 0.1),
      ],
      overlays: [vignette(50, 0.35)],
      textures: [dots('#FFFFFF', 0.025, 4)],
    },
    light: {
      base: { kind: 'linear', angle: 160, stops: [[0, '#F3F8F7'], [55, '#EBF3F2'], [100, '#E3EEED']] },
      blooms: [
        box({ left: -14, top: -20, w: 72, h: 72 }, '#0C706E', 0.14),
        box({ right: -10, bottom: -24, w: 60, h: 60 }, '#0C706E', 0.12),
        box({ right: 18, top: 10, w: 26, h: 26 }, '#2DD4BF', 0.16),
      ],
      textures: [dots('#0C706E', 0.03, 4)],
    },
  },

]

// --- SVG ---

const r1 = (n: number) => Math.round(n * 100) / 100
const stopsSVG = (stops: Stop[]) => stops.map(([o, c, a = 1]) => `<stop offset="${o / 100}" stop-color="${c}"${a < 1 ? ` stop-opacity="${a}"` : ''}/>`).join('')

function paintSVG(id: string, p: Paint): string {
  if (p.kind === 'linear') {
    // CSS: 0deg sobe, 90deg vai à direita; nas unidades da caixa, como o navegador estica a caixa.
    const rad = (p.angle * Math.PI) / 180
    const dx = Math.sin(rad) / 2
    const dy = -Math.cos(rad) / 2
    return `<linearGradient id="${id}" x1="${r1(0.5 - dx)}" y1="${r1(0.5 - dy)}" x2="${r1(0.5 + dx)}" y2="${r1(0.5 + dy)}">${stopsSVG(p.stops)}</linearGradient>`
  }
  const cx = (p.cx / 100) * W
  const cy = (p.cy / 100) * H
  const r = Math.hypot(Math.max(cx, W - cx), Math.max(cy, H - cy))
  return `<radialGradient id="${id}" gradientUnits="userSpaceOnUse" cx="${r1(cx)}" cy="${r1(cy)}" r="${r1(r)}">${stopsSVG(p.stops)}</radialGradient>`
}

/** A mancha desfocada: platô e queda longa, além da caixa original (o `blur-3xl` a espalha). */
const SPREAD = 1.3
const bloomStops = (color: string, a: number): Stop[] => [[0, color, a], [40, color, r1(a * 0.92)], [70, color, r1(a * 0.5)], [88, color, r1(a * 0.16)], [100, color, 0]]

function textureSVG(id: string, t: Texture): string {
  const s = t.size
  const mark = t.kind === 'dots'
    ? `<circle cx="${s / 2}" cy="${s / 2}" r="1" fill="${t.color}" fill-opacity="${t.alpha}"/>`
    : `<path d="M0 0H${s}M0 0V${s}" stroke="${t.color}" stroke-opacity="${t.alpha}" stroke-width="2" fill="none"/>`
  return `<pattern id="${id}" width="${s}" height="${s}" patternUnits="userSpaceOnUse">${mark}</pattern>`
}

export function variantSVG(v: Variant): string {
  const defs: string[] = []
  const body: string[] = []
  const rect = (fill: string) => body.push(`<rect width="${W}" height="${H}" fill="url(#${fill})"/>`)
  defs.push(paintSVG('b', v.base))
  rect('b')
  // Ordem do original: textura de grade antes das manchas na Geada (ela vem antes no código de lá);
  // aqui a ordem não muda o limite de cor, e a retícula fina fica por cima.
  const textures = v.textures ?? []
  const grids = textures.filter((t) => t.kind === 'grid')
  grids.forEach((t, i) => { defs.push(textureSVG(`g${i}`, t)); rect(`g${i}`) })
  ;(v.blooms ?? []).forEach((b, i) => {
    defs.push(`<radialGradient id="m${i}">${stopsSVG(bloomStops(b.color, b.alpha))}</radialGradient>`)
    const cx = ((b.left + b.w / 2) / 100) * W
    const cy = ((b.top + b.h / 2) / 100) * H
    body.push(`<ellipse cx="${r1(cx)}" cy="${r1(cy)}" rx="${r1((b.w / 200) * W * SPREAD)}" ry="${r1((b.h / 200) * H * SPREAD)}" fill="url(#m${i})"/>`)
  })
  ;(v.overlays ?? []).forEach((p, i) => { defs.push(paintSVG(`o${i}`, p)); rect(`o${i}`) })
  textures.filter((t) => t.kind === 'dots').forEach((t, i) => { defs.push(textureSVG(`d${i}`, t)); rect(`d${i}`) })
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><defs>${defs.join('')}</defs>${body.join('')}</svg>`
}

export const svgURL = (svg: string) => `data:image/svg+xml,${encodeURIComponent(svg)}`

// --- Caixa de cor ---

const hex = (c: string): RGB => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as unknown as RGB

/**
 * A caixa por canal de tudo o que a variante pode pintar. A base é opaca (a caixa das suas paradas); cada
 * camada seguinte, com cor entre os extremos das suas paradas e alfa entre 0 (borda, transparência,
 * antisserrilhado) e o máximo, compõe sobre a caixa anterior: o resultado é bilinear em cor e alfa, então os
 * cantos bastam, na interpolação pré-multiplicada ou não.
 */
export function boundsOf(v: Variant): Bounds {
  const lo = [255, 255, 255]
  const hi = [0, 0, 0]
  for (const [, c] of paintStops(v.base)) hex(c).forEach((x, k) => { lo[k] = Math.min(lo[k]!, x); hi[k] = Math.max(hi[k]!, x) })
  const layer = (colors: string[], alpha: number) => {
    for (let k = 0; k < 3; k++) {
      const cs = colors.map((c) => hex(c)[k]!)
      const ends = [Math.min(...cs), Math.max(...cs)]
      const [l, h] = [lo[k]!, hi[k]!]
      const corners = ends.flatMap((c) => [l, h].map((b) => c * alpha + b * (1 - alpha)))
      lo[k] = Math.min(l, ...corners)
      hi[k] = Math.max(h, ...corners)
    }
  }
  const textures = v.textures ?? []
  for (const t of textures.filter((t) => t.kind === 'grid')) layer([t.color], t.alpha)
  for (const b of v.blooms ?? []) layer([b.color], b.alpha)
  for (const p of v.overlays ?? []) {
    const stops = paintStops(p)
    layer(stops.map(([, c]) => c), Math.max(...stops.map(([, , a = 1]) => a)))
  }
  for (const t of textures.filter((t) => t.kind === 'dots')) layer([t.color], t.alpha)
  return { lo: [Math.floor(lo[0]!), Math.floor(lo[1]!), Math.floor(lo[2]!)], hi: [Math.ceil(hi[0]!), Math.ceil(hi[1]!), Math.ceil(hi[2]!)] }
}
const paintStops = (p: Paint) => p.stops

/**
 * Piso de reserva (sem véu) de cada variante: o que `floorFor` dá sobre `boundsOf` com os tokens atuais,
 * arredondado para cima. Usado só até o canvas ler o SVG, ou se não puder lê-lo. O teste recalcula.
 */
export const CODE_WALLPAPER_FLOORS: Record<CodeWallpaperId, { light: number; dark: number }> = {
  'aurora': { light: 0, dark: 0.39 },
  'carbono': { light: 0, dark: 0 },
  'geada': { light: 0, dark: 0.09 },
  'nascer-do-sol': { light: 0, dark: 0.38 },
  'donna-petroleo': { light: 0, dark: 0.25 },
}

export const CODE_WALLPAPERS = CODE_WALLPAPER_SPECS.map((s) => ({
  id: s.id,
  label: ptBR[s.labelKey],
  labelKey: s.labelKey,
  light: svgURL(variantSVG(s.light)),
  dark: svgURL(variantSVG(s.dark)),
  floor: CODE_WALLPAPER_FLOORS[s.id],
}))
