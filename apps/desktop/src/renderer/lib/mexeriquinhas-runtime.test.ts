import assert from 'node:assert/strict'
import test from 'node:test'
import { normalizeBot } from '../../shared/models.ts'
import { INATIVIDADE_MS } from './mexeriquinhas/estado.ts'
import { estadoDoBot, proximoSono, quadrosDoEstado, ultimaAtividadeDoBot } from './mexeriquinhas-runtime.ts'

test('estados reais respeitam prioridade, ausência de audiência e borda estrita de 8h', () => {
  const at = 1000
  const bot = normalizeBot({ id: 'a', status: 'idle', lastAt: at })
  assert.equal(estadoDoBot(bot, at + INATIVIDADE_MS), 'livre')
  assert.equal(proximoSono(bot, at + INATIVIDADE_MS), 1)
  assert.equal(estadoDoBot(bot, at + INATIVIDADE_MS + 1), 'dormindo')
  assert.equal(proximoSono(bot, at + INATIVIDADE_MS + 1), null)
  const later = at + INATIVIDADE_MS * 2
  assert.equal(estadoDoBot({ ...bot, status: 'working' }, later), 'trabalhando')
  assert.equal(estadoDoBot({ ...bot, status: 'needsInput' }, later), 'precisa-resposta')
  assert.equal(estadoDoBot({ ...bot, status: 'error' }, later), 'erro')
  assert.equal(estadoDoBot({ ...bot, status: 'unavailable' }, later), null)
  assert.equal(proximoSono({ ...bot, status: 'unavailable' }, later), null)
  assert.equal(estadoDoBot({ ...bot, lastAt: later + 1 }, later), 'livre')
  assert.equal(estadoDoBot({ ...bot, lastAt: 0 }, later), 'livre')
})

test('entrada de ferramenta recente mantém livre mesmo com última mensagem antiga', () => {
  const bot = normalizeBot({ id: 'a', status: 'idle', lastAt: 1000 })
  const now = INATIVIDADE_MS * 2
  assert.equal(estadoDoBot(bot, now), 'dormindo')
  assert.equal(estadoDoBot(bot, now, now - 1), 'livre')
  assert.equal(ultimaAtividadeDoBot(bot, now - 1), now - 1)
  assert.deepEqual(quadrosDoEstado('livre', false), ['livre-1'])
  assert.deepEqual(quadrosDoEstado('trabalhando', true), ['trabalhando-1', 'trabalhando-2'])
  assert.deepEqual(quadrosDoEstado('erro', true), ['erro'])
})
