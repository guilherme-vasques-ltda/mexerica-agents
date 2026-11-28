import assert from 'node:assert/strict'
import { test } from 'node:test'
import { consumirPlano, guardarPlano } from './fork-open/plano-login.ts'

test('só a URL que o Hub devolveu para o ouvinte do app é entregue, e uma única vez', () => {
  guardarPlano({ url: 'https://login.exemplo.test/a?state=s', callback: 'desktop' })
  assert.equal(consumirPlano('https://login.exemplo.test/outra'), false)
  assert.equal(consumirPlano('https://login.exemplo.test/a?state=s'), false, 'a tentativa errada gasta o plano')
  guardarPlano({ url: 'https://login.exemplo.test/a?state=s', callback: 'desktop' })
  assert.equal(consumirPlano('https://login.exemplo.test/a?state=s'), true)
  assert.equal(consumirPlano('https://login.exemplo.test/a?state=s'), false)
})

test('resposta do Hub que não é para o ouvinte do app não deixa plano', () => {
  guardarPlano({ url: 'https://login.exemplo.test/a?state=s', callback: 'host' })
  assert.equal(consumirPlano('https://login.exemplo.test/a?state=s'), false)
  guardarPlano(null)
  assert.equal(consumirPlano(''), false)
})

