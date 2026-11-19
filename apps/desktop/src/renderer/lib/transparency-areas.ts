import { ptBR } from './fork-i18n/pt-BR.ts'
import type { ChaveTexto } from './fork-i18n/index.ts'
import type { GlassLayer, RGB } from './legibility'

// D169 (personalizacao.md, "Transparência por área"): which percentage each area uses. Geral puts the
// general Transparência on all four areas; Por área lets each one keep its own value, and an area with
// none follows the general one. Pure, so the rules are testable without a window.

export type TransparencyMode = 'general' | 'custom'
export type TransparencyArea = 'sidebar' | 'details' | 'composer' | 'conversation'
/** The areas that have a value of their own (Por área); a missing key follows the general value. */
export type AreaValues = Partial<Record<TransparencyArea, number>>

// D454 (fatia 4): `labelKey` é a chave do catálogo; `label`, o texto pt-BR dele (compatível com quem já lia).
export const TRANSPARENCY_MODES: { id: TransparencyMode; label: string; labelKey: ChaveTexto }[] = [
  { id: 'general', label: ptBR['ajustes.transparencia.geral'], labelKey: 'ajustes.transparencia.geral' },
  { id: 'custom', label: ptBR['ajustes.transparencia.porArea'], labelKey: 'ajustes.transparencia.porArea' },
]

export const TRANSPARENCY_AREAS: { id: TransparencyArea; label: string; labelKey: ChaveTexto }[] = [
  { id: 'sidebar', label: ptBR['ajustes.transparencia.barraLateral'], labelKey: 'ajustes.transparencia.barraLateral' },
  { id: 'details', label: ptBR['ajustes.transparencia.painel'], labelKey: 'ajustes.transparencia.painel' },
  { id: 'composer', label: ptBR['ajustes.transparencia.caixa'], labelKey: 'ajustes.transparencia.caixa' },
  { id: 'conversation', label: ptBR['ajustes.transparencia.fundoConversa'], labelKey: 'ajustes.transparencia.fundoConversa' },
]

const AREA_IDS = new Set<string>(TRANSPARENCY_AREAS.map((a) => a.id))
const clampPercent = (value: number) => Math.min(Math.max(Number.isFinite(value) ? value : 0, 0), 100)
const isPercent = (value: unknown) => typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 100

/** A stored mode (prefs.ts `valid`): anything else falls back to Geral. */
export function isTransparencyMode(value: unknown): value is TransparencyMode {
  return value === 'general' || value === 'custom'
}

/** A stored set of area values: only known areas, each a whole percentage; anything else falls back to none. */
export function isAreaValues(value: unknown): value is AreaValues {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    && Object.entries(value).every(([area, percent]) => AREA_IDS.has(area) && isPercent(percent))
}

/** The percentage an area uses: its own in Por área, otherwise the general one. */
export function areaChoice(area: TransparencyArea, mode: TransparencyMode, general: number, own: AreaValues): number {
  return clampPercent((mode === 'custom' ? own[area] : undefined) ?? general)
}

/** True when the area follows the general value. */
export function followsGeneral(area: TransparencyArea, mode: TransparencyMode, own: AreaValues): boolean {
  return mode !== 'custom' || own[area] === undefined
}

/** Sets one area's own value, or clears it (`null`, "Usar a geral"), leaving the other areas as they were. */
export function withArea(own: AreaValues, area: TransparencyArea, value: number | null): AreaValues {
  const next: AreaValues = { ...own }
  if (value === null) delete next[area]
  else next[area] = Math.round(clampPercent(value))
  return next
}

/** Fundo da conversa: the canvas plate behind the messages, opaque at 0% and gone at 100% (as before). */
export function conversationPlate(transparency: number): number {
  return 1 - clampPercent(transparency) / 100
}

/** The strongest row tint over glass (`selected`: ink at 8% light, 10% dark); fields and hovers are lighter. */
export const ROW_TINT = 0.1

/**
 * The details panel's glass as a text-carrying layer: `glass-raised` (+0.02) with the row tint under its
 * text, since the panel holds fields and buttons in `field`. Its floor is taken with the shared one.
 */
export function detailsLayer(tokens: (name: string) => RGB): GlassLayer {
  return { color: tokens('glass-raised'), offset: 0.02, tint: { color: tokens('ink'), alpha: ROW_TINT } }
}
