// D273 110A/111A: the chart card, preparatory and NOT
// mounted in the chat, Markdown, wire, RPC, MCP or voice. It draws an already validated `Chart` with its own
// SVG, a text summary first and Ver dados over the existing DataTable. No callback, fetch or effect of its own:
// the only actions are the person's (focus, legend, Ver dados, and the table's own copy/CSV).
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { DataTable } from '../../thread/cards/DataTable'
import {
  contextLine, formatExact, formatTick, niceScale, parseExact, percentText, pieSlices, pieTotal, ratio, readout,
  segments, tableData, type Chart, type Words,
} from './graficos-model'
import { fitMeasuredText, fullAxisTicks, lineAxisTicks, type AxisTextBounds, type AxisTick } from './chart-axis'
import { svgTextReader, textEnvelope } from './chart-text'
import { superficieIdioma, useIdioma, useT, tradutor } from '../../../lib/fork-i18n/estado'
import './chart-card.css'

const KIND_KEY = { line_chart: 'conversa.cartoes.grafico.tipo.linha', bar_chart: 'conversa.cartoes.grafico.tipo.barras', pie_chart: 'conversa.cartoes.grafico.tipo.pizza' } as const
const plainCell = (text: string) => text

function useWidth() {
  const ref = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    setWidth(el.clientWidth)
    const ro = new ResizeObserver(() => setWidth(el.clientWidth))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  return [ref, width] as const
}

export function ChartCard({ chart }: { chart: Chart }) {
  const id = useId()
  const [table, setTable] = useState(false)
  const [visible, setVisible] = useState(() => chart.series.map(() => true))
  const [focus, setFocus] = useState<number | null>(null)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const HINT = t('conversa.cartoes.grafico.dica')
  const kind = chart.effective
  const kindLabel = t(KIND_KEY[kind])
  const legend = kind !== 'pie_chart' && chart.series.length > 1
  const words: Words = { unavailable: t('conversa.cartoes.grafico.indisponivel'), category: t('conversa.cartoes.grafico.colunaCategoria'), date: t('conversa.cartoes.grafico.colunaData') }
  const said = focus === null ? '' : readout(chart, focus, visible, words.unavailable)
  const count = chart.labels.length

  const onKey = (e: KeyboardEvent) => {
    const at = focus ?? -1
    const next = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? Math.min(count - 1, at + 1)
      : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? Math.max(0, at < 0 ? 0 : at - 1)
      : e.key === 'Home' ? 0 : e.key === 'End' ? count - 1 : e.key === 'Escape' ? null : undefined
    if (next === undefined) return
    e.preventDefault()
    setFocus(next)
  }
  const toggle = (i: number) => setVisible((v) => (v[i] && v.filter(Boolean).length === 1 ? v : v.map((x, j) => (j === i ? !x : x))))

  const { header, rows } = tableData(chart, words)
  return (
    <section className="chart-card idioma-superficie" {...superficieIdioma(idioma)} aria-labelledby={`${id}-t`}>
      <ChartDefs id={id} />
      <div className="chart-head">
        <div className="chart-title">
          <div id={`${id}-t`} className="chart-title-text">{chart.title}</div>
          <div className="chart-context">{contextLine(chart)}</div>
        </div>
        <span className="chart-kind">{kindLabel}</span>
      </div>
      <p id={`${id}-s`} className="chart-summary">{chart.summary}</p>
      {chart.requested !== kind ? <p className="chart-note">{t('conversa.cartoes.grafico.pizzaEmBarras')}</p> : null}

      {table ? (
        <div id={`${id}-d`} className="chart-data">
          <DataTable header={header} rows={rows} renderCell={plainCell} plain parseNumber={(cell) => parseExact(cell, words.unavailable)} />
          <p className="chart-data-note">{t('conversa.cartoes.grafico.mesmosNumeros', { unidade: chart.unit })}</p>
        </div>
      ) : (
        <div id={`${id}-d`}>
          <div className="chart-readout" aria-hidden>{said || HINT}</div>
          <div className="chart-sr" role="status">{said}</div>
          <div
            className="chart-plot"
            tabIndex={0}
            role="group"
            aria-roledescription={t('conversa.cartoes.grafico.papel')}
            aria-label={t('conversa.cartoes.grafico.aria', { tipo: kindLabel, titulo: chart.title })}
            aria-describedby={`${id}-s ${id}-h`}
            onKeyDown={onKey}
            onBlur={() => setFocus(null)}
          >
            <span id={`${id}-h`} hidden>{HINT}</span>
            {kind === 'line_chart' ? <LineChart chart={chart} visible={visible} focus={focus} setFocus={setFocus} />
              : kind === 'bar_chart' ? <BarChart chart={chart} id={id} visible={visible} focus={focus} setFocus={setFocus} />
              : <PieChart chart={chart} focus={focus} setFocus={setFocus} />}
          </div>
          {legend ? (
            <div className="chart-legend">
              {chart.series.map((s, i) => (
                <button
                  key={s.id}
                  type="button"
                  className="chart-legend-item"
                  aria-pressed={visible[i]}
                  aria-disabled={visible[i] && visible.filter(Boolean).length === 1}
                  aria-label={t(visible[i] ? 'conversa.cartoes.grafico.ocultarSerieAria' : 'conversa.cartoes.grafico.mostrarSerieAria', { serie: s.label })}
                  title={t(visible[i] ? (visible.filter(Boolean).length === 1 ? 'conversa.cartoes.grafico.umaVisivel' : 'conversa.cartoes.grafico.ocultarSerie') : 'conversa.cartoes.grafico.mostrarSerie')}
                  onClick={() => toggle(i)}
                >
                  <Swatch kind={kind} index={i} id={id} />
                  <span className="chart-legend-label">{s.label}</span>
                </button>
              ))}
            </div>
          ) : null}
        </div>
      )}

      <div className="chart-foot">
        <span className="chart-origin">
          <Icon name="info.circle" size={12} />
          <span><b>{t('conversa.cartoes.grafico.informado')}</b> · {t('conversa.cartoes.grafico.fonte', { fonte: chart.sourceLabel })}</span>
        </span>
        <button type="button" className="chart-toggle" aria-expanded={table} aria-controls={`${id}-d`} onClick={() => { setFocus(null); setTable((v) => !v) }}>
          <Icon name={table ? 'chart.bar' : 'list.bullet.rectangle'} size={13} />
          {t(table ? 'conversa.cartoes.grafico.verGrafico' : 'conversa.cartoes.grafico.verDados')}
        </button>
      </div>
    </section>
  )
}

