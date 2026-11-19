import { parseClosedJson } from './json-fechado.ts'

// D273 110A/111A: pure model of the chart card.
// Defensive mirror of the Hub's v1 contract (host/src/fork/genui/charts.rs, PR #221), same limits; the Hub
// stays the authority. Nothing here fetches, acts or turns text into markup: every string is shown as text.

export type Component = 'line_chart' | 'bar_chart' | 'pie_chart'
export type XKind = 'category' | 'date'
export interface Series { id: string; label: string; values: (number | null)[] }
export interface Chart {
  requested: Component
  effective: Component
  title: string
  summary: string
  xKind: XKind | null
  labels: string[]
  series: Series[]
  unit: string
  period: string
  sourceLabel: string
}
export type Parsed = { ok: true; chart: Chart } | { ok: false; code: string }

export const MAX_BYTES = 64 * 1024
export const MAX_VALUE = 9_007_199_254_740_991
export const MAX_POINTS = 500
export const MAX_SERIES = 3
export const PIE_MAX_SLICES = 5
export const UNAVAILABLE = 'Indisponível'
export const KIND_LABEL: Record<Component, string> = { line_chart: 'Linha', bar_chart: 'Barras', pie_chart: 'Pizza' }

const COMPONENTS: readonly string[] = ['line_chart', 'bar_chart', 'pie_chart']
const ID = /^[A-Za-z0-9_.-]{1,160}$/
const CONTROL = /[\p{Cc}]/u

class Refused extends Error {}
const refuse = (code: string): never => { throw new Refused(code) }
const ensure = (ok: boolean, code: string) => { if (!ok) refuse(code) }

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
/** Closed object: exactly the allowed keys, the required ones present (serde deny_unknown_fields). */
function closed(v: unknown, required: readonly string[], optional: readonly string[] = []): Record<string, unknown> {
  if (!isObject(v)) return refuse('genui_chart_invalid_payload')
  const keys = Object.keys(v)
  ensure(keys.every((k) => required.includes(k) || optional.includes(k)) && required.every((k) => Object.hasOwn(v, k)), 'genui_chart_invalid_payload')
  return v
}
const str = (v: unknown) => (typeof v === 'string' ? v : refuse('genui_chart_invalid_payload'))

function text(value: string, max: number) {
  ensure(value.trim() !== '' && [...value].length <= max && ![...value].some((c) => c !== '\n' && c !== '\t' && CONTROL.test(c)), 'genui_chart_invalid_text')
}

function realDate(y: number, m: number, d: number) {
  const t = new Date(0)
  t.setUTCFullYear(y, m - 1, d)
  return t.getUTCFullYear() === y && t.getUTCMonth() === m - 1 && t.getUTCDate() === d
}
/** `YYYY-MM-DD`, or RFC3339 with `T` and an explicit `Z`/`±HH:MM` zone; `-00:00` (unknown zone) refused. */
export function validDate(value: string): boolean {
  const day = /^(\d{4})-(\d{2})-(\d{2})/.exec(value)
  if (!day || !realDate(+day[1]!, +day[2]!, +day[3]!)) return false
  if (value.length === 10) return true
  if (value.endsWith('-00:00')) return false
  const time = /^T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|([+-])(\d{2}):(\d{2}))$/.exec(value.slice(10))
  if (!time) return false
  const [, h, mi, s, , oh, om] = time
  return +h! <= 23 && +mi! <= 59 && +s! <= 60 && (oh === undefined || (+oh <= 23 && +om! <= 59))
}

