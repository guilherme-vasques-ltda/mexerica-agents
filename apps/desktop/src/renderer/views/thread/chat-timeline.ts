// D165 fase E: the timeline's layout, pure so it is testable. One tick per message; the gap shrinks
// from 6px (T3 pitch, 07/11) to 3px as the conversation grows, and past that neighbours share a tick.
// D432 (07/11): marcas maiores, realce das que estão na tela e escala no hover, no comportamento do T3 Code
// (docs/reference/design/barrinha-novas-20261107); nada do código dele.

export interface TimelineMessage {
  id: string
  /** "09:12" */
  time: string
  text: string
  mine: boolean
  unread: boolean
  /** Para mensagem sua: a última resposta do bot antes do seu próximo pedido (prévia). */
  answer?: string | null
}

export type TimelineVariant = 'a' | 'b'
/**
 * D436 (dono): variante B.
 * A: uma marca por mensagem sua (a resposta do bot vai na prévia). B: uma por mensagem, a sua mais longa.
 */
export const TIMELINE_VARIANT: TimelineVariant = 'b'

/**
 * As marcas da variante e, para o realce da tela, a marca de cada mensagem (na A, a resposta do bot fica com o seu
 * pedido). Na A, a marca é não lida quando alguma resposta dela é.
 */
export function timelineMarks(messages: readonly TimelineMessage[], variant: TimelineVariant = TIMELINE_VARIANT) {
  const all = messages.map((m) => ({ ...m, answer: null as string | null }))
  const owner = new Map<string, string>()
  let ask: (typeof all)[number] | null = null
  for (const m of all) {
    if (m.mine) ask = m
    else if (ask) ask.answer = m.text
    const mark = variant === 'b' || m.mine ? m : ask
    if (mark) {
      owner.set(m.id, mark.id)
      if (mark !== m && m.unread) mark.unread = true
    }
  }
  return { marks: variant === 'b' ? all : all.filter((m) => m.mine), owner }
}

/**
 * Comprimento no hover, em degradê como no T3 (pedido do dono, 07/11; medido no T3: 3× / 2× / 1,25×): a marca 3×, as
 * duas vizinhas de cada lado 2× e 1,25×; as demais, 1. A marca da posição atual fica com `VIEW_SCALE`, e o hover só a aumenta.
 */
export const hoverScale = (distance: number) => [3, 2, 1.25][distance] ?? 1
export const VIEW_SCALE = 1.5

export interface TimelineTick {
  ids: string[]
  messages: TimelineMessage[]
  unread: boolean
}

/** Preserve the message identity across grouping; a removed anchor falls back to the view, then newest. */
export function timelineFocusIndex(items: readonly TimelineTick[], anchor: string | null, current: string | null): number {
  const anchored = items.findIndex((tick) => tick.ids.includes(anchor ?? ''))
  if (anchored >= 0) return anchored
  const visible = items.findIndex((tick) => tick.ids.includes(current ?? ''))
  return visible >= 0 ? visible : items.length - 1
}

const TICK = 2
// Passo do T3 (medido, 07/11): 8 px de centro a centro, traço de 2 px e 6 px de espaço.
const MAX_GAP = 6
const MIN_GAP = 3

/** Ticks and their gap for `messages` in a track `height` px tall. */
export function timelineTicks(messages: readonly TimelineMessage[], height: number): { gap: number; items: TimelineTick[] } {
  if (!messages.length) return { gap: MAX_GAP, items: [] }
  const room = Math.max(0, height)
  const fits = (gap: number) => Math.max(1, Math.floor((room + gap) / (TICK + gap)))
  const gap = Math.max(MIN_GAP, Math.min(MAX_GAP, Math.floor(room / messages.length) - TICK))
  const slots = room > 0 ? fits(gap) : messages.length
  const per = Math.max(1, Math.ceil(messages.length / slots))
  const items: TimelineTick[] = []
  for (let i = 0; i < messages.length; i += per) {
    const group = messages.slice(i, i + per)
    items.push({ ids: group.map((m) => m.id), messages: group, unread: group.some((m) => m.unread) })
  }
  return { gap, items }
}