/** Patterns so bar series differ by more than color: solid, stripes, dots. */
function ChartDefs({ id }: { id: string }) {
  return (
    <svg className="chart-defs" width="0" height="0" aria-hidden focusable="false">
      <defs>
        <pattern id={`${id}-p1`} width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <rect className="s1" width="6" height="6" />
          <rect className="gap" width="2" height="6" />
        </pattern>
        <pattern id={`${id}-p2`} width="5" height="5" patternUnits="userSpaceOnUse">
          <rect className="s2" width="5" height="5" />
          <circle className="gap" cx="2.5" cy="2.5" r="1.1" />
        </pattern>
      </defs>
    </svg>
  )
}
const barFill = (id: string, i: number) => (i === 0 ? undefined : `url(#${id}-p${i})`)

const DASH = [undefined, '6 4', '2 3']
function Marker({ series, x, y, r = 3.5, className }: { series: number; x: number; y: number; r?: number; className?: string }) {
  const c = `chart-mark s${series}${className ? ` ${className}` : ''}`
  if (series === 1) return <rect className={c} x={x - r} y={y - r} width={r * 2} height={r * 2} />
  if (series === 2) return <path className={c} d={`M${x} ${y - r * 1.3}L${x + r * 1.3} ${y}L${x} ${y + r * 1.3}L${x - r * 1.3} ${y}Z`} />
  return <circle className={c} cx={x} cy={y} r={r} />
}

function Swatch({ kind, index, id }: { kind: Chart['effective']; index: number; id: string }) {
  return (
    <svg className="chart-swatch" width="22" height="10" aria-hidden focusable="false">
      {kind === 'line_chart' ? (
        <>
          <line className={`chart-line s${index}`} x1="1" x2="21" y1="5" y2="5" strokeDasharray={DASH[index]} />
          <Marker series={index} x={11} y={5} r={3} />
        </>
      ) : <rect className={`chart-bar s${index}`} x="3" y="1" width="16" height="8" rx="2" fill={barFill(id, index)} />}
    </svg>
  )
}

type PlotProps = { chart: Chart; visible: boolean[]; focus: number | null; setFocus: (i: number | null) => void }

