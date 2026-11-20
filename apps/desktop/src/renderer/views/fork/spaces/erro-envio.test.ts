import assert from 'node:assert/strict'
import test from 'node:test'
import { erroDoEnvio, recusaOpen } from './erro-envio.ts'

test('a recusa da Open explica o motivo; o resto mantém o aviso de envio incerto', () => {
  assert.equal(erroDoEnvio(new Error('open_chat_account_required')), 'spaces.compose.semContaOpen')
  assert.equal(erroDoEnvio(new Error('open_chat_busy')), 'spaces.compose.ocupadoOpen')
  for (const e of [new Error('Hub parado'), new Error('falha: open_chat_busy'), 'open_chat_busy', null]) {
    assert.equal(erroDoEnvio(e), 'spaces.compose.falhou')
  }
})

test('a bolha não enviada só guarda motivo conhecido da Open', () => {
  assert.equal(recusaOpen('spaces.compose.semContaOpen'), 'spaces.compose.semContaOpen')
  assert.equal(recusaOpen('spaces.compose.ocupadoOpen'), 'spaces.compose.ocupadoOpen')
  for (const valor of ['spaces.compose.falhou', 'conversa.mensagens.naoEnviada', '', null, undefined, 1]) assert.equal(recusaOpen(valor), null)
})
