import assert from 'node:assert/strict'
import { test } from 'node:test'
import { radioArrowTarget, radioTabStop } from './radio-group.ts'

const key = (k: string, mods: Partial<Record<'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey', boolean>> = {}) =>
  ({ key: k, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...mods })

test('Tab lands on the chosen row, or on the first when the choice is missing', () => {
  assert.equal(radioTabStop(['cli', 'api'], 'api'), 1)
  assert.equal(radioTabStop(['cli', 'api'], 'gone'), 0)
  assert.equal(radioTabStop(['cli', 'api'], ''), 0)
  assert.equal(radioTabStop([], 'api'), -1)
})

test('arrows move to the next or previous row and wrap at the ends', () => {
  assert.equal(radioArrowTarget(key('ArrowDown'), 0, 3), 1)
  assert.equal(radioArrowTarget(key('ArrowRight'), 2, 3), 0)
  assert.equal(radioArrowTarget(key('ArrowUp'), 0, 3), 2)
  assert.equal(radioArrowTarget(key('ArrowLeft'), 1, 3), 0)
  assert.equal(radioArrowTarget(key('ArrowDown'), 0, 1), 0)
})

test('other keys, and arrows with a modifier, stay with the view', () => {
  for (const k of ['Home', 'End', 'Tab', 'Enter', ' ']) assert.equal(radioArrowTarget(key(k), 0, 3), null)
  for (const mod of ['altKey', 'ctrlKey', 'metaKey', 'shiftKey'] as const) assert.equal(radioArrowTarget(key('ArrowDown', { [mod]: true }), 0, 3), null)
  assert.equal(radioArrowTarget(key('ArrowDown'), 0, 0), null)
})