function Empty({ width, height, children }: { width: number; height: number; children: ReactNode }) {
  return <svg width={width} height={height} aria-hidden><text className="chart-empty" x={width / 2} y={height / 2} textAnchor="middle">{children}</text></svg>
}

function domain(chart: Chart, visible: boolean[]) {
  let lo = Infinity
  let hi = -Infinity
  chart.series.forEach((s, i) => { if (visible[i]) for (const v of s.values) if (v !== null) { lo = Math.min(lo, v); hi = Math.max(hi, v) } })
  return lo === Infinity ? null : { lo, hi }
}

function LineChart({ chart, visible, focus, setFocus }: PlotProps) {
  const t = useT()
  const [ref, width] = useWidth()
  const height = 200
  const d = domain(chart, visible)
  const hasValues = d !== null
  const scale = niceScale(d?.lo ?? 0, d?.hi ?? 1)
  const ticks = scale.ticks.map((t) => formatTick(t, scale))
  const count = chart.labels.length
  // Direct labels at the end of each series when they fit; the legend always names them too.
  const ends = chart.series.flatMap((s, i) => {
    const v = s.values[count - 1]
    return visible[i] && v !== null && v !== undefined ? [{ i, v, text: `${s.label} ${formatExact(v)}` }] : []
  })
  const yProbe = useRef<SVGTextElement>(null)
  const endProbe = useRef<SVGTextElement>(null)
  const axisProbe = useRef<SVGTextElement>(null)
  const [measuredLayout, setLayout] = useState<{ chart: Chart; visible: boolean[]; width: number; left: number; innerW: number; direct: boolean; xTicks: AxisTick[] } | null>(null)
  const layout = measuredLayout?.chart === chart && measuredLayout.visible === visible && measuredLayout.width === width ? measuredLayout : null
  useLayoutEffect(() => {
    if (!width || !hasValues || !yProbe.current || !endProbe.current || !axisProbe.current) return
    const yText = svgTextReader(yProbe.current)
    const endText = svgTextReader(endProbe.current)
    const xText = svgTextReader(axisProbe.current)
    const left = Math.max(...ticks.map((label) => yText(label).left)) + 8
    const endBounds = ends.map((e) => endText(e.text))
    const endW = Math.max(0, ...endBounds.map((b) => b.left + b.right))
    let direct = ends.length > 0 && endW <= Math.min(150, width * 0.3)
    let right = direct ? Math.max(0, ...endBounds.map((b) => b.right)) + 10 : 10
    let innerW = Math.max(40, width - left - right)
    if (direct && endBounds.some((b) => left + innerW + 8 - b.left < 2 || left + innerW + 8 + b.right > width - 2)) {
      direct = false
      right = 10
      innerW = Math.max(40, width - left - right)
    }
    // Same native SVG measurement/margin as the R2 fallback, now independent of canvas availability.
    setLayout({ chart, visible, width, left, innerW, direct, xTicks: lineAxisTicks(chart.labels, width, left, innerW, xText, 2) })
  }, [chart, visible, width, hasValues])
  const top = 10
  const bottom = 22
  const left = layout?.left ?? 0
  const innerW = layout?.innerW ?? 0
  const innerH = height - top - bottom
  const x = (i: number) => left + (count === 1 ? innerW / 2 : (i / (count - 1)) * innerW)
  const y = (v: number) => top + (1 - ratio(scale, v)) * innerH
  const markers = count <= 60
  const onMove = (e: PointerEvent<SVGSVGElement>) => {
    const box = e.currentTarget.getBoundingClientRect()
    const at = count === 1 ? 0 : Math.round(((e.clientX - box.left - left) / innerW) * (count - 1))
    setFocus(Math.min(count - 1, Math.max(0, at)))
  }

  // Push end labels apart vertically.
  const placed = layout?.direct ? ends.map((e) => ({ ...e, y: y(e.v) })).sort((a, b) => a.y - b.y) : []
  for (let k = 1; k < placed.length; k++) placed[k]!.y = Math.max(placed[k]!.y, placed[k - 1]!.y + 13)
  const overflow = Math.max(0, (placed.at(-1)?.y ?? 0) - (top + innerH - 4))
  for (const label of placed) label.y -= overflow

  return (
    <div ref={ref} className="chart-svg-box">
      {width === 0 ? null : !d ? (
        <Empty width={width} height={height}>{t(visible.every(Boolean) ? 'conversa.cartoes.grafico.semValores' : 'conversa.cartoes.grafico.semValoresVisiveis')}</Empty>
      ) : (
        <svg width={width} height={height} aria-hidden focusable="false" onPointerMove={onMove} onPointerLeave={() => setFocus(null)}>
          <g visibility="hidden" pointerEvents="none">
            <text ref={yProbe} x={0} y={0} textAnchor="end" />
            <text ref={endProbe} className="chart-end" x={0} y={0} textAnchor="start" />
            <text ref={axisProbe} x={0} y={0} textAnchor="middle" />
          </g>
          {layout ? <>
            {scale.ticks.map((t, k) => (
              <g key={k}>
                <line className={t === 0 ? 'chart-base' : 'chart-grid'} x1={left} x2={left + innerW} y1={y(t)} y2={y(t)} />
                <text x={left - 6} y={y(t) + 4} textAnchor="end">{ticks[k]}</text>
              </g>
            ))}
            {layout.xTicks.map((tick) => <text key={tick.i} x={tick.x} y={height - 6} textAnchor="middle">{tick.label}</text>)}
            {focus !== null ? <line className="chart-guide" x1={x(focus)} x2={x(focus)} y1={top} y2={top + innerH} /> : null}
            {chart.series.map((s, si) => visible[si] ? (
              <g key={s.id}>
                {segments(s.values).map((run) => run.length > 1 ? (
                  <path key={run[0]} className={`chart-line s${si}`} strokeDasharray={DASH[si]} d={run.map((i, k) => `${k ? 'L' : 'M'}${x(i).toFixed(2)} ${y(s.values[i]!).toFixed(2)}`).join('')} />
                ) : null)}
                {segments(s.values).flatMap((run) => (markers || run.length === 1 ? run : [])).map((i) => <Marker key={i} series={si} x={x(i)} y={y(s.values[i]!)} />)}
                {focus !== null && s.values[focus] !== null ? <Marker series={si} x={x(focus)} y={y(s.values[focus]!)} r={5} className="focus" /> : null}
              </g>
            ) : null)}
            {placed.map((e) => (
              <text key={e.i} className="chart-end" x={left + innerW + 8} y={e.y + 4}>{e.text}</text>
            ))}
          </> : null}
        </svg>
      )}
    </div>
  )
}

