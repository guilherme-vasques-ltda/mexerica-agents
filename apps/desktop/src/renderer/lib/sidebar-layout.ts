import { ptBR } from './fork-i18n/pt-BR.ts'
import type { ChaveTexto } from './fork-i18n/index.ts'
// D165, fase C (docs/reference/redesenho/components/SidebarSettings): the sidebar's sections, their order
// and which show. Barra04 (dono, 05/11/2026): Canais are the group chats (`kind: "group"`), Equipes the teams
// from Times (each a chief's row) and Agentes the bots without a team, with the D114 groups as dividers.

/** Barra05 (dono, 05/11/2026): a largura da barra, numa constante só. Arrastar a borda vai de `min` a `max`; duplo clique volta ao padrão. */
export const SIDEBAR_WIDTH = { default: 280, min: 200, max: 360 } as const

/** A largura guardada (ou arrastada) dentro dos limites; um valor estranho volta ao padrão. */
export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_WIDTH.default
  return Math.round(Math.min(Math.max(width, SIDEBAR_WIDTH.min), SIDEBAR_WIDTH.max))
}

export type SidebarSection = 'channels' | 'teams' | 'agents'

export const SIDEBAR_SECTIONS: { id: SidebarSection; label: string; labelKey: ChaveTexto }[] = [
  { id: 'channels', label: ptBR['ajustes.secao.canais'], labelKey: 'ajustes.secao.canais' },
  { id: 'teams', label: ptBR['ajustes.secao.equipes'], labelKey: 'ajustes.secao.equipes' },
  { id: 'agents', label: ptBR['ajustes.secao.agentes'], labelKey: 'ajustes.secao.agentes' },
]

/** Barra04: the old "Bots" section became Equipes and Agentes; a saved `bots` (order, hidden, folded) stands for both. */
export function expandLegacySections(saved: readonly string[]): string[] {
  if (!Array.isArray(saved)) return []
  return saved.flatMap((id) => (id === 'bots' ? ['teams', 'agents'] : [id]))
}

/** A list of strings, as the sidebar's section preferences are stored (D165 revisão C, C3). */
export function isStringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

/** A saved order with unknown ids dropped and missing ones appended in the default order. */
export function sectionOrder(saved: readonly string[]): SidebarSection[] {
  const known = SIDEBAR_SECTIONS.map((s) => s.id)
  // Defensive: a stored value of the wrong shape is the default order, never a crash.
  saved = expandLegacySections(saved)
  const kept = saved.filter((id, i): id is SidebarSection => known.includes(id as SidebarSection) && saved.indexOf(id) === i)
  return [...kept, ...known.filter((id) => !kept.includes(id))]
}

/** The sections the sidebar draws, in order: hidden ones leave without losing anything. */
export function visibleSections(saved: readonly string[], hidden: readonly string[]): SidebarSection[] {
  const off = expandLegacySections(hidden)
  return sectionOrder(saved).filter((id) => !off.includes(id))
}

/** One place up (-1) or down (+1) in the order (⌥↑/⌥↓ in Ajustes → Barra lateral). */
export function moveSection(saved: readonly string[], id: SidebarSection, offset: number): SidebarSection[] {
  const order = sectionOrder(saved)
  const from = order.indexOf(id)
  const to = from + offset
  if (from < 0 || to < 0 || to >= order.length) return order
  const next = [...order]
  next.splice(from, 1)
  next.splice(to, 0, id)
  return next
}

/** Splits a computer's rows into Canais (groups) and the direct bots, keeping each one's order. */
export function splitChannels<T extends { bot: { kind: string } }>(items: readonly T[]) {
  return {
    channels: items.filter((item) => item.bot.kind === 'group'),
    bots: items.filter((item) => item.bot.kind !== 'group'),
  }
}

/** Rows in the order ↑/↓ walks them: visible, unfolded sections only, in the sidebar's order. */
export function navigableRows<T>(order: readonly SidebarSection[], folded: readonly string[], parts: Record<SidebarSection, readonly T[]>): T[] {
  const closed = expandLegacySections(folded)
  return order.filter((id) => !closed.includes(id)).flatMap((id) => parts[id])
}

/** "09:12" today, "ontem", or "14 out": the time a row shows when nothing is unread. */
export function rowTime(at: number, now = new Date()): string {
  if (!at) return ''
  const date = new Date(at)
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const days = Math.round((day(now) - day(date)) / 86_400_000)
  if (days <= 0) return date.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
  if (days === 1) return 'ontem'
  return date.toLocaleDateString('pt-BR', { day: 'numeric', month: 'short' }).replace('.', '').replace(' de ', ' ')
}
