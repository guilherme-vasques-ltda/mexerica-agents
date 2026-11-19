import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ago, interBotLabel, parseInterBotRequests } from './fork-torre-pedidos.ts'
import { openTarget, parseTowerSnapshot } from './fork-torre.ts'

// MB-2 (D70): o parser só aceita o que conhece; incerto continua incerto; sem dado não é "nenhum"; Abrir vai à
// conversa de quem pediu pelo destino do cartão.
const item = (extra: Record<string, unknown> = {}) => ({
  key: 'k1', requesterBotId: 'a', targetBotId: 'b', state: 'cancelled', reason: 'timeout', outcome: null, mode: 'wait', endedAt: 1000, ...extra,
})

test('pedidos: rótulos do mapa e incerto nunca vira concluído nem falha', () => {
  const cases: [Record<string, unknown>, string][] = [
    [{ state: 'interrupted', reason: 'restart_uncertain' }, 'restart_uncertain'],
    [{ state: 'interrupted', reason: 'restart_before_start' }, 'restart_before_start'],
    [{ state: 'interrupted', reason: 'host_stopped' }, 'host_stopped'],
    [{ state: 'interrupted', reason: 'start_failed' }, 'start_failed'],
    [{ state: 'cancelled', reason: 'timeout' }, 'timeout'],
    [{ state: 'cancelled', reason: 'recipient_stopped' }, 'recipient_stopped'],
    [{ state: 'cancelled', reason: 'cancelled' }, 'cancelled'],
    [{ state: 'cancelled', reason: 'not_started' }, 'not_started'],
    [{ state: 'finished', reason: null, outcome: 'failed' }, 'failed'],
    [{ state: 'finished', reason: null, outcome: 'stopped' }, 'stopped'],
    [{ state: 'finished', reason: null, outcome: 'incomplete' }, 'incomplete'],
  ]
  for (const [extra, label] of cases) assert.equal(interBotLabel(item(extra) as never)?.rotulo, label)
  assert.equal(interBotLabel(item({ state: 'interrupted', reason: 'restart_uncertain' }) as never)?.tone, 'uncertain')
  // Concluído, motivo trocado de estado ou desconhecido: a linha não aparece.
  for (const extra of [{ state: 'finished', reason: null, outcome: 'completed' }, { state: 'cancelled', reason: 'restart_uncertain' }, { state: 'running', reason: null }, { reason: 'novo' }]) {
    assert.equal(interBotLabel(item(extra) as never), null)
  }
})

test('pedidos: item desconhecido é ignorado e lista inválida fica indisponível', () => {
  const parsed = parseInterBotRequests({ items: [item(), item({ key: 'k2', state: 'finished', reason: null, outcome: 'completed' }), item({ key: 'k3', mode: 'outro' }), 'x'], omitted: 2 })
  assert.deepEqual(parsed?.items.map((i) => i.key), ['k1'])
  assert.equal(parsed?.omitted, 2)
  for (const bad of [null, { items: 'x', omitted: 0 }, { items: [], omitted: -1 }, { items: [] }]) assert.equal(parseInterBotRequests(bad), null)
  assert.deepEqual(ago(0, 30_000), { unit: 'now' })
  assert.deepEqual(ago(0, 18 * 60_000), { unit: 'minute', n: 18 })
  assert.deepEqual(ago(0, 2 * 3_600_000), { unit: 'hour', n: 2 })
  assert.deepEqual(ago(0, 30 * 3_600_000), { unit: 'day', n: 1 })
})

test('pedidos: snapshot sem o campo lê indisponível; com o campo, Abrir usa o cartão de quem pediu', () => {
  const base = (extra: Record<string, unknown>) => ({ version: 1, hubId: 'h1', snapshotId: 's', collectedAt: 5, spaceId: 'p', bots: [], ...extra })
  assert.deepEqual(parseTowerSnapshot(base({}), { hubId: 'h1', spaceId: 'p' }).interBotRequests, { availability: 'unavailable' })
  const snap = parseTowerSnapshot(base({ interBotRequests: { availability: 'available', source: 'retornos.pendentes', coverage: 'c', value: { items: [item()], omitted: 0 } } }), { hubId: 'h1', spaceId: 'p' })
  assert.equal(snap.interBotRequests.availability, 'available')
  const wire = JSON.stringify(snap.interBotRequests)
  for (const k of ['text', 'message', 'reply', 'entry', 'digest']) assert.ok(!wire.includes(k), k)
  // Abrir de um pedido = destino do cartão do solicitante: no Pessoal, a conversa própria dele.
  assert.deepEqual(openTarget('a', 'a', 'p', 'p'), { kind: 'personal', botId: 'a' })
})
