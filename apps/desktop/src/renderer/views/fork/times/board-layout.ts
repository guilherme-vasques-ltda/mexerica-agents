import type { TimesGraph } from '../../../../shared/fork-times.ts'
import { botsBelow, itemKey, type TreeItem } from './structure.ts'

// Disposição do Quadro, cálculo puro: a árvore de estrutura vira posições (líder acima, filhos lado a lado abaixo,
// áreas como contornos), os cotovelos de “Fica abaixo de”, as curvas de Reporta a e as rotas propostas. Arrastar um
// cartão só soma um deslocamento local (board-prefs.ts); nada aqui muda o grafo.

export const CARD = { w: 176, h: 56 }
export const OWNER = { w: 112, h: 40 }
const GAP_X = 24
const GAP_Y = 48
const PAD = 16
const HEAD = 22
const COLLAPSED = { w: 176, h: 40 }

export interface Point { x: number; y: number }
export interface CardBox { key: string; kind: 'owner' | 'bot'; botId: string | null; x: number; y: number; w: number; h: number; hiddenBelow: number }
export interface FrameBox { key: string; kind: 'area' | 'loose'; nodeId: string | null; x: number; y: number; w: number; h: number; collapsed: boolean; hiddenBelow: number }
export interface Line { key: string; d: string; from: string; to: string }
export interface BoardLayout {
  width: number
  height: number
  cards: CardBox[]
  frames: FrameBox[]
  structure: Line[]
  reports: Line[]
  routes: (Line & { label: Point })[]
}

interface Measured { item: TreeItem; w: number; h: number; kids: Measured[]; collapsed: boolean }

function measure(item: TreeItem, collapsed: ReadonlySet<string>): Measured {
  const key = itemKey(item)
  const folded = collapsed.has(key) && item.children.length > 0 && item.kind !== 'owner'
  const kids = folded ? [] : item.children.map((c) => measure(c, collapsed))
  const rowW = kids.reduce((sum, k) => sum + k.w, 0) + GAP_X * Math.max(0, kids.length - 1)
  const rowH = kids.reduce((max, k) => Math.max(max, k.h), 0)
  if (item.kind === 'area' || item.kind === 'loose') {
    if (folded || !kids.length) return { item, w: Math.max(COLLAPSED.w, rowW) + 2 * PAD, h: HEAD + PAD + (kids.length ? rowH : COLLAPSED.h) + PAD, kids, collapsed: folded }
    return { item, w: Math.max(rowW, 140) + 2 * PAD, h: HEAD + PAD + rowH + PAD, kids, collapsed: false }
  }
  const card = item.kind === 'owner' ? OWNER : CARD
  return { item, w: Math.max(card.w, rowW), h: card.h + (kids.length ? GAP_Y + rowH : 0), kids, collapsed: folded }
}

