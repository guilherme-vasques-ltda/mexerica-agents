import assert from 'node:assert/strict'
import test from 'node:test'
import { conferirFila, decisoesDoItem, estadoFila, motivoFila, resumoFila, type ItemFila } from './fila.ts'
import { tradutor, type ChaveTexto } from '../../../lib/fork-i18n/index.ts'

const semParametros = (tr: ReturnType<typeof tradutor>) => tr as unknown as (chave: ChaveTexto) => string

// MX-18: a fila de saída como a tela mostra — estados, motivos e as decisões do dono.

const item = (o: Partial<ItemFila>): ItemFila => ({
  id: 'i1', botId: 'b', chatId: '5511900000102@s.whatsapp.net', isGroup: false,
  replyTo: 'm1', text: 'oi', origin: 'owner_card', state: 'pending', reason: null,
  notBefore: 0, attempts: 0, sentAt: null, createdAt: 1, ...o,
})

test('conferirFila: resposta de outro formato é erro, nunca lista vazia', () => {
  assert.deepEqual(conferirFila({ items: [item({})] }), [item({})])
  assert.deepEqual(conferirFila({ items: [] }), [])
  for (const ruim of [null, {}, { items: 'x' }, { items: [{ id: 1 }] }, { chats: [] }]) {
    assert.throws(() => conferirFila(ruim), /resposta_invalida/)
  }
})

test('estado e motivo viram texto do catálogo; motivo só quando o estado não diz tudo', () => {
  const t = semParametros(tradutor('pt-BR'))
  assert.equal(t(estadoFila(item({ state: 'pending' }))), 'Pendente')
  assert.equal(t(estadoFila(item({ state: 'held', reason: 'quiet' }))), 'Segurada')
  assert.equal(t(estadoFila(item({ state: 'uncertain' }))), 'Incerta')
  assert.equal(t(estadoFila(item({ state: 'estado_novo' }))), 'Na fila')
  assert.equal(t(motivoFila(item({ state: 'held', reason: 'quiet' }))!), 'horário de silêncio')
  assert.equal(t(motivoFila(item({ state: 'held', reason: 'limit' }))!), 'limite de mensagens')
  assert.equal(t(motivoFila(item({ state: 'held', reason: 'not_connected' }))!), 'sem número conectado')
  assert.equal(t(motivoFila(item({ state: 'held', reason: 'send_error' }))!), 'esperando a vez')
  assert.equal(t(motivoFila(item({ state: 'pending', reason: 'retry' }))!), 'nova tentativa agendada')
  assert.equal(t(motivoFila(item({ state: 'uncertain', reason: 'uncertain' }))!), 'o envio não confirmou — confira no WhatsApp')
  assert.equal(motivoFila(item({ state: 'sent' })), null)
  assert.equal(motivoFila(item({ state: 'held', reason: 'quiet' })) !== null, true)
})

test('decisões: incerto vira "enviou"/"não saiu"; pendente e segurada só cancelam', () => {
  assert.deepEqual(decisoesDoItem(item({ state: 'uncertain' })), ['sent', 'cancelled'])
  assert.deepEqual(decisoesDoItem(item({ state: 'pending' })), ['cancelled'])
  assert.deepEqual(decisoesDoItem(item({ state: 'held' })), ['cancelled'])
  assert.deepEqual(decisoesDoItem(item({ state: 'sent' })), [])
  assert.deepEqual(decisoesDoItem(item({ state: 'cancelled' })), [])
})

test('resumo conta a fila e quantos pedem decisão', () => {
  const items = [
    item({ id: 'a', state: 'sent', sentAt: 2 }),
    item({ id: 'b', state: 'held' }),
    item({ id: 'c', state: 'uncertain' }),
    item({ id: 'd', state: 'uncertain' }),
  ]
  assert.deepEqual(resumoFila(items), { total: 4, esperando: 1, atencao: 2 })
  assert.deepEqual(resumoFila([]), { total: 0, esperando: 0, atencao: 0 })
})
