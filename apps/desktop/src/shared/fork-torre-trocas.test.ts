import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseTrocasDeConta } from './fork-torre-trocas.ts'

// MEX-1391 (o).2: o parser só aceita o que conhece; sem dado é indisponível (nunca "nenhuma troca").
const item = (extra: Record<string, unknown> = {}) => ({ id: 't1', at: 1000, botId: 'b', fromLabel: 'Conta a', toLabel: 'Conta b', reason: 'perto_do_limite', backAt: null, ...extra })

test('trocas: aceita os três motivos e a hora de volta', () => {
  const ok = parseTrocasDeConta({ items: [item(), item({ id: 't2', reason: 'esgotada', backAt: 5000 }), item({ id: 't3', reason: 'voltou' })], omitted: 2 })
  assert.deepEqual(ok?.items.map((i) => [i.id, i.reason, i.backAt]), [['t1', 'perto_do_limite', null], ['t2', 'esgotada', 5000], ['t3', 'voltou', null]])
  assert.equal(ok?.omitted, 2)
})

test('trocas: motivo desconhecido e item torto são ignorados; lista inválida é indisponível', () => {
  assert.deepEqual(parseTrocasDeConta({ items: [item({ reason: 'outro' }), item({ id: '' }), 7, item({ id: 'bom' })], omitted: 0 })?.items.map((i) => i.id), ['bom'])
  assert.equal(parseTrocasDeConta({ items: 'x', omitted: 0 }), null)
  assert.equal(parseTrocasDeConta({ items: [], omitted: -1 }), null)
  assert.equal(parseTrocasDeConta(undefined), null)
})
