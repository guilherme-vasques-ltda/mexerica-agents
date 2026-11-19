import assert from 'node:assert/strict'
import { test } from 'node:test'
import { trapWrap } from './focus-trap.ts'

test('Tab wraps from the last stop and Shift+Tab from the first; in between the browser moves', () => {
  assert.equal(trapWrap(['before', 'before', 'at'], false, true), 'first')
  assert.equal(trapWrap(['at', 'after', 'after'], true, true), 'last')
  assert.equal(trapWrap(['before', 'at', 'after'], false, true), null)
  assert.equal(trapWrap(['before', 'at', 'after'], true, true), null)
})

test('a focused control outside the Tab order wraps once no stop lies beyond it', () => {
  // A chosen radio stop, then the focused unchosen row (tabindex -1) as the last control in the sheet.
  assert.equal(trapWrap(['before', 'before'], false, true), 'first')
  // The focused card holds every stop: Shift+Tab wraps to the last, Tab goes in.
  assert.equal(trapWrap(['after', 'after'], true, true), 'last')
  assert.equal(trapWrap(['after', 'after'], false, true), null)
})

test('focus outside the modal: Shift+Tab comes in at the last stop; no stops, no wrap', () => {
  assert.equal(trapWrap(['after'], true, false), 'last')
  assert.equal(trapWrap(['after'], false, false), null)
  assert.equal(trapWrap([], false, true), null)
})
