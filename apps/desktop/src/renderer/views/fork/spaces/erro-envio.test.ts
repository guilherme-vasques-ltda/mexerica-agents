import assert from 'node:assert/strict'
import test from 'node:test'
import { avisoOpen, erroDoEnvio, recusaOpen } from './erro-envio.ts'
import { IDIOMAS_CATALOGO } from '../../../lib/fork-i18n/registro.ts'
import { tradutor } from '../../../lib/fork-i18n/index.ts'

test('a recusa da Open explica o motivo; o resto mantém o aviso de envio incerto', () => {
  assert.equal(erroDoEnvio(new Error('open_chat_account_required')), 'spaces.compose.semContaOpen')
  assert.equal(erroDoEnvio(new Error('open_chat_busy')), 'spaces.compose.ocupadoOpen')
  assert.equal(erroDoEnvio(new Error('open_chat_reconnecting')), 'spaces.compose.reconectandoOpen')
  for (const e of [new Error('Hub parado'), new Error('falha: open_chat_busy'), 'open_chat_busy', null]) {
    assert.equal(erroDoEnvio(e), 'spaces.compose.falhou')
  }
})

test('a bolha não enviada só guarda motivo conhecido da Open', () => {
  assert.equal(recusaOpen('spaces.compose.semContaOpen'), 'spaces.compose.semContaOpen')
  assert.equal(recusaOpen('spaces.compose.ocupadoOpen'), 'spaces.compose.ocupadoOpen')
  assert.equal(recusaOpen('spaces.compose.reconectandoOpen'), 'spaces.compose.reconectandoOpen')
  for (const valor of ['spaces.compose.falhou', 'conversa.mensagens.naoEnviada', '', null, undefined, 1]) assert.equal(recusaOpen(valor), null)
})

test('os avisos da Open saem no idioma da janela pelo código; código desconhecido fica com o texto do Hub', () => {
  for (const code of ['open_setup', 'open_chat_cancelled', 'open_chat_tool_refused', 'open_chat_timeout', 'open_chat_too_long', 'open_chat_policy', 'open_codex_unavailable', 'open_codex_platform', 'open_chat_failed']) {
    const chave = avisoOpen(code)
    assert.ok(chave, code)
    const textos = IDIOMAS_CATALOGO.map((idioma) => tradutor(idioma)(chave))
    assert.ok(textos.every((texto) => texto && texto !== chave), code)
    assert.equal(new Set(textos).size, IDIOMAS_CATALOGO.length, code)
  }
  for (const code of ['auth_protocol_invalid', '__proto__', 'toString', '', null, undefined]) assert.equal(avisoOpen(code), null)
})
