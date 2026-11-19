// As três ações de cada linha da bandeja (enviar agora, editar, remover): o pedido que sai do app
// (pedidoDaAcao → queuedAction → fork_fila.rs) e a semântica que a demo espelha do Hub (fake-fila.ts).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '@shared/models'
import { naFila, pedidoDaAcao } from './fila.ts'
import { chamarFila } from '../../../demo/fake-fila.ts'

const user = (id: string, seq: number, data: Entry['data']): Entry =>
  ({ id, seq, botId: 'donna', rev: seq, kind: 'user', turn: 1, createdAt: 0, updatedAt: 0, data })
const naFilaDe = (entries: Entry[]) => entries.filter(naFila)

/** A fila do Hub na demo: emitir registra a entrada; recusa lança o código estável. */
const hub = (entries: Entry[], working = true) => {
  const emitidos: Entry[] = []
  const avisos: string[] = []
  return {
    emitidos,
    avisos,
    deps: {
      entries,
      emit: (e: Entry) => { emitidos.push(e) },
      notice: (_botId: string, text: string) => { avisos.push(text) },
      working: () => working,
      fail: (m: string) => new Error(m),
    },
  }
}

test('cada ação da linha manda o método do Hub com botId e entryId', () => {
  const e = user('m1', 1, { text: 'oi', status: 'queued' })
  assert.deepEqual(pedidoDaAcao('queuedSendNow', 'donna', e), { method: 'queuedSendNow', body: { botId: 'donna', entryId: 'm1' } })
  assert.deepEqual(pedidoDaAcao('queuedRemove', 'donna', e), { method: 'queuedRemove', body: { botId: 'donna', entryId: 'm1' } })
  assert.deepEqual(pedidoDaAcao('queuedEdit', 'donna', e, { text: 'novo', expectedRevision: e.rev }),
    { method: 'queuedEdit', body: { botId: 'donna', entryId: 'm1', text: 'novo', expectedRevision: e.rev } })
})

const fila3 = () => [
  user('a', 1, { text: 'A', status: 'queued', queueOrder: 0 }),
  user('b', 2, { text: 'B', status: 'queued', queueOrder: 1 }),
  user('c', 3, { text: 'C', status: 'queued', queueOrder: 2 }),
]

test('enviar agora manda aquela mensagem furando a fila e interrompe a resposta', () => {
  const entries = fila3()
  const h = hub(entries, true)
  const r = chamarFila('queuedSendNow', { entryId: 'c' }, h.deps)
  assert.equal(entries[2]!.data.status, 'sent')
  assert.deepEqual(naFilaDe(entries).map((e) => e.id), ['a', 'b'], 'as outras seguem na fila, na ordem')
  assert.equal(r.stopRequested, true, 'com o bot respondendo, a resposta é interrompida')
  assert.equal(h.avisos.length, 1)
  assert.ok(h.emitidos.includes(entries[2]!))
})

test('enviar agora com o bot livre não pede interrupção nem aviso', () => {
  const entries = fila3()
  const h = hub(entries, false)
  const r = chamarFila('queuedSendNow', { entryId: 'a' }, h.deps)
  assert.equal(r.stopRequested, false)
  assert.equal(h.avisos.length, 0)
  assert.equal(entries[0]!.data.status, 'sent')
})

test('editar grava o texto aparado e só com a revisão esperada', () => {
  const entries = fila3()
  const h = hub(entries)
  chamarFila('queuedEdit', { entryId: 'b', text: '  texto novo  ', expectedRevision: entries[1]!.rev }, h.deps)
  assert.equal(entries[1]!.data.text, 'texto novo')
  assert.equal(entries[1]!.data.status, 'queued', 'continua na fila')
  assert.throws(() => chamarFila('queuedEdit', { entryId: 'a', text: 'x', expectedRevision: 999 }, h.deps), /queued_revision_conflict/)
  assert.equal(entries[0]!.data.text, 'A', 'a revisão velha não grava')
})

test('remover tira a mensagem da fila', () => {
  const entries = fila3()
  const h = hub(entries)
  chamarFila('queuedRemove', { entryId: 'b' }, h.deps)
  assert.equal(entries[1]!.data.status, 'cancelled')
  assert.deepEqual(naFilaDe(entries).map((e) => e.id), ['a', 'c'])
  assert.ok(h.emitidos.includes(entries[1]!))
})

test('ação em mensagem que já saiu da fila é recusada com o código estável', () => {
  const entries = [user('a', 1, { text: 'A', status: 'sent' }), user('b', 2, { text: 'B', status: 'queued' })]
  const h = hub(entries)
  assert.throws(() => chamarFila('queuedSendNow', { entryId: 'a' }, h.deps), /queued_entry_dispatched/)
  assert.throws(() => chamarFila('queuedRemove', { entryId: 'sumiu' }, h.deps), /queued_entry_dispatched/)
  assert.equal(entries[0]!.data.status, 'sent')
})
