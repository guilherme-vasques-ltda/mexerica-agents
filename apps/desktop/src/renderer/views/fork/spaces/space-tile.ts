// D169 (components/SpaceIconPicker, SpaceRail): the tile's pure rules over the Hub's contract
// (shared/fork-spaces.ts, docs/reference/spaces.md). The Hub validates and keeps; this only builds a choice.
import { SPACE_ICON_IDS, type SpaceColor, type SpaceSymbol, type SpaceTile } from '../../../../shared/fork-spaces.ts'
import { DUO_ICONS, DUO_PACKS } from './space-icons.ts'
import { frase, type Frase } from '../../../lib/fork-i18n/frase.ts'

export const TILE_COLORS: { id: SpaceColor; name: string }[] = [
  { id: 'violeta', name: 'Violeta' },
  { id: 'cobre', name: 'Cobre' },
  { id: 'musgo', name: 'Musgo' },
  { id: 'oceano', name: 'Oceano' },
  { id: 'ameixa', name: 'Ameixa' },
  { id: 'grafite', name: 'Grafite' },
]

/** The tile's color as the design token (`tile-*`); an unknown value gets none. */
export const tileColor = (color: SpaceColor) => (TILE_COLORS.some((c) => c.id === color) ? `var(--tile-${color})` : undefined)

/**
 * Up to two letters or digits, as typed (no inference from a name), upper case in pt-BR. The Hub takes one or
 * two characters that are all alphanumeric (Rust `char::is_alphanumeric`: Alphabetic or Numeric), so anything
 * else is dropped here; upper case comes first, as it may turn one character into two (ß → SS).
 */
export function letters(input: string): string {
  const chars = [...input.normalize('NFC').toLocaleUpperCase('pt-BR').normalize('NFC')].filter((c) => /^[\p{Alphabetic}\p{N}]$/u.test(c))
  return chars.slice(0, 2).join('')
}
/** What the Hub keeps as letters (one or two alphanumeric characters), whatever their case. */
export const isLetters = (value: string) => {
  const chars = [...value]
  return chars.length >= 1 && chars.length <= 2 && chars.every((c) => /^[\p{Alphabetic}\p{N}]$/u.test(c))
}

/**
 * The Hub's rule for a space's name: trimmed, 1 to 60 characters, no control characters. Counted by code point,
 * so a cut never splits an emoji, and the spaces inside are kept: saving another change never rewrites a name
 * the Hub already took.
 */
export const NAME_MAX = 60
export const spaceName = (input: string) => [...input.replace(/\p{Cc}/gu, ' ').trim()].slice(0, NAME_MAX).join('').trim()
/**
 * While typing: the same 60, counted by code point like the Hub (the input's own `maxLength` counts UTF-16 units,
 * so 31 emoji would stop at 30 and a cut could split one). Nothing is trimmed here; `spaceName` does that on save.
 */
export const clipName = (input: string) => [...input].slice(0, NAME_MAX).join('')

/** A logo goes to the Hub as a data URL; it decides. Here only the obvious limits, so nothing large leaves the page. */
export const LOGO_TYPES = ['image/png', 'image/svg+xml'] as const
export const LOGO_MAX_BYTES = 256 * 1024
/** Lado máximo do logo raster depois de reduzido no picker (MX-79); o SVG segue vetorial. */
export const LOGO_MAX_SIDE = 256
export function logoProblem(file: { type: string; size: number }): Frase | null {
  if (!(LOGO_TYPES as readonly string[]).includes(file.type)) return frase('spaces.recusa.formato')
  if (file.size > LOGO_MAX_BYTES) return frase('spaces.recusa.logoGrande')
  return null
}
const logoURL = /^data:image\/(png|svg\+xml);base64,[A-Za-z0-9+/]+=*$/
export const isLogoURL = (value: string) => logoURL.test(value)
/** The owner's photo, only as the inline PNG the Hub keeps (no JPEG); never a path or a remote URL. */
export const isPhotoURL = (value: string) => /^data:image\/png;base64,[A-Za-z0-9+/]+=*$/.test(value)

/** The symbol the tile shows, or null when it can't be drawn from what this app knows. */
export function drawable(symbol: SpaceSymbol): SpaceSymbol | null {
  if (symbol.kind === 'icon') return (SPACE_ICON_IDS as readonly string[]).includes(symbol.value) && Object.hasOwn(DUO_ICONS, symbol.value) ? symbol : null
  if (symbol.kind === 'letters') return isLetters(symbol.value) ? symbol : null
  return isLogoURL(symbol.value) ? symbol : null
}

/** A tile the picker may hand to its caller: known color, style and a drawable symbol. */
export function validTile(tile: SpaceTile | null | undefined): tile is SpaceTile {
  return !!tile && !!tileColor(tile.color) && (tile.style === 'filled' || tile.style === 'soft') && drawable(tile.symbol) !== null
}

export const DEFAULT_TILE: SpaceTile = { color: 'cobre', style: 'filled', symbol: { kind: 'icon', value: 'briefcase' } }

/** Icons of a pack, or every icon whose pt-BR name matches the search (accents ignored). */
export function iconsFor(pack: string, query: string): string[] {
  const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim()
  const q = fold(query)
  if (q) return DUO_PACKS.flatMap((p) => p.icons).filter((id) => fold(DUO_ICONS[id]!.name).includes(q))
  return DUO_PACKS.find((p) => p.id === pack)?.icons ?? []
}
