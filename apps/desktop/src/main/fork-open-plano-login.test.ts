import assert from 'node:assert/strict'
import { test } from 'node:test'
import { consumirPlano, descartarPlano, guardarPlano, ligarOuvinte } from './fork-open/plano-login.ts'

const url = 'https://login.exemplo.test/a?state=s'
const desktop = { url, callback: 'desktop' }
let vivo: string | null = null
ligarOuvinte(() => vivo)

test('só a URL que o Hub devolveu para o ouvinte vivo é entregue, e uma única vez', () => {
  vivo = 't1'
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano('https://login.exemplo.test/outra', 't1'), false)
  assert.equal(consumirPlano(url, 't1'), false, 'a tentativa errada gasta o plano')
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano(url, 't1'), true)
  assert.equal(consumirPlano(url, 't1'), false)
})

test('resposta do Hub que não é para o ouvinte do app, ou sem tentativa, não deixa plano', () => {
  vivo = 't1'
  guardarPlano({ url, callback: 'host' }, { tentativa: 't1' })
  assert.equal(consumirPlano(url, 't1'), false)
  guardarPlano(desktop, {})
  assert.equal(consumirPlano(url, 't1'), false)
  guardarPlano(null, { tentativa: 't1' })
  assert.equal(consumirPlano('', 't1'), false)
})

test('resposta atrasada de tentativa cancelada, ou sem ouvinte vivo, não repõe plano', () => {
  vivo = null
  guardarPlano(desktop, { tentativa: 't1' })
  vivo = 't1'
  assert.equal(consumirPlano(url, 't1'), false, 'o ouvinte fechou antes da resposta')
  vivo = 't2'
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano(url, 't2'), false, 'a resposta é de outra tentativa que não a viva')
  assert.equal(consumirPlano(url, 't1'), false)
})

test('o plano só serve ao ouvinte da própria tentativa', () => {
  vivo = 't1'
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano(url, 't2'), false)
  guardarPlano(desktop, { tentativa: 't1' })
  assert.equal(consumirPlano(url, null), false)
})

test('cancelar ou concluir a tentativa descarta o plano dela, e só o dela', () => {
  vivo = 't1'
  guardarPlano(desktop, { tentativa: 't1' })
  descartarPlano({ tentativa: 't1' })
  assert.equal(consumirPlano(url, 't1'), false)
  vivo = 't2'
  guardarPlano(desktop, { tentativa: 't2' })
  descartarPlano({ tentativa: 't1' })
  assert.equal(consumirPlano(url, 't2'), true, 'cancelar uma tentativa velha não derruba a atual')
  guardarPlano(desktop, { tentativa: 't2' })
  descartarPlano({})
  assert.equal(consumirPlano(url, 't2'), false)
})

test('uma tentativa nova invalida o plano da anterior', () => {
  vivo = 't1'
  guardarPlano(desktop, { tentativa: 't1' })
  vivo = 't2'
  guardarPlano({ url: 'https://login.exemplo.test/b?state=z', callback: 'desktop' }, { tentativa: 't2' })
  assert.equal(consumirPlano(url, 't1'), false)
})
