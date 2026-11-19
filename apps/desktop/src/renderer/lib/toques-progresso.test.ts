import { test } from 'node:test'
import assert from 'node:assert/strict'
import { progressoDeterminado } from '../components/toques-mexerica/progresso.ts'

test('D530: gomo usa o valor confirmado, incluindo início e fim, limitado à barra', () => {
  assert.equal(progressoDeterminado(0), 0)
  assert.equal(progressoDeterminado(2 / 3), 2 / 3)
  assert.equal(progressoDeterminado(1), 1)
  assert.equal(progressoDeterminado(-1), 0)
  assert.equal(progressoDeterminado(2), 1)
})

test('D530: indeterminado não recebe gomo nem porcentagem inventada', () => {
  assert.equal(progressoDeterminado(NaN), undefined)
  assert.equal(progressoDeterminado(Infinity), undefined)
  assert.equal(progressoDeterminado(-Infinity), undefined)
})
