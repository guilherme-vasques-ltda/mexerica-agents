import { test } from 'node:test'
import assert from 'node:assert/strict'
import { erroDaDonna } from './fork-donna-erros.ts'
import { CATALOGOS } from './fork-i18n/index.ts'

test('recusas da Donna fixa viram texto do catálogo (sem DOM, no padrão inglês); o resto passa intacto', () => {
  assert.equal(erroDaDonna('donna_fixed: A Donna é fixa; só a aparência muda.'), CATALOGOS.en['botSettings.donnaFixa.erro.fixa'])
  assert.equal(erroDaDonna('donna_reserved: x'), CATALOGOS.en['botSettings.donnaFixa.erro.reservado'])
  assert.equal(erroDaDonna('settings_conflict: x'), null)
  assert.equal(erroDaDonna('donna_fixedx'), null)
})
