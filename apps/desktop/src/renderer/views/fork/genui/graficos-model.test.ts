// D70: the renderer's defensive mirror of the v1 chart contract and the numbers it draws. No bot, wire or app.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  formatExact, formatTick, MAX_VALUE, niceScale, parseChart, parseChartSnapshot, parseExact, percentText, pieTotal, pieSlices, ratio, readout, segments, tableData,
  UNAVAILABLE, validDate, type Chart,
} from './graficos-model.ts'
import { cellLines, numericColumns, toCSV, toTSV, visibleRows } from '../../thread/cards/data-table.ts'

type Json = Record<string, any>
function payload(component: string, count = 3, values?: (number | null)[]): Json {
  const p: Json = {
    version: 1, component, title: 'Quantidade informada', summary: 'Exemplo sintético.',
    data: {
      labels: Array.from({ length: count }, (_, i) => `Categoria ${i}`),
      series: [{ id: 'volume', label: 'Quantidade', values: values ?? Array.from({ length: count }, (_, i) => i + 1) }],
      unit: 'itens', period: 'Novembro de 2026', sourceLabel: 'Conversa informada',
    },
  }
  if (component === 'line_chart') p.data.xKind = 'category'
  return p
}
const ok = (p: unknown): Chart => {
  const r = parseChart(typeof p === 'string' ? p : JSON.stringify(p))
  assert.ok(r.ok, r.ok ? '' : r.code)
  return r.chart
}
const code = (p: unknown) => {
  const r = parseChart(typeof p === 'string' ? p : JSON.stringify(p))
  assert.ok(!r.ok)
  return r.code
}

