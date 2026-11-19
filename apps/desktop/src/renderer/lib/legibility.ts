// D165 (personalizacao.md, "Piso de legibilidade"): the glass over a wallpaper never gets so clear that
// secondary text (`ink-2`) falls under 4.5:1. Pure math, so the floor is testable without a canvas.

export type RGB = readonly [number, number, number]

/** Lowest glass opacity the Transparência control offers (`opacity-glass-floor`). */
export const GLASS_FLOOR = 0.55
/** With `prefers-contrast: more`, the glass never goes below this. */
export const HIGH_CONTRAST_FLOOR = 0.92
/** Secondary text on glass: WCAG AA for normal text. */
export const MIN_CONTRAST = 4.5

const channel = (c: number) => {
  const s = c / 255
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

export function luminance([r, g, b]: RGB) {
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

export function contrast(a: RGB, b: RGB) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number]
  return (hi + 0.05) / (lo + 0.05)
}

/**
 * A semantic text color on the layers covered by `reference` (ink-2). Preserve it if it is already on
 * the more legible side; otherwise mix only its foreground toward black/light or white/dark until its
 * luminance is at least as safe as the reference. Fills, focus rings and the source tokens stay intact.
 */
export function readableForeground(color: RGB, reference: RGB, dark: boolean): RGB {
  const limit = luminance(reference)
  const safe = (c: RGB) => dark ? luminance(c) >= limit : luminance(c) <= limit
  return adjustedForeground(color, dark, safe)
}

/** Text over a separately composed layer (Donna's selected card + hovered inner button). */
export function foregroundOn(color: RGB, ground: RGB, dark: boolean): RGB {
  return adjustedForeground(color, dark, (c) => contrast(c, ground) >= MIN_CONTRAST)
}

/**
 * D169 (pedido do dono 28/10, personalizacao.md "Piso de legibilidade"): a text color that keeps `ratio` on every
 * ground, moved only toward black (light) or white (dark) as far as the clearest glass needs; a color already
 * legible there stays as it is. Secondary and tertiary text keep their contrast by their own color, so the glass
 * is held up only for primary text and the light surfaces show the wallpaper the person asked for.
 */
export function legibleOn(color: RGB, grounds: readonly RGB[], dark: boolean, ratio = MIN_CONTRAST): RGB {
  return adjustedForeground(color, dark, (c) => grounds.every((g) => contrast(c, g) >= ratio))
}

function adjustedForeground(color: RGB, dark: boolean, safe: (c: RGB) => boolean): RGB {
  if (safe(color)) return color
  const end: RGB = dark ? [255, 255, 255] : [0, 0, 0]
  if (!safe(end)) throw new RangeError('A camada não permite texto legível no tema atual.') // textos-ok: invariante interna do cálculo, nunca mostrada
  let lo = 0
  let hi = 1
  for (let step = 0; step < 24; step++) {
    const mid = (lo + hi) / 2
    if (safe(over(end, mid, color))) hi = mid
    else lo = mid
  }
  // Round toward the safe side, so serializing to CSS cannot weaken the bound.
  return over(end, hi, color).map((c) => dark ? Math.ceil(c) : Math.floor(c)) as unknown as RGB
}

/** `top` at `alpha` over an opaque `bottom`. */
export function over(top: RGB, alpha: number, bottom: RGB): RGB {
  return [0, 1, 2].map((i) => top[i]! * alpha + bottom[i]! * (1 - alpha)) as unknown as RGB
}

/** Transparência 0–100% → glass opacity from 1 down to `GLASS_FLOOR`. */
export function glassAlpha(transparency: number) {
  const t = Math.min(Math.max(transparency, 0), 100) / 100
  return 1 - t * (1 - GLASS_FLOOR)
}

/** The inverse, to show an opacity as the percentage the person picks. */
export function transparencyFor(alpha: number) {
  return Math.round(((1 - Math.min(Math.max(alpha, GLASS_FLOOR), 1)) / (1 - GLASS_FLOOR)) * 100)
}

/** Per-channel range of a picture, after its transparent parts fall on `ground` (the canvas color). */
export interface Bounds {
  lo: RGB
  hi: RGB
}

/**
 * The range of each channel over every pixel (`pixels` is RGBA, as `getImageData` returns it). Scaling for
 * `cover`, cropping and blur only average pixels, so every color shown lies inside this box, whatever
 * part of the picture lands under the glass.
 */
