// MX-77: os pedidos de outros bots na bandeja — o que entra nela, a origem "De <bot/sessão>" e a mistura com as
// mensagens do dono (conversa/pedidos.ts).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '@shared/models'
import { filaComPedidos, origemDoPedido, pedidoNaFila, posicaoEntreMensagens } from './pedidos.ts'

const user = (id: string, seq: number, data: Entry['data'], sessionId?: string): Entry =>
  ({ id, seq, botId: 'donna', rev: seq, kind: 'user', turn: 1, createdAt: 0, updatedAt: 0, data, ...(sessionId ? { sessionId } : {}) })

const pedido = (id: string, seq: number, data: Partial<Entry['data']> = {}, botId = 'donna', alvo = 'donna'): Entry =>
  ({
    id, seq, botId, rev: seq, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0,
    data: {
      status: 'queued', delegationId: `del-${id}`, sourceBotId: 'byte', targetBotId: alvo,
      sourceChat: 'byte', sourceSession: null,
      botMessage: { sourceBotId: 'byte', targetBotId: alvo, text: 'revisa o diff' },
      ...data,
    },
  })

test('a bandeja inclui o pedido de outro bot ainda na fila, só o aviso do destinatário', () => {
  const alvo = pedido('p1', 2)
  const remetente = pedido('p2', 3, {}, 'byte', 'donna') // na conversa do Byte: aviso do remetente, targetBotId != botId
  const despachado = pedido('p3', 4, { status: 'sent' })
  const semPedido = pedido('p4', 5, { botMessage: undefined, delegationId: undefined })
  assert.equal(pedidoNaFila(alvo), true)
  assert.equal(pedidoNaFila(remetente), false, 'o aviso do remetente não é item da fila do destinatário')
  assert.equal(pedidoNaFila(despachado), false, 'despachado sai da bandeja')
  assert.equal(pedidoNaFila(semPedido), false, 'aviso comum não é pedido')
})

test('a bandeja mistura mensagens do dono e pedidos de bots na ordem de chegada', () => {
  const entries = [
    user('a', 1, { text: 'A', status: 'queued' }),
    pedido('p1', 2),
    user('b', 3, { text: 'B', status: 'queued' }),
    pedido('p2', 4, { status: 'sent' }),
    pedido('p3', 5, {}, 'byte', 'donna'),
  ]
  assert.deepEqual(filaComPedidos(entries, 'donna', null).map((e) => e.id), ['a', 'p1', 'b'], 'despachado e remetente ficam fora')
  assert.deepEqual(filaComPedidos(entries, 'donna', 's2').map((e) => e.id), [], 'sessão extra não tem pedido de bot')
})

test('a origem do pedido é o bot e, fora da principal, a sessão ou a conversa dele', () => {
  const rotulos = {
    bot: (id: string) => ({ byte: 'Byte', prosa: 'Prosa' })[id],
    sessao: (_chat: string, id: string) => (id === 's-guard' ? 'Guarda de data' : undefined),
    conversa: (chat: string) => (chat === 'conv-9' ? 'Planejamento' : undefined),
  }
  assert.equal(origemDoPedido(pedido('p1', 1), rotulos), 'Byte')
  assert.equal(origemDoPedido(pedido('p2', 2, { sourceBotId: 'byte', sourceSession: 's-guard' }), rotulos), 'Byte · Guarda de data')
  assert.equal(origemDoPedido(pedido('p3', 3, { sourceBotId: 'prosa', sourceChat: 'conv-9' }), rotulos), 'Prosa · Planejamento')
  assert.equal(origemDoPedido(pedido('p4', 4, { sourceBotId: 'fantasma' }), rotulos), 'fantasma', 'sem nome, cai no id')
})

test('mover usa o índice entre as mensagens do dono: pedido no meio não conta nem se move', () => {
  const A = user('A', 1, { text: 'A', status: 'queued' })
  const P = pedido('P', 2)
  const B = user('B', 3, { text: 'B', status: 'queued' })
  const C = user('C', 4, { text: 'C', status: 'queued' })
  const itens = [A, P, B, C]
  // Repro do revisor: Alt+↑ em C manda o índice visual 2; o Hub quer 1 (entre A, B, C) → [A, P, C, B].
  assert.equal(posicaoEntreMensagens(itens, C, 2), 1)
  // Arraste de C por cima do pedido, parando logo antes dele: também a 2ª entre as do dono ([A, C, P, B]).
  assert.equal(posicaoEntreMensagens(itens, C, 1), 1, 'antes do pedido, depois de A')
  assert.equal(posicaoEntreMensagens(itens, B, 1), null, 'B só passou por cima do pedido: nada muda para o Hub')
  // Topo e fim com o pedido no meio.
  assert.equal(posicaoEntreMensagens(itens, C, 0), 0, 'para o topo')
  assert.equal(posicaoEntreMensagens(itens, A, 3), 2, 'para o fim')
  assert.equal(posicaoEntreMensagens(itens, B, 3), 2)
  assert.equal(posicaoEntreMensagens(itens, P, 0), null, 'o pedido nunca se move')
})
