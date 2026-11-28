import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '../../../../shared/models.ts'
import { pendingView, responderOf } from './responder.ts'

// MB-1 (D70): o Board responde pelo MESMO cartão e pela MESMA intenção da conversa; resolvido em qualquer lugar,
// mostra o resultado sem botões; sem cartão carregado, conexão ou fora do Pessoal, só Abrir conversa.
const card = (status: string, rev = 4) => ({ id: 'e1', kind: 'permission', botId: 'fin', rev, seq: 1, turn: 1, createdAt: 0, updatedAt: 0, data: { status, author: 'fin' } }) as unknown as Entry

test('Torre abre a conversa para aprovar comando local', () => {
  const entry = card('pending')
  entry.data.toolKind = 'open_local_command'
  assert.deepEqual(pendingView(entry, { live: true, personal: true }), { kind: 'openOnly', nota: null })
})
const ctx = { live: true, personal: true }

test('estados do item: cartão, já respondido, não está mais esperando, só Abrir', () => {
  assert.equal(pendingView(card('pending'), ctx).kind, 'card')
  assert.deepEqual(pendingView(card('answered'), ctx), { kind: 'settled', entry: card('answered'), nota: 'torre.resp.jaRespondido' })
  assert.equal(pendingView(card('cancelled'), ctx).kind === 'settled' && pendingView(card('cancelled'), ctx).nota, 'torre.resp.naoEspera')
  assert.equal(pendingView(card('pending'), { ...ctx, live: false }).kind, 'openOnly')
  assert.equal(pendingView(card('pending'), { ...ctx, personal: false }).kind, 'openOnly')
  assert.equal(pendingView(null, ctx).kind, 'openOnly')
})

test('responder no Board é a mesma chamada da conversa, com o cartão atual do store', () => {
  const calls: [Entry, string | null][] = []
  const latest = card('pending', 7)
  const r = responderOf({ allEntries: (chat) => (chat === 'fin' ? [latest] : []), respond: (e, o) => calls.push([e, o]), answering: new Map([['e1', 'allow']]), hostName: 'Servidor', send: () => {}, discard: () => {}, subscribe: () => () => {} })
  const entry = r.entry('fin', 'e1')!
  r.respond(entry, 'allow')
  // O mesmo objeto do store (com a revisão que o Hub confere), sem cópia nem campo extra.
  assert.equal(calls[0]![0], latest)
  assert.equal(calls[0]![1], 'allow')
  assert.equal(r.answering('e1'), 'allow')
  assert.equal(r.entry('outra', 'e1'), null)
})
