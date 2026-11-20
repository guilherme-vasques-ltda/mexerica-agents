import assert from 'node:assert/strict'
import test from 'node:test'
import { erroDoEnvio } from './erro-envio.ts'

test('a recusa da Open explica o motivo; o resto mantém o aviso de envio incerto', () => {
  assert.equal(erroDoEnvio(new Error('open_chat_account_required')), 'spaces.compose.semContaOpen')
  assert.equal(erroDoEnvio(new Error('open_chat_busy')), 'spaces.compose.ocupadoOpen')
  for (const e of [new Error('Hub parado'), new Error('falha: open_chat_busy'), 'open_chat_busy', null]) {
    assert.equal(erroDoEnvio(e), 'spaces.compose.falhou')
  }
})
