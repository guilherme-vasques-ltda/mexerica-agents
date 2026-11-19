import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fitMeasuredText, fullAxisTicks, lineAxisTicks, type AxisTextBounds } from './chart-axis.ts'

// Regression boundaries use unequal ink extents, as getBBox can report around text-anchor=middle.
const ink = (s: string): AxisTextBounds => ({ left: [...s].length * 5 + 1.5, right: [...s].length * 4 - 1.5 })
const assertContained = (width: number, labels: readonly string[]) => {
  const ticks = lineAxisTicks(labels, width, 30, width - 100, ink, 2)
  assert.equal(ticks[0]!.i, 0)
  assert.equal(ticks.at(-1)!.i, labels.length - 1)
  for (const t of ticks) {
    const b = ink(t.label)
    assert.ok(t.x - b.left >= 2 && t.x + b.right <= width - 2)
  }
  for (let i = 1; i < ticks.length; i++) assert.ok(ticks[i]!.x - ticks[i]!.left > ticks[i - 1]!.x + ticks[i - 1]!.right)
  for (const t of ticks.slice(1, -1)) assert.equal(t.label, labels[t.i])
  return ticks
}

test('extremos de 159 caracteres e ponto único usam limites medidos, com margem', () => {
  for (const width of [268, 348, 508]) {
    const labels = ['Início ' + 'A'.repeat(152), 'Fim ' + 'Z'.repeat(155)]
    const ticks = assertContained(width, labels)
    assert.equal(ticks.length, 2)
    assert.ok(ticks.every((t) => t.label.endsWith('…')))
    assert.equal(assertContained(width, [labels[0]!]).length, 1)
    assert.equal(labels[0]!.length, 159)
  }
})

test('500 pontos conservam índices extremos e intermediários completos após resize', () => {
  const labels = Array.from({ length: 500 }, (_, i) => `2026-${String(i).padStart(5, '0')}`)
  for (const width of [508, 348, 268, 508]) assertContained(width, labels)
  assert.equal(labels.length, 500)
})


test('categoria larga mantém prefixo distinto e respeita limite de tinta normal/foco', () => {
  for (const budget of [72, 100, 180]) {
    const a = fitMeasuredText('A' + 'W'.repeat(158), budget, ink)
    const b = fitMeasuredText('B' + 'W'.repeat(158), budget, ink)
    assert.ok(a.startsWith('A') && b.startsWith('B') && a !== b)
    assert.ok(ink(a).left + ink(a).right <= budget)
    assert.ok(ink(b).left + ink(b).right <= budget)
  }
  assert.equal(fitMeasuredText('W', 1, ink), '')
})

test('ticks numéricos completos reservam extremos sem sobrepor intermediários', () => {
  const ticks = [0, 1, 2, 3, 4].map((i) => ({ i, label: String(i), x: 20 + i * 20, left: 12, right: 12 }))
  const selected = fullAxisTicks(ticks)
  assert.deepEqual(selected.map((t) => t.i), [0, 2, 4])
  assert.ok(selected.every((t) => t.label === ticks[t.i]!.label))
})
