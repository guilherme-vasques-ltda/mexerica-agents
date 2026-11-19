import { ptBR } from './fork-i18n/pt-BR.ts'

// Barra04 (dono, 05/11/2026; docs/reference/design/times-lider-20261105/implementacao.md, passo 2): a fonte do app e as cores das
// etiquetas da barra lateral. Só dados e regras puras; quem grava e aplica no <html> é lib/appearance.ts.

/** Ajustes → Aparência → Fonte. "Sistema" é a pilha atual dos tokens (SF Pro no Mac); as outras vêm em styles/fonts.css. */
export const APP_FONTS = [
  { id: 'sistema', name: ptBR['ajustes.fonte.sistema'], nameKey: 'ajustes.fonte.sistema', family: null },
  { id: 'inter', name: 'Inter', family: "'Inter'" },
  { id: 'geist', name: 'Geist', family: "'Geist'" },
  { id: 'manrope', name: 'Manrope', family: "'Manrope'" },
  { id: 'plex', name: 'IBM Plex Sans' /* textos-ok: nome próprio da fonte, igual em todo idioma */, family: "'IBM Plex Sans'" },
  { id: 'dmsans', name: 'DM Sans' /* textos-ok: nome próprio da fonte, igual em todo idioma */, family: "'DM Sans'" },
] as const

export type AppFont = (typeof APP_FONTS)[number]['id']
export const DEFAULT_APP_FONT: AppFont = 'dmsans'
export const isAppFont = (value: unknown): value is AppFont => APP_FONTS.some((f) => f.id === value)

/**
 * O `--font-sans` da escolha: a fonte embutida na frente da pilha do sistema (`--font-system`, que cobre o que o
 * subconjunto latino não tem), ou null para "Sistema", que deixa o valor dos tokens.
 */
export function fontStack(id: AppFont): string | null {
  const family = APP_FONTS.find((f) => f.id === id)?.family ?? null
  return family ? `${family}, var(--font-system)` : null
}

/** Ajustes → Barra lateral → Cores das etiquetas: os tokens `tag-<cor>-bg`/`tag-<cor>-ink`. */
export const TAG_COLORS = [
  { id: 'azul', name: ptBR['ajustes.cor.azul'], nameKey: 'ajustes.cor.azul' },
  { id: 'jade', name: ptBR['ajustes.cor.jade'], nameKey: 'ajustes.cor.jade' },
  { id: 'ambar', name: ptBR['ajustes.cor.ambar'], nameKey: 'ajustes.cor.ambar' },
  { id: 'rosa', name: ptBR['ajustes.cor.rosa'], nameKey: 'ajustes.cor.rosa' },
  { id: 'violeta', name: ptBR['ajustes.cor.violeta'], nameKey: 'ajustes.cor.violeta' },
  { id: 'grafite', name: ptBR['ajustes.cor.grafite'], nameKey: 'ajustes.cor.grafite' },
] as const
export type TagColor = (typeof TAG_COLORS)[number]['id']

/** Nível 1 (chefes do topo), 2 (logo abaixo), 3 (e abaixo) e os bots fora das equipes (Agentes). */
export const TAG_SLOTS = [
  { id: 'l1', name: ptBR['ajustes.etiqueta.l1'], nameKey: 'ajustes.etiqueta.l1', detail: ptBR['ajustes.etiqueta.l1Detalhe'], detailKey: 'ajustes.etiqueta.l1Detalhe' },
  { id: 'l2', name: ptBR['ajustes.etiqueta.l2'], nameKey: 'ajustes.etiqueta.l2', detail: ptBR['ajustes.etiqueta.l2Detalhe'], detailKey: 'ajustes.etiqueta.l2Detalhe' },
  { id: 'l3', name: ptBR['ajustes.etiqueta.l3'], nameKey: 'ajustes.etiqueta.l3', detail: ptBR['ajustes.etiqueta.l3Detalhe'], detailKey: 'ajustes.etiqueta.l3Detalhe' },
  { id: 'l0', name: ptBR['ajustes.etiqueta.l0'], nameKey: 'ajustes.etiqueta.l0', detail: ptBR['ajustes.etiqueta.l0Detalhe'], detailKey: 'ajustes.etiqueta.l0Detalhe' },
] as const
export type TagSlot = (typeof TAG_SLOTS)[number]['id']
export type TagColors = Record<TagSlot, TagColor>

/** Padrão do dono: tudo Grafite, para não encher a barra de cores. */
export const DEFAULT_TAG_COLORS: TagColors = { l1: 'grafite', l2: 'grafite', l3: 'grafite', l0: 'grafite' }

const isTagColor = (value: unknown): value is TagColor => TAG_COLORS.some((c) => c.id === value)

/** Um valor guardado com forma estranha não derruba a barra: cada nível inválido volta ao padrão. */
export function readTagColors(value: unknown): TagColors {
  const saved = value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
  return Object.fromEntries(TAG_SLOTS.map(({ id }) => [id, isTagColor(saved[id]) ? saved[id] : DEFAULT_TAG_COLORS[id]])) as TagColors
}

/** As variáveis que a barra lê (`--tag-l1-bg`…), apontando para os tokens da cor escolhida. */
export function tagColorVars(colors: TagColors): [string, string][] {
  return TAG_SLOTS.flatMap(({ id }) => [[`--tag-${id}-bg`, `var(--tag-${colors[id]}-bg)`], [`--tag-${id}-ink`, `var(--tag-${colors[id]}-ink)`]] as [string, string][])
}