export function channelBounds(pixels: ArrayLike<number>, ground: RGB): Bounds {
  const lo = [255, 255, 255]
  const hi = [0, 0, 0]
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const a = pixels[i + 3]! / 255
    for (let c = 0; c < 3; c++) {
      const v = pixels[i + c]! * a + ground[c]! * (1 - a)
      if (v < lo[c]!) lo[c] = v
      if (v > hi[c]!) hi[c] = v
    }
  }
  if (lo[0]! > hi[0]!) return { lo: ground, hi: ground }
  return { lo: lo as unknown as RGB, hi: hi as unknown as RGB }
}

/** The veil laid over the whole box (`wallpaper-veil` at `alpha`). */
export function veiledBounds(b: Bounds, veil: RGB, alpha: number): Bounds {
  return { lo: over(veil, alpha, b.lo), hi: over(veil, alpha, b.hi) }
}

/**
 * The box after the glass's `saturate(s)` backdrop filter (CSS filter matrix): saturation pushes channels
 * apart, so each output channel is bounded by the worst corner of the input box, then clamped.
 */
export function saturatedBounds(b: Bounds, s: number): Bounds {
  const m = [
    [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
    [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
    [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
  ]
  const clamp = (v: number) => Math.min(255, Math.max(0, v))
  const lo = m.map((row) => clamp(row.reduce((sum, k, c) => sum + k * (k >= 0 ? b.lo[c]! : b.hi[c]!), 0)))
  const hi = m.map((row) => clamp(row.reduce((sum, k, c) => sum + k * (k >= 0 ? b.hi[c]! : b.lo[c]!), 0)))
  return { lo: lo as unknown as RGB, hi: hi as unknown as RGB }
}

/** The color under the glass that is worst for the text: darkest in the light theme, lightest in the dark. */
export function worstOf(b: Bounds, dark: boolean): RGB {
  return dark ? b.hi : b.lo
}

/** A glass layer that carries text: its base color, how much it adds to the shared opacity, and the row tint drawn on it (if rows live there). */
export interface GlassLayer {
  color: RGB
  offset: number
  tint?: { color: RGB; alpha: number }
}

/** What lies under a layer's text: its glass at `alpha` (+ its offset) over `worst`, with its row tint on top. */
export function layerGround(worst: RGB, layer: GlassLayer, alpha: number): RGB {
  const ground = over(layer.color, Math.min(1, alpha + layer.offset), worst)
  return layer.tint ? over(layer.tint.color, layer.tint.alpha, ground) : ground
}

/** Independent 3:1 floor for glass controls; does not hold up other glass areas. */
export function controlGlassAlpha(choice: number, worst: RGB, layer: GlassLayer, ink: RGB): number {
  // Round upward to the same precision CSS receives, including an already-safe choice.
  const picked = Math.ceil(Math.min(1, Math.max(0, choice)) * 1000) / 1000
  const safe = (a: number) => contrast(ink, layerGround(worst, layer, a)) >= 3
  if (safe(picked)) return picked
  if (!safe(1)) return 1
  let lo = picked
  let hi = 1
  for (let step = 0; step < 24; step++) {
    const mid = (lo + hi) / 2
    if (safe(mid)) hi = mid
    else lo = mid
  }
  return Math.min(1, Math.ceil(hi * 1000) / 1000)
}

/**
 * The smallest shared glass opacity that keeps `ink` at 4.5:1 on every text-carrying layer over the worst
 * backdrop, with the layer's row tint (hover, field, selected) on top where rows live. Mathematically safe
 * for any crop, scale or blur of the picture; it can be higher than what a given window shows.
 */
export function safeFloor(worst: RGB, layers: readonly GlassLayer[], ink: RGB) {
  let floor = 0
  for (const layer of layers) {
    const tinted = (c: RGB) => (layer.tint ? over(layer.tint.color, layer.tint.alpha, c) : c)
    const ok = (a: number) => contrast(ink, tinted(over(layer.color, Math.min(1, a + layer.offset), worst))) >= MIN_CONTRAST
    if (!ok(1 - layer.offset)) throw new RangeError('A cor do texto não atinge 4.5:1 nem com a camada opaca.') // textos-ok: invariante interna do cálculo, nunca mostrada
    let lo = 0
    let hi = 1 - layer.offset
    if (ok(0)) continue
    for (let step = 0; step < 24; step++) {
      const mid = (lo + hi) / 2
      if (ok(mid)) hi = mid
      else lo = mid
    }
    floor = Math.max(floor, Math.min(1, Math.ceil(hi * 100) / 100))
  }
  return floor
}

/** What the glass actually uses: the person's choice, held up by the wallpaper's floor and the system's contrast. */
export function effectiveGlass(choice: number, floor: number, opts: { moreContrast: boolean; reduceTransparency: boolean }) {
  if (opts.reduceTransparency) return 1
  return Math.max(choice, floor, opts.moreContrast ? HIGH_CONTRAST_FLOOR : 0)
}