function payloadToChart(raw: unknown): Chart {
  const p = closed(raw, ['version', 'component', 'title', 'summary', 'data'])
  ensure(typeof p.version === 'number' && Number.isInteger(p.version) && p.version >= 0 && p.version <= 255, 'genui_chart_invalid_payload')
  const component = str(p.component)
  ensure(COMPONENTS.includes(component), 'genui_chart_invalid_payload')
  const d = closed(p.data, ['labels', 'series', 'unit', 'period', 'sourceLabel'], ['xKind'])
  if (Object.hasOwn(d, 'xKind')) ensure(d.xKind === 'category' || d.xKind === 'date', 'genui_chart_invalid_payload')
  ensure(Array.isArray(d.labels) && Array.isArray(d.series), 'genui_chart_invalid_payload')
  const labels = (d.labels as unknown[]).map(str)
  const series = (d.series as unknown[]).map((s) => {
    const o = closed(s, ['id', 'label', 'values'])
    ensure(Array.isArray(o.values), 'genui_chart_invalid_payload')
    const values = (o.values as unknown[]).map((v) => v === null || typeof v === 'number' ? v : refuse('genui_chart_invalid_payload'))
    return { id: str(o.id), label: str(o.label), values }
  })
  const chart: Chart = {
    requested: component as Component, effective: component as Component,
    title: str(p.title), summary: str(p.summary), xKind: (d.xKind as XKind | undefined) ?? null,
    labels, series, unit: str(d.unit), period: str(d.period), sourceLabel: str(d.sourceLabel),
  }

  ensure(p.version === 1, 'genui_chart_unsupported_version')
  text(chart.title, 160)
  text(chart.summary, 2_000)
  for (const v of [chart.unit, chart.period, chart.sourceLabel]) text(v, 160)
  ensure(labels.length >= 1 && labels.length <= MAX_POINTS, 'genui_chart_invalid_labels')
  ensure(series.length >= 1 && series.length <= MAX_SERIES, 'genui_chart_invalid_series')
  ensure(component === 'line_chart' ? chart.xKind !== null : !Object.hasOwn(d, 'xKind'), 'genui_chart_invalid_x_kind')
  for (const label of labels) {
    text(label, 160)
    if (chart.xKind === 'date') ensure(validDate(label), 'genui_chart_invalid_date')
  }
  const ids = new Set<string>()
  for (const s of series) {
    ensure(ID.test(s.id), 'genui_chart_invalid_series_id')
    ensure(!ids.has(s.id), 'genui_chart_duplicate_series_id')
    ids.add(s.id)
    text(s.label, 160)
    ensure(s.values.length === labels.length, 'genui_chart_irregular_matrix')
    for (const v of s.values) if (v !== null) ensure(Number.isFinite(v) && Math.abs(v) <= MAX_VALUE, 'genui_chart_invalid_number')
  }
  if (component === 'pie_chart') {
    ensure(series.length === 1, 'genui_chart_invalid_pie')
    const total: number[] = []
    for (const v of series[0]!.values) {
      ensure(v !== null, 'genui_chart_incomplete_pie')
      ensure(v! >= 0, 'genui_chart_invalid_pie')
      addExact(total, v!)
    }
    ensure(total.length > 0, 'genui_chart_invalid_pie_total')
    addExact(total, -MAX_VALUE)
    ensure(total.length === 0 || total[total.length - 1]! < 0, 'genui_chart_invalid_pie_total')
    // B (decided): above five parts, bars with every label and value; no "Outros".
    if (labels.length > PIE_MAX_SLICES) chart.effective = 'bar_chart'
  }
  // This renderer copy is immutable too; it never edits the snapshot's labels or values.
  for (const s of series) { Object.freeze(s.values); Object.freeze(s) }
  Object.freeze(labels)
  Object.freeze(series)
  return Object.freeze(chart)
}

function guarded(raw: string, read: (value: unknown) => Chart): Parsed {
  try {
    if (typeof raw !== 'string' || new TextEncoder().encode(raw).length > MAX_BYTES) return { ok: false, code: 'genui_chart_too_large' }
    const parsed = parseClosedJson(raw, { unsignedIntegerFields: ['version'] })
    if (!parsed.ok) return { ok: false, code: 'genui_chart_invalid_payload' }
    return { ok: true, chart: read(parsed.value) }
  } catch (e) {
    if (e instanceof Refused) return { ok: false, code: e.message }
    throw e
  }
}

