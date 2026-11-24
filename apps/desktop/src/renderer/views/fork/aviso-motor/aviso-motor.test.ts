import assert from 'node:assert/strict'
import test from 'node:test'
import { CATALOGOS, tradutor } from '../../../lib/fork-i18n/index.ts'
import { avisoDoMotor } from './aviso-motor.ts'

test('a OpenCode mostra o aviso da D224', () => {
  const chave = avisoDoMotor('opencode')
  assert.ok(chave)
  assert.match(tradutor('pt-BR')(chave), /não cobre tudo/)
  assert.match(tradutor('en')(chave), /doesn't cover everything/)
})

test('os demais motores seguem sem aviso', () => {
  for (const backend of ['codex', 'claude', 'custom', '', 'toString', '__proto__']) assert.equal(avisoDoMotor(backend), null)
})

test('Antigravity avisa antes de escolher o motor nos seis idiomas', () => {
  const chave = avisoDoMotor('antigravity')
  assert.equal(chave, 'extensoes.motor.antigravityPermissoes')
  assert.equal(Object.keys(CATALOGOS).length, 6)
  for (const catalogo of Object.values(CATALOGOS)) assert.ok(catalogo[chave])
  assert.match(tradutor('pt-BR')(chave), /sem mostrar um cartão/)
})
