import { test } from 'node:test'
import assert from 'node:assert/strict'
import { faceState, FACE_STATE_LABEL } from './face-state.ts'

test('estado no rosto: pedido vence trabalho, que vence o sono; parado é Livre', () => {
  assert.equal(faceState({ status: 'needsInput' }, 'sleep'), 'listen')
  assert.equal(faceState({ status: 'working' }, 'sleep'), 'work')
  assert.equal(faceState({ status: 'idle' }, 'sleep'), 'sleep')
  assert.equal(faceState({ status: 'idle' }, 'drowsy'), 'sleep')
  assert.equal(faceState({ status: 'idle' }, 'think'), 'idle')
  assert.equal(faceState({ status: 'waiting' }, 'idle'), 'idle')
  assert.equal(FACE_STATE_LABEL.listen, 'Aguardando resposta')
})
