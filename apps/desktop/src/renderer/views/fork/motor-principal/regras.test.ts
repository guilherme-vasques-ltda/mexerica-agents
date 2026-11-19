import assert from 'node:assert/strict'
import { test } from 'node:test'
import { LIMIAR_CONFIRMAR_TOKENS, type Escolha } from '../seletor-modelo/regras.ts'
import { confirmarNaPrincipal, estadoDoBotao, pedidoNaPrincipal, usarNaPrincipal, type EngineDaPrincipal } from './regras.ts'

const principal: EngineDaPrincipal = { accountId: null, backend: 'claude', model: 'claude-opus-5-5', modelOptions: { effort: 'high' }, fastMode: false, revision: 7, source: 'botDefault' }
const padrao: Escolha = { accountId: null, backend: 'claude', model: 'claude-sonnet-5-5', modelOptions: { effort: 'high' }, fastMode: false }

test('o botão só aparece com o anúncio engine do Hub e a projeção da principal', () => {
  assert.equal(estadoDoBotao(false, principal, padrao), 'oculto', 'Hub sem anúncio: sem botão')
  assert.equal(estadoDoBotao(true, null, padrao), 'oculto', 'sem engine legível: sem botão')
  assert.equal(estadoDoBotao(true, principal, padrao), 'pronto')
  assert.equal(estadoDoBotao(true, principal, { ...principal }), 'igual')
  assert.equal(estadoDoBotao(true, principal, { ...principal, fastMode: true }), 'pronto')
})

test('o pedido é o sessionEngineSet da C9: principal = sessionId null, CAS e fastMode booleano', () => {
  assert.deepEqual(pedidoNaPrincipal('bot-1', principal, padrao), {
    botId: 'bot-1', sessionId: null, expectedRevision: 7,
    engine: { accountId: null, backend: 'claude', model: 'claude-sonnet-5-5', modelOptions: { effort: 'high' }, fastMode: false },
  })
})

test('conversa grande no mesmo provedor pede a confirmação D375; sem medida, troca direto', () => {
  assert.equal(confirmarNaPrincipal(principal, padrao, LIMIAR_CONFIRMAR_TOKENS), true)
  assert.equal(confirmarNaPrincipal(principal, padrao, LIMIAR_CONFIRMAR_TOKENS - 1), false)
  assert.equal(confirmarNaPrincipal(principal, padrao, null), false)
  assert.equal(confirmarNaPrincipal(principal, { ...padrao, backend: 'codex' }, 200_000), false, 'outro provedor não relê a conversa')
})

test('grava só por sessionEngineSet; sem anúncio não chama o Hub (nada de updateBot)', async () => {
  const chamadas: string[] = []
  const client = { call: async (method: string) => { chamadas.push(method); return {} } }
  await usarNaPrincipal(client, true, 'bot-1', principal, padrao)
  await assert.rejects(usarNaPrincipal(client, false, 'bot-1', principal, padrao), /engine_unavailable/)
  assert.deepEqual(chamadas, ['sessionEngineSet'])
})

test('recusa do Hub vira texto do catálogo, nunca a chave literal (revisão Sol do #459)', async () => {
  const { traduzir } = await import('../../../lib/fork-i18n/index.ts')
  const { erroDaTroca } = await import('../seletor-modelo/regras.ts')
  const { textoDaNota } = await import('./regras.ts')
  const t = (chave: Parameters<typeof textoDaNota>[0] extends (c: infer C) => string ? C : never) => traduzir('pt-BR', chave)
  for (const code of ['engine_owner_only', 'engine_not_allowed', 'session_revision_conflict', 'session_closed', 'engine_unavailable', 'outra_coisa']) {
    const chave = erroDaTroca(new Error(`${code}: detalhe do Hub`))
    const texto = textoDaNota(t, chave)
    assert.notEqual(texto, chave, `${code}: não mostra a chave`)
    assert.ok(!/^conversa\.seletor\./.test(texto), `${code}: texto traduzido`)
    assert.ok(!texto.includes('detalhe do Hub'), `${code}: o texto do Hub não vai à tela`)
  }
  assert.equal(textoDaNota(t, erroDaTroca(new Error('engine_owner_only'))), traduzir('pt-BR', 'conversa.seletor.erro.soDono'))
  assert.equal(textoDaNota(t, null), traduzir('pt-BR', 'botSettings.motor.pronto'))
})
