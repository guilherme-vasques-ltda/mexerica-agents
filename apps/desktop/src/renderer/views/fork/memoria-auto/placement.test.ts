import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Entry } from '../../../../shared/models.ts'
import { placeMemoryNotices } from './placement.ts'
import { stepsByReply } from '../../thread/cards/tool-steps.ts'

const entry = (id: string, kind: string, turn: number, data: Entry['data'] = {}): Entry => ({
  id, botId: 'chat', kind, turn, data, rev: 1, seq: 1, createdAt: 0, updatedAt: 0,
})
const ids = (entries: Entry[]) => entries.map((entry) => entry.id)

test('recibo atrasado fica sob resposta explícita; pedido seguinte e ferramentas conservam ordem', () => {
  const first = entry('first', 'agent', 1, { final: true, author: 'bot' })
  const tool = entry('tool', 'tool', 2, { title: 'Ler', author: 'bot', status: 'completed' })
  const entries = [entry('human', 'user', 1), first, entry('next', 'user', 2), tool, entry('second', 'agent', 2, { final: true, author: 'bot' }),
    entry('late', 'notice', 1, { memory: { status: 'done', rev: 3, responseIds: ['first'] } })]
  const ordered = placeMemoryNotices(entries)
  assert.deepEqual(ids(ordered), ['human', 'first', 'late', 'next', 'tool', 'second'])
  assert.deepEqual(ids(ordered.filter((e) => !e.data.memory)), ids(entries.filter((e) => !e.data.memory)))
  assert.deepEqual(stepsByReply(ordered), stepsByReply(entries))
})

test('ID ausente, chat alheio, turno alheio ou ID humano não deslocam o aviso', () => {
  for (const target of [entry('target', 'agent', 2), { ...entry('target', 'agent', 1), botId: 'other' }, entry('target', 'user', 1), entry('different', 'agent', 1)]) {
    const entries = [target, entry('next', 'user', 3), entry('late', 'notice', 1, { memory: { status: 'done', rev: 1, responseIds: ['target'] } })]
    assert.equal(placeMemoryNotices(entries), entries)
  }
})

test('legado exige última resposta final do mesmo chat/turno; lista explícita vazia não usa fallback', () => {
  const first = entry('first', 'agent', 1, { final: true })
  const last = entry('last', 'agent', 1, { final: true })
  const next = entry('next', 'user', 2)
  const legacy = entry('legacy', 'notice', 1, { memory: { status: 'done', rev: 1 } })
  assert.deepEqual(ids(placeMemoryNotices([first, last, next, legacy])), ['first', 'last', 'legacy', 'next'])
  const empty = { ...legacy, data: { memory: { status: 'done', rev: 1, responseIds: [] } } }
  const entries = [first, last, next, empty]
  assert.equal(placeMemoryNotices(entries), entries)
  assert.equal(placeMemoryNotices([entry('draft', 'agent', 1), next, legacy])[2], legacy)
})
