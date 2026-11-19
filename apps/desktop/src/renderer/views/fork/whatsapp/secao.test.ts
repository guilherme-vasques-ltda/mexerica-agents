import assert from 'node:assert/strict'
import test from 'node:test'
import { estadoDe, type Chamador } from './estado.ts'
import { pluginLigado, secaoValida, secoesVisiveis } from './secao.ts'

// D350: a seção WhatsApp das Configurações do bot só com o plugin ligado em Ajustes → Plugins.
const SECOES = [['identity', 'Identidade'], ['engine', 'Motor'], ['whatsapp', 'WhatsApp'], ['memory', 'Memória']] as const
const leitura = (enabled: boolean) => ({ plugin: { enabled, mode: 'per_bot' as const }, transcription: { provider: null, model: null, source: 'none' as const, keyOnHub: false }, bots: [] })

test('plugin desligado: sem o item e sem a seção WhatsApp; sem leitura conta como desligado', () => {
  assert.deepEqual(secoesVisiveis(SECOES, pluginLigado(leitura(false))).map(([id]) => id), ['identity', 'engine', 'memory'])
  assert.deepEqual(secoesVisiveis(SECOES, pluginLigado(null)).map(([id]) => id), ['identity', 'engine', 'memory'])
})

test('plugin ligado: o item e a seção WhatsApp aparecem no lugar de sempre', () => {
  assert.deepEqual(secoesVisiveis(SECOES, pluginLigado(leitura(true))).map(([id]) => id), ['identity', 'engine', 'whatsapp', 'memory'])
})

test('estando na seção WhatsApp e o plugin desligando, volta para a primeira seção; as outras ficam', () => {
  assert.equal(secaoValida('whatsapp', secoesVisiveis(SECOES, false)), 'identity')
  assert.equal(secaoValida('whatsapp', secoesVisiveis(SECOES, true)), 'whatsapp')
  assert.equal(secaoValida('memory', secoesVisiveis(SECOES, false)), 'memory')
})

test('troca ao vivo: ligar e desligar em Plugins muda na hora a leitura que as Configurações do bot usam', async () => {
  let enabled = false
  const chamador: Chamador = { call: async <T,>(method: string, body?: unknown) => {
    if (method === 'donnaWhatsappSave') enabled = (body as { enabled: boolean }).enabled
    return leitura(enabled) as T
  } }
  const loja = {}
  const plugins = estadoDe(loja, () => chamador)
  const configuracoes = estadoDe(loja, () => chamador)
  assert.equal(plugins, configuracoes)
  await configuracoes.ler()
  assert.equal(pluginLigado(configuracoes.leitura), false)
  await plugins.salvarHub({ enabled: true })
  assert.equal(pluginLigado(configuracoes.leitura), true)
  await plugins.salvarHub({ enabled: false })
  assert.equal(pluginLigado(configuracoes.leitura), false)
  assert.notEqual(estadoDe({}, () => chamador), plugins)
})
