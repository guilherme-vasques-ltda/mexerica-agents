import assert from 'node:assert/strict'
import test from 'node:test'
import { canStartCall } from './call-availability.ts'

test('sem voz local nem provedor confirmado, não oferece Ligar', () => {
  for (const status of [null, { providers: [] }, { providers: [{ provider: 'openai' as const, configured: false }] }]) {
    assert.equal(canStartCall(false, null, status), false)
    assert.equal(canStartCall(false, 'openai', status), false)
  }
})

test('preserva voz local e provedor escolhido; sem voz local, nunca escolhe pago sozinho (voz-03)', () => {
  const status = { providers: [{ provider: 'openai' as const, configured: false }, { provider: 'gemini' as const, configured: true }] }
  assert.equal(canStartCall(true, null, null), true)
  assert.equal(canStartCall(true, 'openai', status), true, 'falha da nuvem pode usar voz local')
  assert.equal(canStartCall(false, 'gemini', status), true)
  assert.equal(canStartCall(false, null, status), false, 'sem voz local, a chamada não pega o primeiro provedor com chave')
  assert.equal(canStartCall(false, 'openai', status), false, 'a chamada não troca um provedor escolhido por outro sem passar pelo fallback local')
})

test('chave só no aparelho habilita o provedor escolhido e estados pendentes não usam a reserva sozinhos', () => {
  const status = { providers: [{ provider: 'openai' as const, configured: false }] }
  for (const situacao of ['guardada', 'ausente', 'travada', 'recusada', 'incerta', 'limpeza-pendente'] as const) {
    const aparelho = { openai: { situacao, reserva: false }, gemini: { situacao: 'ausente' as const, reserva: false } }
    assert.equal(canStartCall(false, 'openai', status, aparelho), situacao === 'guardada')
    assert.equal(canStartCall(false, null, status, aparelho), false, 'não escolhe provedor pago sozinho')
    const hub = { providers: [{ provider: 'openai' as const, configured: true }] }
    assert.equal(canStartCall(false, 'openai', hub, aparelho), situacao === 'guardada' || situacao === 'ausente')
  }
})
