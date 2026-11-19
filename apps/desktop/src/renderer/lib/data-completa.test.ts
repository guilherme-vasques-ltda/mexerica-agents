import { test } from 'node:test'
import assert from 'node:assert/strict'
import { dataCompleta } from './data-completa.ts'

test('a data completa da mensagem sai em pt-BR, qualquer que seja o idioma do sistema', () => {
  assert.equal(dataCompleta(new Date(2026, 10, 4, 0, 41).getTime()), '4 de nov. de 2026, 00:41')
})
