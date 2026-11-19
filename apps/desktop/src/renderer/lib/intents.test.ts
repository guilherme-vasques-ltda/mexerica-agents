import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Intents } from './intents.ts'

// D165 (revisão A, F1/F5): só a intenção mais recente escreve.
test('uma intenção nova torna as anteriores obsoletas', () => {
  const intents = new Intents()
  const first = intents.begin()
  assert.ok(intents.current(first))
  const second = intents.begin()
  assert.equal(intents.current(first), false)
  assert.ok(intents.current(second))
})
