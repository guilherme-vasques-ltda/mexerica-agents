import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DONNABOT_HUB_HOST } from '../../shared/ssh.ts'
import { hubSSHAlias } from './hub-alias.ts'

const com = (valor: string) => hubSSHAlias({ MEXERICA_HUB_SSH_ALIAS: valor })

test('padrão genérico quando nada está definido', () => {
  assert.equal(DONNABOT_HUB_HOST, 'mexerica-hub')
  assert.equal(hubSSHAlias({}), 'mexerica-hub')
  assert.equal(com(''), 'mexerica-hub')
})

test('MEXERICA_HUB_SSH_ALIAS válido sobrescreve o padrão', () => {
  assert.equal(com('meu-hub'), 'meu-hub')
  assert.equal(com('Hub_2.casa'), 'Hub_2.casa')
  assert.equal(com('a'), 'a')
})

test('caracteres fora de [A-Za-z0-9._-] voltam ao padrão', () => {
  for (const ruim of ['meu;hub', 'meu hub', 'meu\thub', 'meu/hub', 'meu@hub', 'meu:22', 'hub*', 'hub?', 'hub$(id)', 'hub`id`', 'hub"x', "hub'x", 'hub|x', 'hub&x', 'hub\\x', 'hubç', 'hub\u0000x'])
    assert.equal(com(ruim), 'mexerica-hub', ruim)
})

test('espaços nas pontas ou só espaços voltam ao padrão (sem trim)', () => {
  assert.equal(com('   '), 'mexerica-hub')
  assert.equal(com('  meu-hub  '), 'mexerica-hub')
  assert.equal(com('meu-hub\n'), 'mexerica-hub')
})

test('prefixo hífen (opção do ssh) volta ao padrão', () => {
  assert.equal(com('-oProxyCommand=x'), 'mexerica-hub')
  assert.equal(com('-'), 'mexerica-hub')
  assert.equal(com('--help'), 'mexerica-hub')
  assert.equal(com('meu-hub-'), 'meu-hub-')
})
