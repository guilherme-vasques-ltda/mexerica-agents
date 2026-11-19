import assert from 'node:assert/strict'
import test from 'node:test'
import { sessionDeletedOf } from './fork-session-delete.ts'

test('D545: tombstone estrito, sem remover principal com mensagem malformada', () => {
  const value = { type: 'sessionDeleted', spaceId: 'pessoal', chatId: 'b', sessionId: 'extra', rev: 9 } as const
  assert.deepEqual(sessionDeletedOf(value), value)
  for (const change of [{ spaceId: '' }, { chatId: '' }, { sessionId: 'b' }, { rev: 0 }, { rev: 1.5 }, { type: 'session' }]) assert.equal(sessionDeletedOf({ ...value, ...change }), null)
})
