import assert from 'node:assert/strict'
import { test } from 'node:test'
import { envAlias } from './env-alias.ts'

test('#915 nome novo vence, antigo continua valendo e avisa uma vez', () => {
  const avisos: string[] = []
  const warn = (m: string) => avisos.push(m)
  assert.equal(envAlias({}, 'MEXERICA_A', 'DONNABOT_A', warn), undefined)
  assert.equal(envAlias({ MEXERICA_A: 'novo', DONNABOT_A: 'velho' }, 'MEXERICA_A', 'DONNABOT_A', warn), 'novo')
  assert.equal(avisos.length, 0)
  // instalação com configuração antiga: só o nome antigo definido
  assert.equal(envAlias({ DONNABOT_A: 'velho' }, 'MEXERICA_A', 'DONNABOT_A', warn), 'velho')
  assert.equal(envAlias({ MEXERICA_A: '', DONNABOT_A: 'velho' }, 'MEXERICA_A', 'DONNABOT_A', warn), 'velho')
  assert.equal(avisos.length, 1)
  assert.match(avisos[0], /DONNABOT_A.*MEXERICA_A/)
})
