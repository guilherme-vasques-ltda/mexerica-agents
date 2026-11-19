// D70 (DON-907, D431): o fluxo da bandeja — o que entra nela e em que ordem, os atalhos do campo vazio, o arraste e
// o erro do Hub em código estável (pedido atrasado, revisão velha, autoridade encerrada).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '@shared/models'
import { atalhoDaFila, destinoDoArraste, falhaDaFila, filaDaConversa, mover, naFila, SHIFT_ENTER_MANDA_PRIMEIRA } from './fila.ts'

const user = (id: string, seq: number, data: Entry['data'], sessionId?: string): Entry =>
  ({ id, seq, botId: 'donna', rev: seq, kind: 'user', turn: 1, createdAt: 0, updatedAt: 0, data, ...(sessionId ? { sessionId } : {}) })

test('a bandeja mostra só as da fila desta conversa, na ordem guardada pelo Hub', () => {
  const entries = [
    user('a', 1, { text: 'A', status: 'queued' }),
    user('enviada', 2, { text: 'já foi', status: 'sent' }),
    user('b', 3, { text: 'B', status: 'queued', queueOrder: 0 }),
    user('extra', 4, { text: 'X', status: 'queued' }, 's2'),
    user('c', 5, { text: 'C', status: 'queued' }, 'donna'),
    user('interna', 6, { text: 'continuação', status: 'queued', from: 'host' }),
  ]
  assert.deepEqual(filaDaConversa(entries, 'donna', null).map((e) => e.id), ['b', 'a', 'c'])
  assert.deepEqual(filaDaConversa(entries, 'donna', 's2').map((e) => e.id), ['extra'])
  assert.equal(naFila(entries[1]!), false, 'a mandada volta para a conversa')
})

const tecla = (key: string, shiftKey = false) => ({ key, shiftKey, altKey: false, metaKey: false, ctrlKey: false, isComposing: false })

test('Shift+Enter e ↑ só agem com o campo vazio e fila não vazia', () => {
  assert.equal(SHIFT_ENTER_MANDA_PRIMEIRA, true, 'D431 provisória')
  assert.equal(atalhoDaFila(tecla('Enter', true), true, 2), 'mandarPrimeira')
  assert.equal(atalhoDaFila(tecla('ArrowUp'), true, 2), 'editarUltima')
  assert.equal(atalhoDaFila(tecla('Enter', true), false, 2), null, 'com texto, Shift+Enter quebra a linha (C1)')
  assert.equal(atalhoDaFila(tecla('ArrowUp'), true, 0), null)
  assert.equal(atalhoDaFila({ ...tecla('Enter', true), isComposing: true }, true, 2), null)
  assert.equal(atalhoDaFila(tecla('Enter'), true, 2), null, 'Enter sozinho continua sendo o envio')
})

test('arrastar escolhe a posição pelo meio das linhas e a prévia move só o item', () => {
  const centros = [10, 38, 66]
  assert.equal(destinoDoArraste(centros, 40, 0), 1, 'a 1ª passa do meio da 2ª: vira a 2ª')
  assert.equal(destinoDoArraste(centros, 0, 2), 0)
  assert.equal(destinoDoArraste(centros, 200, 0), 2)
  assert.equal(destinoDoArraste(centros, 12, 0), 0, 'sem passar de ninguém, fica')
  assert.deepEqual(mover(['a', 'b', 'c'], 2, 0), ['c', 'a', 'b'])
  assert.deepEqual(mover(['a', 'b', 'c'], 0, 2), ['b', 'c', 'a'])
})

test('o erro do Hub vira um código estável; o texto do Hub nunca vai à tela', () => {
  assert.equal(falhaDaFila(new Error('queued_entry_dispatched: a mensagem já saiu da fila')), 'saiu')
  assert.equal(falhaDaFila(new Error('queued_revision_conflict: a mensagem mudou')), 'mudou')
  assert.equal(falhaDaFila(new Error('not_allowed: autorização original encerrada')), 'permissao')
  assert.equal(falhaDaFila(new Error('queued_control_pending')), 'pendente')
  assert.equal(falhaDaFila(new Error('Boom')), 'falhou')
  assert.equal(falhaDaFila(null), 'falhou')
})
