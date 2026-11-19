import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Bot, Entry } from '../../../shared/models.ts'
import { stepsByReply } from './cards/tool-steps.ts'
import { workingBlocks } from './working-blocks.ts'

// D169, revisão R1 F4: num canal, cada autor de passos pendentes tem o próprio bloco; o canal nunca vira autor.
const entry = (id: string, botId: string, kind: string, turn: number, at: number, data: Entry['data'] = {}): Entry =>
  ({ id, seq: at, botId, rev: 1, kind, turn, data, createdAt: at, updatedAt: at })
const bot = (id: string, kind = 'agent') => ({ id, kind, name: id, members: kind === 'group' ? ['byte', 'sprint'] : [] }) as unknown as Bot
const bots = new Map([['byte', bot('byte')], ['sprint', bot('sprint')], ['canal', bot('canal', 'group')]])

test('dois autores no mesmo turno do canal ficam em blocos separados, cada um com os próprios passos', () => {
  const { pending } = stepsByReply([
    entry('u', 'canal', 'user', 3, 0, { text: 'oi' }),
    entry('b', 'canal', 'tool', 3, 1000, { title: 'Byte: verificou', toolKind: 'read', status: 'completed', author: 'byte' }),
    entry('s', 'canal', 'tool', 3, 2000, { title: 'Sprint: conferindo', toolKind: 'search', status: 'in_progress', author: 'sprint' }),
  ])
  const blocks = workingBlocks(pending, bots)
  assert.deepEqual(blocks.map((b) => [b.authorId, b.author?.id ?? null, b.steps.steps.map((s) => s.id)]), [['byte', 'byte', ['b']], ['sprint', 'sprint', ['s']]])
})

test('passo sem autor (o canal) ou com id desconhecido fica neutro, sem nome inventado', () => {
  const { pending } = stepsByReply([
    entry('u', 'canal', 'user', 3, 0, { text: 'oi' }),
    entry('x', 'canal', 'tool', 3, 1000, { title: 'Sem autor', toolKind: 'read', status: 'in_progress' }),
    entry('y', 'canal', 'tool', 3, 2000, { title: 'Desconhecido', toolKind: 'read', status: 'in_progress', author: 'fantasma' }),
  ])
  const blocks = workingBlocks(pending, bots)
  assert.deepEqual(blocks.map((b) => [b.authorId, b.author]), [['canal', null], ['fantasma', null]])
})
