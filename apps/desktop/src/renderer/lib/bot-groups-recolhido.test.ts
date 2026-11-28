import assert from 'node:assert/strict'
import test from 'node:test'
import { stayVisibleWhenClosed } from './bot-groups.ts'

test('grupo recolhido mostra a conversa aberta e as que esperam o dono', () => {
  assert.equal(stayVisibleWhenClosed({ status: 'idle' }, true), true)
  assert.equal(stayVisibleWhenClosed({ status: 'needsInput' }, false), true)
})

test('grupo recolhido esconde as demais', () => {
  for (const status of ['idle', 'working', 'waiting', 'error', 'unavailable']) assert.equal(stayVisibleWhenClosed({ status }, false), false)
})
