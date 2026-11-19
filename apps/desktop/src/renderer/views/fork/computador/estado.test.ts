import assert from 'node:assert/strict'
import test from 'node:test'
import type { BotStore } from '../../../store/bot-store.ts'
import { saveBotSettingsIntent } from '../../../store/fork-bot-settings-intents.ts'
import { controleDoComputador, estadoDoComputador, moverFocoAoDesligar, mudarComputador, podeMudarComputador, TEXTO_DISPONIVEL, TEXTO_LIGADO_SEM_TELA, TEXTO_PERMISSAO, TEXTO_SEM_TELA } from './estado.ts'

const tela = (computerUse: boolean, pronta = true, platform = 'linux') =>
  ({ computerUse, platform, enabled: computerUse, connected: pronta, capture: pronta, input: pronta })
const ligado = { computer: true }
const desligado = { computer: false }

test('os três estados vêm da tela informada, e tela nula conta como sem tela', () => {
  assert.equal(estadoDoComputador(tela(true), desligado), 'disponivel')
  assert.equal(estadoDoComputador(tela(true), ligado), 'disponivel')
  assert.equal(estadoDoComputador(tela(false), ligado), 'ligadoSemTela')
  assert.equal(estadoDoComputador(tela(false), desligado), 'semTela')
  assert.equal(estadoDoComputador(null, ligado), 'ligadoSemTela')
  assert.equal(estadoDoComputador(null, desligado), 'semTela')
  assert.equal(estadoDoComputador(undefined, desligado), 'semTela')
})

test('bot ligado nunca perde o interruptor, com ou sem tela e conexão', () => {
  for (const screen of [tela(true), tela(true, false), tela(false), tela(false, false), null]) {
    for (const online of [true, false]) assert.equal(controleDoComputador(screen, ligado, online).mostraInterruptor, true)
  }
  assert.equal(controleDoComputador(tela(false), desligado, true).mostraInterruptor, false)
  assert.equal(controleDoComputador(null, desligado, true).mostraInterruptor, false)
})

test('desconectado, o controle fica em leitura', () => {
  assert.equal(controleDoComputador(tela(true), desligado, false).desabilitado, true)
  assert.equal(controleDoComputador(tela(false), ligado, false).desabilitado, true)
  assert.equal(controleDoComputador(tela(true), desligado, true).desabilitado, false)
})

test('ajuda: permissão só como dica, e nada de Hub, MCP ou tela remota', () => {
  assert.equal(controleDoComputador(tela(true), desligado, true).ajuda, TEXTO_DISPONIVEL)
  assert.equal(controleDoComputador(tela(true, false), desligado, true).ajuda, `${TEXTO_DISPONIVEL} ${TEXTO_PERMISSAO}`)
  assert.equal(controleDoComputador(tela(true, false, 'windows'), desligado, true).ajuda, TEXTO_DISPONIVEL)
  assert.equal(controleDoComputador(tela(false), ligado, true).ajuda, TEXTO_LIGADO_SEM_TELA)
  assert.equal(controleDoComputador(null, desligado, true).ajuda, TEXTO_SEM_TELA)
  for (const texto of [TEXTO_DISPONIVEL, TEXTO_PERMISSAO, TEXTO_LIGADO_SEM_TELA, TEXTO_SEM_TELA]) {
    assert.doesNotMatch(texto, /\bHub\b|MCP|tela remota|remote/i)
  }
})

test('o editor envia só {computer} ao updateBot', async () => {
  const calls: unknown[] = []
  const store = { ready: async () => ({ call: async (method: string, body: unknown) => { calls.push({ method, body }); return {} } }) } as unknown as BotStore
  await saveBotSettingsIntent(store, 'a', { computer: false })
  assert.deepEqual(calls, [{ method: 'updateBot', body: { id: 'a', computer: false } }])
})

test('só online e sem versão divergente libera o controle; conectando ou sem par fica em leitura', () => {
  const kinds = ['unpaired', 'connecting', 'online', 'computerOffline', 'offline', 'unauthorized'] as const
  for (const kind of kinds) {
    const pode = podeMudarComputador({ connection: { kind }, mismatch: null })
    assert.equal(pode, kind === 'online', kind)
    assert.equal(controleDoComputador(tela(false), ligado, pode).desabilitado, kind !== 'online', kind)
  }
  assert.equal(podeMudarComputador({ connection: { kind: 'online' }, mismatch: { hub: '2.0' } }), false)
})

test('painel: recusa ao desligar mantém o valor confirmado, o interruptor e o erro junto ao controle', async () => {
  const mirror = { computer: true }
  const bots = new Map([['a', mirror]])
  const calls: unknown[] = []
  const store = { bots, ready: async () => ({ call: async (method: string, body: unknown) => {
    calls.push({ method, body })
    throw new Error('Recusado pelo Hub')
  } }) } as unknown as BotStore
  const erro = await mudarComputador((patch) => saveBotSettingsIntent(store, 'a', patch), false)
  assert.equal(erro, 'Recusado pelo Hub')
  assert.deepEqual(calls, [{ method: 'updateBot', body: { id: 'a', computer: false } }])
  assert.equal(bots.get('a'), mirror, 'nada muda o espelho antes do evento do Hub')
  assert.equal(bots.get('a')!.computer, true)
  assert.equal(controleDoComputador(tela(false), bots.get('a')!, true).mostraInterruptor, true)
})

test('painel: confirmação não escreve no espelho; o evento do bot é que muda o estado', async () => {
  const mirror = { computer: true }
  const bots = new Map([['a', mirror]])
  const store = { bots, ready: async () => ({ call: async () => ({ bot: { computer: false } }) }) } as unknown as BotStore
  assert.equal(await mudarComputador((patch) => saveBotSettingsIntent(store, 'a', patch), false), null)
  assert.equal(bots.get('a'), mirror)
  assert.equal(estadoDoComputador(tela(false), { computer: false }), 'semTela', 'depois do evento, a linha toma o lugar do interruptor')
})

test('foco: só depois do desligar confirmado sem tela, e sem tirar o foco de outro lugar', () => {
  assert.equal(moverFocoAoDesligar('ligadoSemTela', 'semTela', true, true), true)
  assert.equal(moverFocoAoDesligar('ligadoSemTela', 'semTela', true, false), false, 'a pessoa já está em outro controle')
  assert.equal(moverFocoAoDesligar('ligadoSemTela', 'semTela', false, true), false, 'mudança vinda de outro cliente')
  assert.equal(moverFocoAoDesligar('disponivel', 'semTela', true, true), false)
  assert.equal(moverFocoAoDesligar('ligadoSemTela', 'disponivel', true, true), false)
})
