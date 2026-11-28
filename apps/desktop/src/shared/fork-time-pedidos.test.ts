import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseTeamRequests, teamRequestLabel } from './fork-time-pedidos.ts'

// Aba "Time": o parser só aceita o que conhece e só a resposta do bot pedido; o texto ausente continua ausente.
const item = (extra: Record<string, unknown> = {}) => ({
  key: 'k1', targetBotId: 'ana', text: 'Revise o contrato', state: 'running', reason: null, outcome: null, mode: 'wait', updatedAt: 1000, ...extra,
})
const resposta = (items: unknown[], extra: Record<string, unknown> = {}) => ({ botId: 'berga', spaceId: 'personal', items, omitted: 0, ...extra })

test('time: rótulos de aberto, concluído e terminado sem sucesso', () => {
  const cases: [Record<string, unknown>, string, string][] = [
    [{ state: 'queued' }, 'queued', 'active'],
    [{ state: 'running' }, 'running', 'active'],
    [{ state: 'finished', outcome: 'completed' }, 'completed', 'done'],
    [{ state: 'finished', outcome: 'failed' }, 'failed', 'error'],
    [{ state: 'cancelled', reason: 'timeout' }, 'timeout', 'neutral'],
    [{ state: 'interrupted', reason: 'restart_uncertain' }, 'restart_uncertain', 'uncertain'],
  ]
  for (const [extra, rotulo, tone] of cases) {
    const lido = parseTeamRequests(resposta([item(extra)]), 'berga')
    assert.equal(lido?.items.length, 1, JSON.stringify(extra))
    assert.deepEqual(teamRequestLabel(lido!.items[0]!), { rotulo, tone })
  }
})

test('time: combinação desconhecida some, nunca vira outro estado', () => {
  for (const extra of [{ state: 'running', outcome: 'completed' }, { state: 'queued', reason: 'timeout' }, { state: 'finished', reason: 'timeout' }, { state: 'novo' }, { mode: 'outro' }, { key: '' }, { text: 7 }, { updatedAt: 'ontem' }, { reason: 7 }, { outcome: {} }, { state: 'finished', outcome: 'completed', reason: '' }]) {
    assert.deepEqual(parseTeamRequests(resposta([item(extra)]), 'berga')?.items, [], JSON.stringify(extra))
  }
})

test('time: texto ausente continua ausente e resposta de outro bot é recusada', () => {
  assert.equal(parseTeamRequests(resposta([item({ text: null })]), 'berga')?.items[0]?.text, null)
  assert.equal(parseTeamRequests(resposta([item()]), 'ana'), null)
  assert.equal(parseTeamRequests(resposta([item()], { omitted: -1 }), 'berga'), null)
  assert.equal(parseTeamRequests({ botId: 'berga', omitted: 0 }, 'berga'), null)
  assert.equal(parseTeamRequests(resposta([item(), item({ key: 'k2' })], { omitted: 3 }), 'berga')?.omitted, 3)
})