function BarChart({ chart, id, visible, focus, setFocus }: PlotProps & { id: string }) {
  const t = useT()
  const [ref, width] = useWidth()
  const scroll = useRef<HTMLDivElement>(null)
  const shown = useMemo(() => chart.series.map((s, i) => ({ s, i })).filter(({ i }) => visible[i]), [chart, visible])
  const d = domain(chart, visible)
  const scale = niceScale(d?.lo ?? 0, d?.hi ?? 1)
  const unavailable = t('conversa.cartoes.grafico.indisponivel')
  const valueText = (v: number | null) => (v === null ? unavailable : formatExact(v))
  const k = shown.length
  const barH = k === 1 ? 14 : 10
  const rowH = k * barH + (k - 1) * 2 + 12
  const count = chart.labels.length
  const height = count * rowH
  const axis = scale.ticks.map((t) => ({ t, text: formatTick(t, scale) }))
  const categoryProbe = useRef<SVGTextElement>(null)
  const categoryFocusProbe = useRef<SVGTextElement>(null)
  const valueStartProbe = useRef<SVGTextElement>(null)
  const valueStartFocusProbe = useRef<SVGTextElement>(null)
  const valueEndProbe = useRef<SVGTextElement>(null)
  const valueEndFocusProbe = useRef<SVGTextElement>(null)
  const naProbe = useRef<SVGTextElement>(null)
  const axisProbe = useRef<SVGTextElement>(null)
  const [measuredLayout, setLayout] = useState<{ chart: Chart; shown: typeof shown; width: number; categoryW: number; categories: { label: string; x: number }[]; valueW: number; axis: AxisTextBounds[] } | null>(null)
  const layout = measuredLayout?.chart === chart && measuredLayout.shown === shown && measuredLayout.width === width ? measuredLayout : null
  const hasValues = d !== null
  useLayoutEffect(() => {
    if (!width || !hasValues || !categoryProbe.current || !categoryFocusProbe.current || !valueStartProbe.current
      || !valueStartFocusProbe.current || !valueEndProbe.current || !valueEndFocusProbe.current || !naProbe.current || !axisProbe.current) return
    const category = textEnvelope(svgTextReader(categoryProbe.current), svgTextReader(categoryFocusProbe.current))
    const valueStart = textEnvelope(svgTextReader(valueStartProbe.current), svgTextReader(valueStartFocusProbe.current))
    const valueEnd = textEnvelope(svgTextReader(valueEndProbe.current), svgTextReader(valueEndFocusProbe.current))
    const na = svgTextReader(naProbe.current)
    const axisText = svgTextReader(axisProbe.current)
    const categoryW = Math.min(Math.max(...chart.labels.map((s) => { const b = category(s); return b.left + b.right })) + 4, width * 0.34)
    const categories = chart.labels.map((s) => {
      const label = fitMeasuredText(s, categoryW - 4, category)
      return { label, x: categoryW - 2 - category(label).right }
    })
    const valueW = Math.max(0, ...shown.flatMap(({ s }) => s.values.map((v) => {
      const label = valueText(v)
      return v === null ? na(label).right : Math.max(valueStart(label).right, valueEnd(label).left)
    }))) + 8
    setLayout({ chart, shown, width, categoryW, categories, valueW, axis: axis.map(({ text }) => axisText(text)) })
  }, [chart, shown, width, hasValues])
  const neg = (d?.lo ?? 0) < 0
  const valueW = layout?.valueW ?? 0
  const left = Math.max((layout?.categoryW ?? 0) + 12 + (neg ? valueW : 0), Math.max(0, ...(layout?.axis.map((b) => b.left) ?? [])) + 2)
  const right = Math.max(valueW, Math.max(0, ...(layout?.axis.map((b) => b.right) ?? [])) + 2)
  const minPlot = Math.max(100, (layout?.axis[0]?.right ?? 0) + (layout?.axis.at(-1)?.left ?? 0) + 14)
  const plotWidth = Math.max(width - 16, left + right + minPlot)
  const innerW = plotWidth - left - right
  const x = (v: number) => left + ratio(scale, v) * innerW
  const zero = x(0)
  const axisTicks = layout ? fullAxisTicks(axis.map(({ t, text }, i) => ({ i, label: text, x: x(t), ...layout.axis[i]! }))) : []

  // Keep the focused row in view when the bars scroll inside the card.
  useEffect(() => {
    const el = scroll.current
    if (!el || focus === null) return
    const top = focus * rowH
    if (top < el.scrollTop) el.scrollTop = top
    else if (top + rowH > el.scrollTop + el.clientHeight) el.scrollTop = top + rowH - el.clientHeight
  }, [focus, rowH])

  const onMove = (e: PointerEvent<SVGSVGElement>) => {
    const box = e.currentTarget.getBoundingClientRect()
    setFocus(Math.min(count - 1, Math.max(0, Math.floor((e.clientY - box.top) / rowH))))
  }

  return (
    <div ref={ref} className="chart-svg-box">
      {width === 0 ? null : !d ? (
        <Empty width={width} height={80}>{t(visible.every(Boolean) ? 'conversa.cartoes.grafico.semValores' : 'conversa.cartoes.grafico.semValoresVisiveis')}</Empty>
      ) : (
        <div className="chart-bars-width">
          <div style={{ width: plotWidth + 16 }}>
            <div ref={scroll} className="chart-bars-scroll">
              <svg width={plotWidth} height={height} aria-hidden focusable="false" onPointerMove={onMove} onPointerLeave={() => setFocus(null)}>
                <g visibility="hidden" pointerEvents="none">
                  <text ref={categoryProbe} className="chart-cat" x={0} y={0} textAnchor="end" />
                  <text ref={valueStartProbe} className="chart-value" x={0} y={0} textAnchor="start" />
                  <text ref={valueEndProbe} className="chart-value" x={0} y={0} textAnchor="end" />
                  <text ref={naProbe} className="chart-na" x={0} y={0} textAnchor="start" />
                  <text ref={axisProbe} x={0} y={0} textAnchor="middle" />
                  <g className="is-focus">
                    <text ref={categoryFocusProbe} className="chart-cat" x={0} y={0} textAnchor="end" />
                    <text ref={valueStartFocusProbe} className="chart-value" x={0} y={0} textAnchor="start" />
                    <text ref={valueEndFocusProbe} className="chart-value" x={0} y={0} textAnchor="end" />
                  </g>
                </g>
                {layout ? <>
                  {focus !== null ? <rect className="chart-row-focus" x={0} y={focus * rowH} width={plotWidth} height={rowH} rx={4} /> : null}
                  {axis.map(({ t }) => <line key={t} className={t === 0 ? 'chart-base' : 'chart-grid'} x1={x(t)} x2={x(t)} y1={0} y2={height} />)}
                  {chart.labels.map((_, r) => {
                    const y0 = r * rowH + 6
                    return (
                      <g key={r} className={focus === r ? 'is-focus' : undefined}>
                        <text className="chart-cat" x={layout.categories[r]!.x} y={r * rowH + rowH / 2 + 4} textAnchor="end">{layout.categories[r]!.label}</text>
                        {shown.map(({ s, i }, b) => {
                          const v = s.values[r] ?? null
                          const by = y0 + b * (barH + 2)
                          if (v === null) return <text key={s.id} className="chart-na" x={zero + 4} y={by + barH - 2}>{unavailable}</text>
                          const xv = x(v)
                          const w = Math.abs(xv - zero)
                          return (
                            <g key={s.id}>
                              {w > 0 ? <rect className={`chart-bar s${i}`} x={Math.min(xv, zero)} y={by} width={w} height={barH} rx={Math.min(3, w / 2)} fill={barFill(id, i)} /> : null}
                              <text className="chart-value" x={v < 0 ? xv - 4 : xv + 4} y={by + barH - 2} textAnchor={v < 0 ? 'end' : 'start'}>{valueText(v)}</text>
                            </g>
                          )
                        })}
                      </g>
                    )
                  })}
                </> : null}
              </svg>
            </div>
            <svg className="chart-axis" width={plotWidth} height={18} aria-hidden focusable="false">
              {axisTicks.map((tick) => <text key={tick.i} x={tick.x} y={12} textAnchor="middle">{tick.label}</text>)}
            </svg>
          </div>
        </div>
      )}
    </div>
  )
}

