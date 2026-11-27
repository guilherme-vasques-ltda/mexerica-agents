import assert from 'node:assert/strict'
import test from 'node:test'
import { tabsStartExpanded } from './inicio.ts'

test('sem sessões e sem escolha salva, a barra nasce retraída', () => {
  assert.equal(tabsStartExpanded(0, null), false)
})

test('com sessões e sem escolha salva, a barra nasce expandida', () => {
  assert.equal(tabsStartExpanded(2, null), true)
})

test('a escolha salva vence, com ou sem sessões', () => {
  assert.equal(tabsStartExpanded(0, true), true)
  assert.equal(tabsStartExpanded(3, false), false)
})
