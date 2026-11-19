// Native SVG UI measurement. The probe must use the rendered text's class, font and anchor.
import type { AxisTextBounds } from './chart-axis'
export type TextReader = (text: string) => AxisTextBounds

export function svgTextReader(probe: SVGTextElement): TextReader {
  const cache = new Map<string, AxisTextBounds>()
  return (text) => {
    const cached = cache.get(text)
    if (cached) return cached
    const previous = probe.textContent
    try {
      probe.textContent = text
      const box = probe.getBBox()
      const bounds = { left: -box.x, right: box.x + box.width }
      cache.set(text, bounds)
      return bounds
    } finally {
      probe.textContent = previous
    }
  }
}

/** Envelope of normal/focused ink, measured at the same anchor; avoids clipping when font weight changes. */
export function textEnvelope(...readers: TextReader[]): TextReader {
  return (text) => {
    const bounds = readers.map((read) => read(text))
    return { left: Math.max(...bounds.map((b) => b.left)), right: Math.max(...bounds.map((b) => b.right)) }
  }
}
