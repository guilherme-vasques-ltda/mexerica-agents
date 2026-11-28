import { test } from 'node:test'
import assert from 'node:assert/strict'
import { limparRuidoDeDitado } from './limpar-ruido.ts'

test('tira sons entre colchetes e asteriscos', () => {
  assert.equal(limparRuidoDeDitado('[BLANK_AUDIO]'), '')
  assert.equal(limparRuidoDeDitado('*Loud noise*'), '')
  assert.equal(limparRuidoDeDitado('oi [música] tudo *aplausos* bem'), 'oi tudo bem')
})

test('mantém contas, parênteses e texto normal', () => {
  assert.equal(limparRuidoDeDitado('quanto é 2 * 3 * 4'), 'quanto é 2 * 3 * 4')
  assert.equal(limparRuidoDeDitado('feito (rápido) e bem'), 'feito (rápido) e bem')
  assert.equal(limparRuidoDeDitado('olá, tudo bem?'), 'olá, tudo bem?')
})