function PieChart({ chart, focus, setFocus }: Omit<PlotProps, 'visible'>) {
  const t = useT()
  const values = chart.series[0]!.values.map((v) => v ?? 0)
  const total = pieTotal(chart)
  // Keep every significant digit of the computed total, including MAX_SAFE.
  const totalText = formatExact(total)
  const size = 152
  const c = size / 2
  const r = 68
  const point = (a: number, rr = r) => [c + rr * Math.sin(a), c - rr * Math.cos(a)] as const
  const slices = pieSlices(values)
  return (
    <div className={`chart-pie${Math.max(totalText.length, ...values.map((v) => formatExact(v).length)) > 16 ? ' chart-pie-wide' : ''}`}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden focusable="false" onPointerLeave={() => setFocus(null)}>
        {slices.map((s) => {
          const mid = (s.start + s.end) / 2
          const off = focus === s.index ? 4 : 0
          const t = `translate(${(off * Math.sin(mid)).toFixed(2)} ${(-off * Math.cos(mid)).toFixed(2)})`
          const cls = `chart-slice c${s.index}`
          if (s.end - s.start >= 2 * Math.PI - 1e-9) return <circle key={s.index} className={cls} transform={t} cx={c} cy={c} r={r} onPointerEnter={() => setFocus(s.index)} />
          const [x1, y1] = point(s.start)
          const [x2, y2] = point(s.end)
          const large = s.end - s.start > Math.PI ? 1 : 0
          return <path key={s.index} className={cls} transform={t} d={`M${c} ${c}L${x1} ${y1}A${r} ${r} 0 ${large} 1 ${x2} ${y2}Z`} onPointerEnter={() => setFocus(s.index)} />
        })}
      </svg>
      <div className="chart-pie-list">
        {chart.labels.map((label, i) => (
          <div key={i} className={`chart-pie-row${focus === i ? ' is-focus' : ''}`} onPointerEnter={() => setFocus(i)}>
            <span className={`chart-dot c${i}`} aria-hidden />
            <span className="chart-pie-name">{label}</span>
            <span className="chart-pie-num">{formatExact(values[i]!)}</span>
            <span className="chart-pie-num">{percentText(values[i]!, total, values.some((v, j) => j !== i && v > 0))}</span>
          </div>
        ))}
        <div className="chart-pie-row total">
          <span className="chart-dot" aria-hidden />
          <span className="chart-pie-name">{t('conversa.cartoes.grafico.total')}</span>
          <span className="chart-pie-num">{totalText}</span>
          <span className="chart-pie-num">100%</span>
        </div>
      </div>
    </div>
  )
}
