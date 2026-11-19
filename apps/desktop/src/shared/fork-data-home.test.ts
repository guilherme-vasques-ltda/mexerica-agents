import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mexericaHome } from './fork-data-home.ts'

test('D520 B1 seleciona padrão e override, recusando configurações antigas antes de I/O', () => {
  assert.equal(mexericaHome('/synthetic', '.mexerica', {}), '/synthetic/.mexerica')
  assert.equal(mexericaHome('/synthetic', '.mexerica-dev', {}), '/synthetic/.mexerica-dev')
  assert.equal(mexericaHome('/synthetic', '.mexerica', { MEXERICA_HOME: '/synthetic/override' }), '/synthetic/override')
  for (const key of ['DONNABOT_HOME', 'CODYNC_HOME']) {
    for (const value of ['', '/synthetic/old', '/synthetic/override']) {
      assert.throws(() => mexericaHome('/synthetic', '.mexerica', { [key]: value }))
      assert.throws(() => mexericaHome('/synthetic', '.mexerica', { [key]: value, MEXERICA_HOME: '/synthetic/override' }))
    }
  }
  for (const path of ['', '/synthetic/.DONNABOT-dev', '/SYNTHETIC/.DONNABOT-dev', '/synthetic/.CODYNC/data']) {
    assert.throws(() => mexericaHome('/synthetic', '.mexerica', { MEXERICA_HOME: path }))
  }
})
