import assert from 'node:assert/strict'
import { test } from 'node:test'
import { precisaConfirmar } from './regras.ts'
import { usoDaConversa } from './uso.ts'

const medidor = (leituras: Record<string, number>) => ({ of: (b: string, s: string | null) => (`${b}/${s}` in leituras ? { used: leituras[`${b}/${s}`]! } : null) })

test('o uso vem do medidor da mesma conversa e sessão; sem medida, a troca vai direto', () => {
  const m = medidor({ 'bot/null': 84_000, 'bot/extra': 10_000 })
  assert.equal(usoDaConversa(m, 'bot', null), 84_000)
  assert.equal(usoDaConversa(m, 'bot', 'extra'), 10_000)
  assert.equal(usoDaConversa(m, 'bot', 'outra'), null)
  const opus = { backend: 'claude', model: 'opus' }, haiku = { backend: 'claude', model: 'haiku' }
  assert.equal(precisaConfirmar(opus, haiku, usoDaConversa(m, 'bot', null)), true, 'principal com 84 mil: pergunta')
  assert.equal(precisaConfirmar(opus, haiku, usoDaConversa(m, 'bot', 'extra')), false, 'extra com 10 mil: troca direto')
  assert.equal(precisaConfirmar(opus, haiku, usoDaConversa(m, 'bot', 'outra')), false, 'sem medida: troca direto')
})
