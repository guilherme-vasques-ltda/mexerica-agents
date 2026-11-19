// UI geometry only: the caller supplies actual text measurements, never payload/contract validation.
export interface AxisTextBounds { left: number; right: number }
export interface AxisTick extends AxisTextBounds { i: number; label: string; x: number }

/** Prefix plus ellipsis fitted to measured ink; no character-width approximation. */
export function fitMeasuredText(label: string, budget: number, measure: (label: string) => AxisTextBounds): string {
  const span = (text: string) => { const b = measure(text); return b.left + b.right }
  if (span(label) <= budget) return label
  if (span('…') > budget) return ''
  const chars = [...label]
  let lo = 0
  let hi = chars.length
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2)
    if (span(`${chars.slice(0, mid).join('')}…`) <= budget) lo = mid
    else hi = mid - 1
  }
  return `${chars.slice(0, lo).join('')}…`
}

/** Already positioned numeric ticks: keep the endpoints and only complete, separated intermediates. */
export function fullAxisTicks(ticks: readonly AxisTick[], gap = 8): AxisTick[] {
  if (ticks.length < 3) return [...ticks]
  const first = ticks[0]!
  const last = ticks[ticks.length - 1]!
  const kept = [first]
  for (const tick of ticks.slice(1, -1)) {
    const previous = kept[kept.length - 1]!
    if (tick.x - tick.left >= previous.x + previous.right + gap
      && tick.x + tick.right <= last.x - last.left - gap) kept.push(tick)
  }
  return [...kept, last]
}

/** Reserve both endpoints, then fit complete intermediates. Bounds are relative to the text anchor. */
export function lineAxisTicks(
  labels: readonly string[], width: number, left: number, innerW: number,
  measure: (label: string) => AxisTextBounds, inset = 0,
): AxisTick[] {
  if (!labels.length || width <= 2 * inset) return []
  const cache = new Map<string, AxisTextBounds>()
  const bounds = (label: string) => {
    let result = cache.get(label)
    if (!result) { result = measure(label); cache.set(label, result) }
    return result
  }
  const textWidth = (label: string) => { const b = bounds(label); return b.left + b.right }
  const fit = (label: string, budget: number) => fitMeasuredText(label, budget, bounds)

  const x = (i: number) => left + (labels.length === 1 ? innerW / 2 : (i / (labels.length - 1)) * innerW)
  const tick = (i: number, budget: number): AxisTick => {
    const label = fit(labels[i]!, budget)
    const b = bounds(label)
    return { i, label, ...b, x: Math.min(width - inset - b.right, Math.max(inset + b.left, x(i))) }
  }
  const edgeBudget = Math.max(12, (innerW - 14) / 2)
  const first = tick(0, labels.length === 1 ? innerW : edgeBudget)
  const last = tick(labels.length - 1, edgeBudget)
  const ticks = [first]
  for (let i = 1; i < labels.length - 1; i++) {
    const previous = ticks[ticks.length - 1]!
    const available = 2 * Math.min(x(i) - previous.x - previous.right - 14, last.x - last.left - 14 - x(i))
    const labelWidth = textWidth(labels[i]!)
    if (available < labelWidth) continue
    const candidate = tick(i, labelWidth)
    // SVG ink can extend asymmetrically around its anchor; verify actual edges in DOM mode.
    if (inset > 0 && (candidate.x - candidate.left < previous.x + previous.right + 14
      || candidate.x + candidate.right > last.x - last.left - 14)) continue
    ticks.push(candidate)
  }
  if (labels.length > 1) ticks.push(last)
  return ticks
}
