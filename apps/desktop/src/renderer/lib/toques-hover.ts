import { legibleOn, over, type RGB } from './legibility.ts'

/** D530: o mesmo 18% do accent no CSS. A superfície opaca protege só o item em hover,
 * inclusive sobre vidro/foto extremos, sem mudar pisos, tinta ou transparência do restante. */
export const TOQUES_HOVER_ALPHA = 0.18
export const TOQUES_HOVER_FILL = `color-mix(in srgb, var(--accent) ${TOQUES_HOVER_ALPHA * 100}%, var(--surface))`
export function toquesHoverInk(accent: RGB, surface: RGB, dark: boolean): RGB {
  const ground = over(accent, TOQUES_HOVER_ALPHA, surface)
  try { return legibleOn(accent, [ground], dark) }
  catch { return legibleOn(accent, [ground], !dark) } // superfície invertida: mantém 4,5:1 pelo outro extremo
}