/** The v1 payload as the bot sends it. Refusal carries only a stable code, never the body. */
export const parseChart = (raw: string): Parsed => guarded(raw, payloadToChart)

/** The Hub's `{requested, effective, payload}` snapshot; refused unless it agrees with the payload. */
export const parseChartSnapshot = (raw: string): Parsed => guarded(raw, (value) => {
  const s = closed(value, ['requested', 'effective', 'payload'])
  const chart = payloadToChart(s.payload)
  ensure(s.requested === chart.requested && s.effective === chart.effective, 'genui_chart_invalid_snapshot')
  return chart
})

/** Increasing nonoverlapping expansion, matching the Hub's TwoSum comparison. Never collapse the
 * residue before comparing the exact sum with MAX_VALUE; MAX + 0.1 exceeds it in either order. */
function addExact(partials: number[], value: number) {
  let kept = 0
  for (const partial of partials) {
    const high = value + partial
    const recovered = high - value
    const low = (value - (high - recovered)) + (partial - recovered)
    value = high
    if (low !== 0) partials[kept++] = low
  }
  partials.length = kept
  if (value !== 0) partials.push(value)
}

// ── Numbers ────────────────────────────────────────────────────────────────

/** Shortest exact decimal of a double, never in exponent form. */
function plainDecimal(n: number): string {
  const s = String(Math.abs(n))
  const m = /^(\d)(?:\.(\d+))?e([+-]\d+)$/.exec(s)
  if (!m) return s
  const digits = m[1]! + (m[2] ?? '')
  const exp = Number(m[3])
  return exp < 0 ? `0.${'0'.repeat(-exp - 1)}${digits}` : digits + '0'.repeat(exp - digits.length + 1)
}

/** The accepted value exactly, in pt-BR (1.234,567): no rounding. */
export function formatExact(n: number): string {
  const [int, frac] = plainDecimal(n).split('.')
  const grouped = int!.replace(/\B(?=(\d{3})+$)/g, '.')
  return `${n < 0 ? '-' : ''}${grouped}${frac ? `,${frac}` : ''}`
}

/** Inverse of formatExact for the table's sort; Indisponível sorts before every number. */
export function parseExact(cell: string): number | null {
  if (cell === UNAVAILABLE) return -Infinity
  if (!/^-?\d{1,3}(?:\.\d{3})*(?:,\d+)?$/.test(cell)) return null
  return Number(cell.replace(/\./g, '').replace(',', '.'))
}

export interface Scale { min: number; max: number; step: number; ticks: number[] }

/** Round ticks that always include zero; never NaN, never a zero-height span. */
export function niceScale(lo: number, hi: number, target = 4): Scale {
  let min = Math.min(0, lo)
  let max = Math.max(0, hi)
  if (!(Number.isFinite(min) && Number.isFinite(max))) { min = 0; max = 1 }
  if (min === max) max = 1
  const raw = (max - min) / target
  const mag = 10 ** Math.floor(Math.log10(raw))
  const norm = raw / mag
  const step = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10) * mag
  if (!(step > 0 && Number.isFinite(step))) return { min, max, step: max - min, ticks: [min, max] }
  const from = Math.floor(min / step) * step
  const to = Math.ceil(max / step) * step
  const count = Math.round((to - from) / step)
  const ticks = Array.from({ length: count + 1 }, (_, i) => Number((from + i * step).toPrecision(12)))
  return { min: ticks[0]!, max: ticks[count]!, step, ticks }
}

/** Position of a value in [0, 1] along the scale. */
export const ratio = (scale: Scale, v: number) => (v - scale.min) / (scale.max - scale.min)

