import { test } from 'node:test'
import assert from 'node:assert/strict'
import { railLabel, waitingYouCount } from './badge.ts'

// D70: o ponto só acende com dado do Hub da faixa, no Pessoal, contando só quem precisa de você.
test('badge: conta só Precisa de você do Pessoal; sem dado não acende', () => {
  const bots = [
    { status: 'needsInput' },
    { status: 'needsInput', spaceId: 'p' },
    { status: 'needsInput', spaceId: 'loja' },
    { status: 'needsInput', kind: 'group' },
    { status: 'needsInput', hidden: true },
    { status: 'working', spaceId: 'p' },
  ]
  const base = { online: true, inPessoal: true, personalSpaceId: 'p', bots }
  assert.equal(waitingYouCount(base), 2)
  assert.equal(railLabel(2, 'pt-BR'), '\u2068Mission Board\u2069, 2 esperando você')
  assert.equal(railLabel(2, 'en'), '\u2068Mission Board\u2069, 2 waiting for you')
  // D341: parado, trabalhando, aguardando, com erro ou sem estado não acendem o ponto.
  assert.equal(waitingYouCount({ ...base, bots: ['idle', 'working', 'waiting', 'error', 'unavailable'].map((status) => ({ status })) }), 0)
  assert.equal(railLabel(0, 'pt-BR'), 'Mission Board')
  for (const off of [{ online: false }, { inPessoal: false }, { personalSpaceId: null }]) {
    const n = waitingYouCount({ ...base, ...off })
    assert.equal(n, null)
    assert.equal(railLabel(n, 'en'), 'Mission Board')
  }
})
