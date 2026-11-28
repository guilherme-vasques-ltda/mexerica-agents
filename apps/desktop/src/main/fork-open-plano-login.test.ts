import assert from 'node:assert/strict'
import { test } from 'node:test'
import { consumirPlano, descartarPlano, guardarPlano } from './fork-open/plano-login.ts'

const url = 'https://login.exemplo.test/a?state=s'
const desktop = { url, callback: 'desktop' }

test('só a URL que o Hub devolveu para o ouvinte do app é entregue, e uma única vez', () => {
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano('https://login.exemplo.test/outra'), false)
  assert.equal(consumirPlano(url), false, 'a tentativa errada gasta o plano')
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano(url), true)
  assert.equal(consumirPlano(url), false)
})

test('resposta do Hub que não é para o ouvinte do app, ou sem tentativa, não deixa plano', () => {
  guardarPlano({ url, callback: 'host' }, { tentativa: 't1' })
  assert.equal(consumirPlano(url), false)
  guardarPlano(desktop, {})
  assert.equal(consumirPlano(url), false)
  guardarPlano(null, { tentativa: 't1' })
  assert.equal(consumirPlano(''), false)
})

test('cancelar ou concluir a tentativa descarta o plano dela, e só o dela', () => {
  guardarPlano(desktop, { tentativa: 't1' })
  descartarPlano({ tentativa: 't1' })
  assert.equal(consumirPlano(url), false)
  guardarPlano(desktop, { tentativa: 't2' })
  descartarPlano({ tentativa: 't1' })
  assert.equal(consumirPlano(url), true, 'cancelar uma tentativa velha não derruba a atual')
  guardarPlano(desktop, { tentativa: 't2' })
  descartarPlano({})
  assert.equal(consumirPlano(url), false)
})

test('uma tentativa nova invalida o plano da anterior', () => {
  guardarPlano(desktop, { tentativa: 't1' })
  guardarPlano({ url: 'https://login.exemplo.test/b?state=z', callback: 'desktop' }, { tentativa: 't2' })
  assert.equal(consumirPlano(url), false)
})