/** Posições de tudo a partir da árvore, com `offsets` (disposição local) somados aos cartões. */
export function boardLayout(root: TreeItem, graph: TimesGraph, options: { collapsed: ReadonlySet<string>; offsets: Readonly<Record<string, Point>>; showRoutes: boolean }): BoardLayout {
  const cards: CardBox[] = []
  const frames: FrameBox[] = []
  const structure: Line[] = []
  /** Onde uma linha de estrutura chega a cada item: topo do cartão ou do contorno. */
  const tops = new Map<string, Point>()
  const bottoms = new Map<string, Point>()

  const offset = (key: string) => options.offsets[key] ?? { x: 0, y: 0 }
  const placeRow = (kids: Measured[], x: number, y: number, width: number) => {
    const rowW = kids.reduce((sum, k) => sum + k.w, 0) + GAP_X * Math.max(0, kids.length - 1)
    let at = x + (width - rowW) / 2
    for (const k of kids) {
      place(k, at, y)
      at += k.w + GAP_X
    }
  }
  const place = (m: Measured, x: number, y: number) => {
    const key = itemKey(m.item)
    if (m.item.kind === 'area' || m.item.kind === 'loose') {
      frames.push({ key, kind: m.item.kind, nodeId: m.item.kind === 'area' ? m.item.nodeId : null, x, y, w: m.w, h: m.h, collapsed: m.collapsed, hiddenBelow: m.collapsed ? botsBelow(m.item) : 0 })
      tops.set(key, { x: x + m.w / 2, y })
      placeRow(m.kids, x + PAD, y + HEAD + PAD, m.w - 2 * PAD)
      return
    }
    const card = m.item.kind === 'owner' ? OWNER : CARD
    const shift = offset(key)
    const cx = x + (m.w - card.w) / 2 + shift.x
    const cy = y + shift.y
    cards.push({ key, kind: m.item.kind, botId: m.item.kind === 'bot' ? m.item.botId : null, x: cx, y: cy, w: card.w, h: card.h, hiddenBelow: m.collapsed ? botsBelow(m.item) : 0 })
    tops.set(key, { x: cx + card.w / 2, y: cy })
    bottoms.set(key, { x: cx + card.w / 2, y: cy + card.h })
    placeRow(m.kids, x, y + card.h + GAP_Y, m.w)
    for (const k of m.kids) structure.push({ key: `${key}>${itemKey(k.item)}`, from: itemKey(k.item), to: key, d: '' })
  }
  const measured = measure(root, options.collapsed)
  place(measured, 0, 0)

  // Cotovelos: do pé do cartão de cima até o topo de cada filho (cartão ou contorno), depois de todos os deslocamentos.
  for (const line of structure) {
    const a = bottoms.get(line.to)
    const b = tops.get(line.from)
    if (!a || !b) continue
    const mid = a.y + (b.y - a.y) / 2
    line.d = `M${r(a.x)} ${r(a.y)}V${r(mid)}H${r(b.x)}V${r(b.y)}`
  }

  const cardOf = new Map(cards.map((c) => [c.key, c]))
  const reports: Line[] = []
  for (const m of graph.members) {
    const from = cardOf.get(m.botId)
    const to = cardOf.get(m.reportsTo.kind === 'owner' ? 'owner' : m.reportsTo.botId)
    if (!from || !to) continue
    reports.push({ key: `r:${m.botId}`, from: m.botId, to: to.key, d: curve(port(from), anchor(to, port(from))) })
  }
  const routes: (Line & { label: Point })[] = []
  if (options.showRoutes) {
    for (const route of graph.routes) {
      const from = cardOf.get(route.fromBotId)
      const to = cardOf.get(route.toBotId)
      if (!from || !to) continue
      const a = { x: from.x + from.w, y: from.y + from.h / 2 }
      const b = { x: to.x, y: to.y + to.h / 2 }
      const flip = b.x < a.x
      const start = flip ? { x: from.x, y: a.y } : a
      const end = flip ? { x: to.x + to.w, y: b.y } : b
      const bend = flip ? -48 : 48
      routes.push({
        key: `p:${route.fromBotId}>${route.toBotId}`, from: route.fromBotId, to: route.toBotId,
        d: `M${r(start.x)} ${r(start.y)}C${r(start.x + bend)} ${r(start.y + 28)} ${r(end.x - bend)} ${r(end.y + 28)} ${r(end.x)} ${r(end.y)}`,
        label: { x: (start.x + end.x) / 2, y: (start.y + end.y) / 2 + 26 },
      })
    }
  }
  const right = Math.max(measured.w, ...cards.map((c) => c.x + c.w), ...frames.map((f) => f.x + f.w))
  const bottom = Math.max(measured.h, ...cards.map((c) => c.y + c.h), ...frames.map((f) => f.y + f.h))
  return { width: right, height: bottom, cards, frames, structure: structure.filter((l) => l.d), reports, routes }
}

const r = (n: number) => Math.round(n * 10) / 10

/** A porta: o ponto no topo do cartão, de onde sai Reporta a. */
export const port = (c: { x: number; y: number; w: number }): Point => ({ x: c.x + c.w / 2, y: c.y })

/** Onde a seta chega: no pé do destinatário se ele está acima, senão no topo. */
function anchor(c: CardBox, from: Point): Point {
  return c.y + c.h <= from.y ? { x: c.x + c.w / 2, y: c.y + c.h } : { x: c.x + c.w / 2, y: c.y }
}

function curve(a: Point, b: Point) {
  const pull = Math.max(28, Math.abs(a.y - b.y) / 2)
  const up = b.y <= a.y
  return `M${r(a.x)} ${r(a.y)}C${r(a.x)} ${r(a.y - pull)} ${r(b.x)} ${r(up ? b.y + pull : b.y - pull)} ${r(b.x)} ${r(b.y)}`
}

/** Zoom e deslocamento para caber tudo em `view`, com margem, entre 30% e 150%. */
export function fitView(layout: { width: number; height: number }, view: { w: number; h: number }, margin = 32) {
  const zoom = clampZoom(Math.min((view.w - 2 * margin) / Math.max(1, layout.width), (view.h - 2 * margin) / Math.max(1, layout.height)))
  return { zoom, pan: { x: (view.w - layout.width * zoom) / 2, y: Math.max(margin, (view.h - layout.height * zoom) / 2) } }
}

export const ZOOM = { min: 0.3, max: 1.5, step: 0.1 }
export const clampZoom = (z: number) => Math.min(ZOOM.max, Math.max(ZOOM.min, Math.round(z * 100) / 100))

/** Deslocamento para que `box` fique inteiro à vista em `view`; null se já está. */
export function revealPan(box: { x: number; y: number; w: number; h: number }, zoom: number, pan: Point, view: { w: number; h: number }, margin = 24): Point | null {
  const left = box.x * zoom + pan.x
  const top = box.y * zoom + pan.y
  const right = left + box.w * zoom
  const bottom = top + box.h * zoom
  let dx = 0
  let dy = 0
  if (left < margin) dx = margin - left
  else if (right > view.w - margin) dx = view.w - margin - right
  if (top < margin) dy = margin - top
  else if (bottom > view.h - margin) dy = view.h - margin - bottom
  return dx || dy ? { x: pan.x + dx, y: pan.y + dy } : null
}
