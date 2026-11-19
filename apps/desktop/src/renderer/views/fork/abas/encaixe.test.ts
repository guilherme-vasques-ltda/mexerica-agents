import assert from 'node:assert/strict'
import test from 'node:test'
import { fitTabs } from './encaixe.ts'

const opts = { gap: 10, more: 30 }

test('abas: todas cabem, sem "···"', () => {
  assert.deepEqual(fitTabs([100, 120, 80], 400, -1, opts), { shown: [0, 1, 2], cut: null, hidden: 0 })
})

test('abas: entram as que cabem, a próxima cortada ocupa a sobra, o resto vai para "···"', () => {
  // 400 - 30 (···) = 370: 0 (110) e 1 (130) entram; sobram 130; a 2 (200) entra cortada em 130 - 10 - 6.
  const fit = fitTabs([100, 120, 200, 90, 90], 400, -1, opts)
  assert.deepEqual(fit.shown, [0, 1, 2])
  assert.deepEqual(fit.cut, { index: 2, width: 114 })
  assert.equal(fit.hidden, 2)
})

test('abas: a aberta nunca sai, mesmo no fim da lista', () => {
  const fit = fitTabs([150, 150, 150, 150, 150], 400, 4, opts)
  assert.ok(fit.shown.includes(4))
  assert.deepEqual(fit.shown, [0, 4])
  assert.equal(fit.cut, null)
  assert.equal(fit.hidden, 3)
})

test('abas: só corta com sobra de pelo menos 104 px', () => {
  const fit = fitTabs([180, 180, 180], 400, -1, opts)
  assert.deepEqual(fit, { shown: [0, 1], cut: { index: 1, width: 164 }, hidden: 1 })
  const tight = fitTabs([180, 180, 180], 300, -1, opts)
  assert.deepEqual(tight, { shown: [0], cut: null, hidden: 2 })
})