const compact = new Intl.NumberFormat('pt-BR', { notation: 'compact', maximumFractionDigits: 1 })
/** Axis tick text: compact for big magnitudes, otherwise the step's precision. Exact values live in the table. */
export function formatTick(v: number, scale: Scale): string {
  if (Math.max(Math.abs(scale.min), Math.abs(scale.max)) >= 1e6) return compact.format(v)
  if (v !== 0 && Math.abs(v) < 1e-6) return new Intl.NumberFormat('pt-BR', { notation: 'scientific', maximumFractionDigits: 2 }).format(v)
  // One digit beyond the step's magnitude covers 2,5 × 10^k steps; trailing zeros are dropped.
  const decimals = Math.min(20, Math.max(0, 1 - Math.floor(Math.log10(scale.step))))
  return new Intl.NumberFormat('pt-BR', { minimumFractionDigits: 0, maximumFractionDigits: decimals }).format(v)
}

// ── Shapes ──────────────────────────────────────────────────────────────────

/** Runs of consecutive available values; a null breaks the line (never bridged, never zero). */
export function segments(values: readonly (number | null)[]): number[][] {
  const out: number[][] = []
  let run: number[] = []
  values.forEach((v, i) => {
    if (v === null) { if (run.length) out.push(run); run = [] } else run.push(i)
  })
  if (run.length) out.push(run)
  return out
}

export function pieTotal(chart: Chart): number {
  const partials: number[] = []
  for (const value of chart.series[0]!.values) addExact(partials, value ?? 0)
  return partials.reduce((sum, value) => sum + value, 0)
}

/** Share of the whole, never rounded to a false 0% or 100%. */
export function percentText(value: number, total: number, hasOtherPositive = false): string {
  if (value === 0) return '0%'
  if (value === total && !hasOtherPositive) return '100%'
  const p = (value / total) * 100
  if (p < 0.1) return '<0,1%'
  if (p > 99.9) return '>99,9%'
  return `${new Intl.NumberFormat('pt-BR', { maximumFractionDigits: 1 }).format(p)}%`
}

export interface Slice { index: number; start: number; end: number }
/** Clockwise from noon, angles in radians; zero values get no slice. */
export function pieSlices(values: readonly number[]): Slice[] {
  const total = values.reduce((a, v) => a + v, 0)
  let at = 0
  const out: Slice[] = []
  values.forEach((v, index) => {
    if (v <= 0) return
    const start = at
    at += v / total
    out.push({ index, start: start * 2 * Math.PI, end: at * 2 * Math.PI })
  })
  // Close the circle exactly: float sums may stop a hair short of 1.
  if (out.length) out[out.length - 1]!.end = 2 * Math.PI
  return out
}

// ── Text equivalents ────────────────────────────────────────────────────────

const valueText = (v: number | null, unit: string) => (v === null ? UNAVAILABLE : `${formatExact(v)} ${unit}`)

/** What the focused point/bar/slice says, the same for keyboard, pointer and the live region. */
export function readout(chart: Chart, index: number, visible: readonly boolean[]): string {
  const label = chart.labels[index] ?? ''
  if (chart.effective === 'pie_chart' || chart.requested === 'pie_chart') {
    const v = chart.series[0]!.values[index] ?? 0
    return `${label}: ${valueText(v, chart.unit)} · ${percentText(v, pieTotal(chart), chart.series[0]!.values.some((n, i) => i !== index && n !== null && n > 0))}`
  }
  if (chart.series.length === 1) return `${label}: ${valueText(chart.series[0]!.values[index] ?? null, chart.unit)}`
  const parts = chart.series.filter((_, i) => visible[i]).map((s) => `${s.label} ${valueText(s.values[index] ?? null, chart.unit)}`)
  return `${label}: ${parts.join('; ')}`
}

/** Ver dados: every label and value, exact; null as Indisponível. Plain text cells. */
export function tableData(chart: Chart): { header: string[]; rows: string[][] } {
  const first = chart.xKind === 'date' ? 'Data' : 'Categoria'
  return {
    header: [first, ...chart.series.map((s) => s.label)],
    rows: chart.labels.map((label, i) => [label, ...chart.series.map((s) => { const v = s.values[i]; return v === null || v === undefined ? UNAVAILABLE : formatExact(v) })]),
  }
}

export const contextLine = (chart: Chart) => `${chart.period} · ${chart.unit}`
