import assert from 'node:assert/strict'
import test from 'node:test'
import { tradutor } from '../../../lib/fork-i18n/index.ts'
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
