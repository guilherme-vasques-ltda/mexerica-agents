// MEX-1391/F6-B: regras puras da ordem de reserva — só assinatura comprovada do mesmo motor, até 3, e a hora do descanso.
import assert from 'node:assert/strict'
import test from 'node:test'
import type { FallbackState, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { acrescentar, candidatas, emReservaPor, mover, MAX_RESERVAS, ordemDoMotor, situacaoDaConta, tirar } from './reserva.ts'

const conta = (id: string, extra: Partial<ManagedAccount> = {}): ManagedAccount => ({
  id, label: id, brand: 'Anthropic', backend: 'claude', billing: 'subscription', state: 'connected', models: [], revision: 1, origin: 'managed', botIds: [], ...extra,
})
const estado = (extra: Partial<FallbackState> = {}): FallbackState => ({ spaceId: 's', orders: [], botOrders: [], resting: [], sessions: [], ...extra })

test('candidatas: só assinatura conectada do mesmo motor, fora da própria conta e da lista; chave de API nunca', () => {
  const contas = [
    conta('a'), conta('b'), conta('c'), conta('api', { billing: 'paid_api' }), conta('naoConf', { billing: 'unverified' }),
    conta('off', { state: 'disconnected' }), conta('cx', { backend: 'codex', brand: 'OpenAI' }),
  ]
  assert.deepEqual(candidatas(contas, 'claude', ['c'], 'a').map((c) => c.id), ['b'])
  // Sem vínculo com o Space aberto, nem aparece como opção (o Hub recusa do mesmo jeito).
  assert.deepEqual(candidatas(contas, 'claude', [], 'a', (c) => c.id === 'c').map((c) => c.id), ['c'])
})

test('ordem: subir, descer, tirar e acrescentar respeitam as bordas e o teto de 3', () => {
  assert.deepEqual(mover(['a', 'b', 'c'], 'b', -1), ['b', 'a', 'c'])
  assert.deepEqual(mover(['a', 'b', 'c'], 'a', -1), ['a', 'b', 'c'])
  assert.deepEqual(mover(['a', 'b', 'c'], 'c', 1), ['a', 'b', 'c'])
  assert.deepEqual(tirar(['a', 'b'], 'a'), ['b'])
  assert.deepEqual(acrescentar(['a'], 'a'), ['a'], 'cada conta aparece uma vez')
  assert.equal(MAX_RESERVAS, 3)
  assert.deepEqual(acrescentar(['a', 'b', 'c'], 'd'), ['a', 'b', 'c'])
  assert.deepEqual(ordemDoMotor(estado({ orders: [{ backend: 'claude', accounts: ['b'] }] }), 'codex'), [])
})

test('situação: descanso vencido não conta; uso extra e bloqueio vivos contam; "Na reserva" lista as conversas da origem', () => {
  const now = 1_000
  const s = estado({
    resting: [
      { engineAccountId: 'a', restUntil: 5_000, reason: 'perto_do_limite', extraUntil: null, blockedUntil: null },
      { engineAccountId: 'v', restUntil: 500, reason: 'esgotada', extraUntil: null, blockedUntil: null },
      { engineAccountId: 'p', restUntil: null, reason: null, extraUntil: null, blockedUntil: 9_000 },
    ],
    sessions: [{ sessionId: 'x', botId: 'bot', originAccountId: 'a', currentAccountId: 'b', since: 1, reason: 'perto_do_limite', originBackAt: 5_000 }],
  })
  assert.equal(situacaoDaConta(s, 'a', now)?.reason, 'perto_do_limite')
  assert.equal(situacaoDaConta(s, 'v', now), null)
  assert.equal(situacaoDaConta(s, 'p', now)?.blockedUntil, 9_000)
  assert.equal(situacaoDaConta(null, 'a', now), null)
  assert.deepEqual(emReservaPor(s, 'a').map((x) => x.currentAccountId), ['b'])
})