test('catálogo fechado: peças, versão, campos extras e xKind só na linha', () => {
  for (const c of ['line_chart', 'bar_chart', 'pie_chart']) assert.equal(ok(payload(c)).requested, c)
  for (const c of ['choice', 'table', 'svg', 'html']) assert.equal(code(payload(c)), 'genui_chart_invalid_payload')
  for (const version of [0, 2]) assert.equal(code({ ...payload('bar_chart'), version }), 'genui_chart_unsupported_version')
  for (const version of [1.5, '1', null, 256]) assert.equal(code({ ...payload('bar_chart'), version }), 'genui_chart_invalid_payload')
  const extra = payload('bar_chart'); extra.data.series[0].color = 'red'
  assert.equal(code(extra), 'genui_chart_invalid_payload')
  for (const key of ['verified', 'sourceRefs', '__proto__']) {
    const top = JSON.stringify(payload('bar_chart')).replace(/^\{/, `{"${key}":true,`)
    assert.equal(code(top), 'genui_chart_invalid_payload')
  }
  const line = payload('line_chart'); delete line.data.xKind
  assert.equal(code(line), 'genui_chart_invalid_x_kind')
  const bar = payload('bar_chart'); bar.data.xKind = 'category'
  assert.equal(code(bar), 'genui_chart_invalid_x_kind')
  bar.data.xKind = null
  assert.equal(code(bar), 'genui_chart_invalid_payload')
  assert.equal(code('{'), 'genui_chart_invalid_payload')
  assert.equal(code(JSON.stringify({ ...payload('bar_chart'), summary: 'x'.repeat(70 * 1024) })), 'genui_chart_too_large')
})

test('JSON bruto fechado: duplicidade em todo nível e chave escapada, no payload e snapshot', () => {
  const p = payload('bar_chart')
  const raw = JSON.stringify(p)
  const duplicatePayloads = [
    raw.replace('"version":1', '"version":1,"version":1'),
    raw.replace('"version":1', String.raw`"version":1,"ver\u0073ion":1`),
    raw.replace('"data":', '"data":{},"data":'),
    raw.replace('"labels":', '"labels":[],"labels":'),
    raw.replace('"labels":', String.raw`"lab\u0065ls":[],"labels":`),
    raw.replace('"id":', '"id":"outro","id":'),
    raw.replace('"values":', String.raw`"val\u0075es":[],"values":`),
  ]
  const snapshot = (body: string) => `{"requested":"bar_chart","effective":"bar_chart","payload":${body}}`
  const refused = (result: ReturnType<typeof parseChart>) => assert.deepEqual(result, { ok: false, code: 'genui_chart_invalid_payload' })
  for (const body of duplicatePayloads) {
    refused(parseChart(body))
    refused(parseChartSnapshot(snapshot(body)))
  }
  for (const body of [
    snapshot(raw).replace('"requested":', '"requested":"bar_chart","requested":'),
    snapshot(raw).replace('"effective":', String.raw`"effect\u0069ve":"bar_chart","effective":`),
    snapshot(raw).replace('"payload":', '"payload":{},"payload":'),
  ]) refused(parseChartSnapshot(body))

  // Fields repeated in different objects and JSON-looking strings remain ordinary JSON.
  p.summary = 'Texto com {"id": "a", "id": "b"}, [colchetes], \\" e 🌙'
  p.data.series.push({ id: 'outra', label: 'Quantidade', values: [3, 2, 1] })
  const normal = ` \n${JSON.stringify(p)}\t `.replace('"version":1', String.raw`"ver\u0073ion":1`)
  assert.ok(parseChart(normal).ok)
  assert.ok(parseChartSnapshot(snapshot(normal)).ok)
  for (const body of [raw, duplicatePayloads[1]!]) {
    assert.deepEqual(parseChart(body + ' '.repeat(65 * 1024)), { ok: false, code: 'genui_chart_too_large' })
    assert.deepEqual(parseChartSnapshot(snapshot(body) + ' '.repeat(65 * 1024)), { ok: false, code: 'genui_chart_too_large' })
  }
})

test('version exige token uint no bruto; valores dos dados mantêm fração/expoente/MAX .0', () => {
  const raw = JSON.stringify(payload('bar_chart', 1, [0]))
  const snapshot = (body: string) => `{"requested":"bar_chart","effective":"bar_chart","payload":${body}}`
  for (const token of ['1.0', '1e0', '1E+0', '1e-0', '-0', '"1"', 'null']) {
    for (const name of ['version', String.raw`ver\u0073ion`]) {
      const body = raw.replace('"version":1', `"${name}":${token}`)
      assert.deepEqual(parseChart(body), { ok: false, code: 'genui_chart_invalid_payload' })
      assert.deepEqual(parseChartSnapshot(snapshot(body)), { ok: false, code: 'genui_chart_invalid_payload' })
    }
  }
  for (const number of ['0.1', '1e-9', '9007199254740991.0']) {
    const body = raw.replace('[0]', `[${number}]`)
    assert.ok(parseChart(body).ok)
    assert.ok(parseChartSnapshot(snapshot(body)).ok)
  }
})

test('textos, ids, matriz e limites iguais ao Hub', () => {
  assert.equal(code({ ...payload('bar_chart'), title: '   ' }), 'genui_chart_invalid_text')
  assert.equal(code({ ...payload('bar_chart'), title: 'a\u0007b' }), 'genui_chart_invalid_text')
  assert.equal(code({ ...payload('bar_chart'), title: 'é'.repeat(161) }), 'genui_chart_invalid_text')
  ok({ ...payload('bar_chart'), title: 'é'.repeat(160), summary: 'linha 1\nlinha\t2' })
  assert.equal(code(payload('bar_chart', 0)), 'genui_chart_invalid_labels')
  assert.equal(code(payload('bar_chart', 501)), 'genui_chart_invalid_labels')
  ok(payload('bar_chart', 500))
  const ids = payload('bar_chart'); ids.data.series[0].id = 'tem espaço'
  assert.equal(code(ids), 'genui_chart_invalid_series_id')
  const dup = payload('bar_chart'); dup.data.series.push({ ...dup.data.series[0] })
  assert.equal(code(dup), 'genui_chart_duplicate_series_id')
  const four = payload('line_chart'); four.data.series = ['a', 'b', 'c', 'd'].map((id) => ({ id, label: id, values: [1, 2, 3] }))
  assert.equal(code(four), 'genui_chart_invalid_series')
  assert.equal(code(payload('bar_chart', 3, [1, 2])), 'genui_chart_irregular_matrix')
  assert.equal(code(JSON.stringify(payload('bar_chart', 1, [0])).replace('[0]', '[1e400]')), 'genui_chart_invalid_number')
  assert.equal(code(payload('bar_chart', 1, [MAX_VALUE + 2])), 'genui_chart_invalid_number')
  // The decimal spelling of 2^53 − 1 keeps its exact value in JS; it is accepted, not rounded down.
  const max = ok(JSON.stringify(payload('bar_chart', 1, [0])).replace('[0]', '[9007199254740991.0]'))
  assert.equal(max.series[0]!.values[0], MAX_VALUE)
  ok(payload('bar_chart', 2, [-MAX_VALUE, MAX_VALUE]))
})

test('datas reais com fuso explícito; o label não é reescrito', () => {
  for (const d of ['2026-11-04', '2026-11-04T10:00:00Z', '2026-11-04T10:00:00.5-03:00', '2024-02-29', '0000-02-29', '0099-12-31']) assert.ok(validDate(d), d)
  for (const d of ['2026-02-30', '2026-11-04T10:00:00', '2026-11-04T10:00:00-00:00', '2026-11-04 10:00:00Z', '04/11/2026', '2026-11-04T25:00:00Z']) assert.ok(!validDate(d), d)
  const p = payload('line_chart', 2); p.data.xKind = 'date'; p.data.labels = ['2026-11-04T10:00:00-03:00', '2026-11-05']
  assert.deepEqual(ok(p).labels, ['2026-11-04T10:00:00-03:00', '2026-11-05'])
  p.data.labels = ['2026-11-04', 'amanhã']
  assert.equal(code(p), 'genui_chart_invalid_date')
})

test('pizza: completa, não negativa, total seguro; acima de 5 vira barras com todos os dados', () => {
  assert.equal(code(payload('pie_chart', 2, [1, null])), 'genui_chart_incomplete_pie')
  assert.equal(code(payload('pie_chart', 2, [1, -1])), 'genui_chart_invalid_pie')
  assert.equal(code(payload('pie_chart', 2, [0, 0])), 'genui_chart_invalid_pie_total')
  for (const values of [[MAX_VALUE, 0.1], [0.1, MAX_VALUE], [MAX_VALUE - 1, 0.5, 0.5, Number.MIN_VALUE], [Number.MIN_VALUE, 0.5, 0.5, MAX_VALUE - 1]]) {
    assert.equal(code(payload('pie_chart', values.length, values)), 'genui_chart_invalid_pie_total')
    assert.equal(code(payload('pie_chart', 6, [...values, ...Array(6 - values.length).fill(0)])), 'genui_chart_invalid_pie_total')
  }
  for (const values of [[MAX_VALUE, 0], [0, MAX_VALUE], [MAX_VALUE - 1, 0.5, 0.5], [0.5, 0.5, MAX_VALUE - 1], [Number.MIN_VALUE, 0]]) ok(payload('pie_chart', values.length, values))
  const two = payload('pie_chart'); two.data.series.push({ id: 'b', label: 'B', values: [1, 2, 3] })
  assert.equal(code(two), 'genui_chart_invalid_pie')
  assert.equal(ok(payload('pie_chart', 5)).effective, 'pie_chart')
  const eight = ok(payload('pie_chart', 8, [8, 7, 6, 5, 4, 3, 2, 0]))
  assert.equal(eight.requested, 'pie_chart')
  assert.equal(eight.effective, 'bar_chart')
  assert.equal(tableData(eight).rows.length, 8)
  assert.deepEqual(tableData(eight).rows.map((r) => r[1]), ['8', '7', '6', '5', '4', '3', '2', '0'])
  assert.ok(!tableData(eight).rows.some((r) => r[0] === 'Outros'))
  assert.equal(readout(eight, 0, [true]), 'Categoria 0: 8 itens · 22,9%')
  assert.equal(code(payload('pie_chart', 7, [1, 1, 1, 1, 1, 1, null])), 'genui_chart_incomplete_pie')
})

test('snapshot só quando coerente com o payload', () => {
  const p = payload('pie_chart', 6)
  assert.ok(parseChartSnapshot(JSON.stringify({ requested: 'pie_chart', effective: 'bar_chart', payload: p })).ok)
  for (const s of [{ requested: 'bar_chart', effective: 'bar_chart', payload: p }, { requested: 'pie_chart', effective: 'pie_chart', payload: p }]) {
    const r = parseChartSnapshot(JSON.stringify(s))
    assert.ok(!r.ok && r.code === 'genui_chart_invalid_snapshot')
  }
  assert.ok(!parseChartSnapshot(JSON.stringify({ requested: 'pie_chart', effective: 'bar_chart', payload: p, extra: 1 })).ok)
  const chart = ok(p)
  assert.ok([chart, chart.labels, chart.series, chart.series[0], chart.series[0]!.values].every(Object.isFrozen))
})

test('null é Indisponível, nunca zero, e quebra a linha', () => {
  assert.deepEqual(segments([1, null, 0, 2, null, 3]), [[0], [2, 3], [5]])
  assert.deepEqual(segments([null, null]), [])
  const c = ok(payload('line_chart', 3, [2, null, 0]))
  assert.deepEqual(tableData(c).rows.map((r) => r[1]), ['2', UNAVAILABLE, '0'])
  assert.equal(readout(c, 1, [true]), `Categoria 1: ${UNAVAILABLE}`)
  assert.equal(readout(c, 2, [true]), 'Categoria 2: 0 itens')
})

test('escala inclui zero, sem NaN nem altura zero, de minúscula a 2^53 − 1', () => {
  const cases: [number, number][] = [[0, 0], [5, 5], [-3, -3], [-40, 25], [1e-9, 3e-9], [Number.MIN_VALUE, Number.MIN_VALUE], [0, MAX_VALUE], [-MAX_VALUE, MAX_VALUE], [42, 61]]
  for (const [lo, hi] of cases) {
    const s = niceScale(lo, hi)
    assert.ok(s.max > s.min, `${lo}..${hi}`)
    assert.ok(s.min <= Math.min(0, lo) && s.max >= Math.max(0, hi), `${lo}..${hi}`)
    for (const v of [lo, hi, 0]) { const r = ratio(s, v); assert.ok(Number.isFinite(r) && r >= 0 && r <= 1, `${v} in ${lo}..${hi}`) }
    assert.ok(s.ticks.every(Number.isFinite) && s.ticks.length >= 2)
  }
  assert.deepEqual(niceScale(42, 61).ticks, [0, 20, 40, 60, 80])
  assert.deepEqual(niceScale(-40, 25).ticks, [-40, -20, 0, 20, 40])
})

test('números exatos em pt-BR, lidos de volta pela tabela; percentuais sem 0%/100% falsos', () => {
  for (const [n, s] of [[1234.567, '1.234,567'], [-0.125, '-0,125'], [1e-7, '0,0000001'], [MAX_VALUE, '9.007.199.254.740.991'], [0, '0'], [61, '61']] as const) {
    assert.equal(formatExact(n), s)
    assert.equal(parseExact(s), n)
  }
  assert.equal(parseExact(UNAVAILABLE), -Infinity)
  assert.equal(parseExact('12 pedidos'), null)
  assert.equal(percentText(0, 10), '0%')
  assert.equal(percentText(1, 100_000), '<0,1%')
  assert.equal(percentText(99_999, 100_000), '>99,9%')
  assert.equal(percentText(167, 348), '48%')
  assert.equal(percentText(MAX_VALUE - 1, MAX_VALUE - 1, true), '>99,9%')
  assert.equal(formatExact(pieTotal(ok(payload('pie_chart', 2, [MAX_VALUE, 0])))), '9.007.199.254.740.991')
  assert.notEqual(formatTick(Number.MIN_VALUE, niceScale(0, Number.MIN_VALUE)), '0')
  const slices = pieSlices([1, 0, 1])
  assert.deepEqual(slices.map((s) => s.index), [0, 2])
  assert.equal(slices.at(-1)!.end, 2 * Math.PI)
})

test('Ver dados: 500 linhas literais, ordenação exata e <br> sem virar quebra', () => {
  const p = payload('bar_chart', 500, Array.from({ length: 500 }, (_, i) => (i % 7 === 0 ? null : (i - 250) / 8)))
  p.data.labels[0] = '<b>x</b><br>=HYPERLINK("http://exemplo.invalid")'
  const { header, rows } = tableData(ok(p))
  assert.deepEqual(header, ['Categoria', 'Quantidade'])
  assert.equal(rows.length, 500)
  assert.equal(rows[0]![0], '<b>x</b><br>=HYPERLINK("http://exemplo.invalid")')
  assert.deepEqual(cellLines(rows[0]![0]!, true), [rows[0]![0]])
  assert.deepEqual(cellLines('a<br>b'), ['a', 'b'])
  for (const exportTable of [toCSV, toTSV]) {
    assert.ok(exportTable(['H'], [['a\\|b<br>c']], true).includes('a\\|b<br>c'))
    assert.ok(exportTable(['H'], [['a\\|b<br>c']]).includes('a|b<br>c'))
    assert.ok(exportTable(['H'], [['=1+1']], true).includes("'=1+1"))
  }
  const numeric = numericColumns(rows, 2, parseExact)
  assert.deepEqual(numeric, [false, true])
  const sorted = visibleRows(rows, '', { column: 1, dir: 'asc' }, numeric, parseExact).map((r) => parseExact(r[1]!))
  for (let i = 1; i < sorted.length; i++) assert.ok(sorted[i]! >= sorted[i - 1]!)
  // The Markdown heuristic would read 0,125 as 125; the chart's reader keeps it exact.
  assert.equal(parseExact('0,125'), 0.125)
})
