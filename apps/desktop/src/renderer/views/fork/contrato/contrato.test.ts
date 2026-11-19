// T2a (DON-912), D70: o contrato aparece só sob o pedido, com os campos na ordem do Hub.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { botConversation, buildBotConversation } from '../../thread/bot-exchange.ts'
import { camposDoContrato } from './campos.ts'
import type { Entry, TaskContract } from '../../../../shared/models.ts'

const aviso = (contract: unknown): Entry => ({
  id: 'n', seq: 1, botId: 'a', rev: 1, kind: 'notice', turn: 0, createdAt: 1, updatedAt: 1,
  data: { status: 'completed', botMessage: { sourceBotId: 'a', targetBotId: 'b', text: 'Pedido', reply: 'Feito', contract: contract as TaskContract } },
})
const mensagens = (contract: unknown) => buildBotConversation(botConversation([], [aviso(contract)], 'b')).filter((r) => r.kind === 'message')

test('o contrato fica só no pedido; sem result válido, não aparece', () => {
  const contrato = { result: 'Três opções', limits: '1 dia' }
  const [pedido, resposta] = mensagens(contrato)
  assert.deepEqual(pedido && 'contract' in pedido ? pedido.contract : null, contrato)
  assert.ok(resposta && !('contract' in resposta))
  for (const invalido of [undefined, { acceptance: 'testes' }, { result: '' }, { result: 3 }]) {
    assert.ok(mensagens(invalido).every((r) => !('contract' in r)))
  }
})

test('campos na ordem do Hub, sem os vazios', () => {
  const campos = camposDoContrato({ rule: 'D371: código com o Rafa', result: 'Três opções', scope: ' ', acceptance: 'Fontes citadas' })
  assert.deepEqual(campos.map((c) => [c.chave, c.rotulo]), [
    ['result', 'contrato.resultado'], ['acceptance', 'contrato.aceite'], ['rule', 'contrato.regra'],
  ])
})
