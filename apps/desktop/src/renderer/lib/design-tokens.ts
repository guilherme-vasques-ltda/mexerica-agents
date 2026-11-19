// D165: the design system's tokens come from apps/desktop/design/tokens.json, the one source.
// `designTokensCSS` compiles it to the same CSS the docs carry (tokens.css); the app imports its own
// copy (styles/tokens.css). `node tools/design-tokens.mjs` writes both; the test keeps them equal.

type Themed = string | { light: string; dark: string }

interface Token {
  name: string
  value: Themed
}

interface TokenGroup {
  tokens: Token[]
}

export interface DesignTokens {
  color: TokenGroup
  shadow: TokenGroup
  spacing: TokenGroup
  radius: TokenGroup
  size: TokenGroup
  blur: TokenGroup
  opacity: TokenGroup
  /** Motion and glow (MX-1287): plain values go to `:root`, themed ones to the theme blocks. */
  motion?: TokenGroup
  type: { families: Record<string, string> }
}

export const TOKENS_HEADER = '/* Gerado de tokens.json (mesma compilação do artifact). Não edite à mão: edite tokens.json e gere de novo. */'

const declaration = (name: string, value: string) => `  --${name}: ${value};`

/** A theme block: every token with a value for that theme; plain values only in the light block, which `:root` also covers. */
function themeBlock(groups: TokenGroup[], theme: 'light' | 'dark') {
  return groups.flatMap((g) => g.tokens).flatMap((t) => {
    if (typeof t.value === 'string') return theme === 'light' ? [declaration(t.name, t.value)] : []
    return [declaration(t.name, t.value[theme])]
  })
}

/** Themed tokens of a group that is otherwise plain (motion): only the object values, per theme. */
function themedOnly(group: TokenGroup | undefined, theme: 'light' | 'dark') {
  return (group?.tokens ?? []).flatMap((t) => typeof t.value === 'string' ? [] : [declaration(t.name, t.value[theme])])
}

function plainBlock(groups: TokenGroup[]) {
  return groups.flatMap((g) => g.tokens).map((t) => declaration(t.name, typeof t.value === 'string' ? t.value : t.value.light))
}

/** Plain motion tokens (strings) for `:root`; themed ones come through `themedOnly`. */
function plainOnly(group: TokenGroup | undefined) {
  return (group?.tokens ?? []).flatMap((t) => typeof t.value === 'string' ? [declaration(t.name, t.value)] : [])
}

export function designTokensCSS(tokens: DesignTokens) {
  const themed = [tokens.color, tokens.shadow]
  const families = Object.entries(tokens.type.families).map(([name, value]) => declaration(`font-${name}`, value))
  return [
    TOKENS_HEADER,
    '',
    ':root, [data-theme="light"] {',
    ...themeBlock(themed, 'light'),
    ...themedOnly(tokens.motion, 'light'),
    declaration('ink-rgb', inkRGB(tokens, 'light')),
    '}',
    '',
    '[data-theme="dark"] {',
    ...themeBlock(themed, 'dark'),
    ...themedOnly(tokens.motion, 'dark'),
    declaration('ink-rgb', inkRGB(tokens, 'dark')),
    '}',
    '',
    ':root {',
    ...plainBlock([tokens.spacing, tokens.radius, tokens.size, tokens.blur, tokens.opacity]),
    ...plainOnly(tokens.motion),
    ...families,
    '}',
    '',
  ].join('\n')
}

/** `#rrggbb` or `rgba(r, g, b, a)` → [r, g, b, a]. */
export function parseColor(value: string): [number, number, number, number] {
  const hex = /^#([0-9a-f]{6})$/i.exec(value.trim())
  if (hex) {
    const n = parseInt(hex[1]!, 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1]
  }
  const rgba = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(value.trim())
  if (rgba) return [Number(rgba[1]), Number(rgba[2]), Number(rgba[3]), rgba[4] === undefined ? 1 : Number(rgba[4])]
  throw new Error(`Cor fora do formato dos tokens: ${value}`)
}

/** A color token's value in one theme. */
export function tokenValue(tokens: DesignTokens, name: string, theme: 'light' | 'dark') {
  const token = tokens.color.tokens.find((t) => t.name === name)
  if (!token) throw new Error(`Token ausente: ${name}`)
  return typeof token.value === 'string' ? token.value : token.value[theme]
}

/** Opaque ink channels for rgba(var(--ink-rgb), alpha), derived from the single palette. */
function inkRGB(tokens: DesignTokens, theme: 'light' | 'dark') {
  const [r, g, b, a] = parseColor(tokenValue(tokens, 'ink', theme))
  if (a !== 1 || [r, g, b].some((c) => !Number.isInteger(c) || c < 0 || c > 255)) {
    throw new Error(`ink precisa de canais RGB inteiros opacos no tema ${theme}`)
  }
  return `${r}, ${g}, ${b}`
}
