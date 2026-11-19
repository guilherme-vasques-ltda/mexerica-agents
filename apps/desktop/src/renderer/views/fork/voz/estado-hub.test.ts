import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../../../shared/models'
import type { HostTransport } from '../../../client/host-client'
import type { VoiceStatus } from '../../settings/voice-settings'

// Store, cliente, estado por Hub e formulário reais; só o transporte é fictício.
const bundle = buildSync({
  stdin: {
    contents: `export { BotSync } from './store/bot-sync';
      export { estadoVozHub } from './views/fork/voz/estado-hub';
      export { guardarChave, limparChave } from './views/fork/voz/formulario-chave';
      export { chosenModel, rememberDefaults, saveModel } from './views/settings/voice-settings';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  define: { 'process.env.NODE_ENV': '"production"' },
})

const CHAVE = 'sk-proj-ZYXWVUTSRQPONMLKJIHGFEDCBA9876543210'
const logs: unknown[][] = []

function carregar() {
  const module = { exports: {} }
  const storage = { getItem: () => null, setItem() {} }
  const registrar = (...a: unknown[]) => { logs.push(a) }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, setTimeout, clearTimeout, URLSearchParams,
    console: { log: registrar, info: registrar, warn: registrar, error: registrar, debug: registrar },
    localStorage: storage, window: { addEventListener() {}, codync: { appVersion: '2.20.3' } },
  })
  return module.exports as {
    BotSync: typeof import('../../../store/bot-sync').BotSync
    estadoVozHub: typeof import('./estado-hub').estadoVozHub
    guardarChave: typeof import('./formulario-chave').guardarChave
    limparChave: typeof import('./formulario-chave').limparChave
    chosenModel: typeof import('../../settings/voice-settings').chosenModel
    rememberDefaults: typeof import('../../settings/voice-settings').rememberDefaults
    saveModel: typeof import('../../settings/voice-settings').saveModel
  }
}
const m = carregar()

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => { resolve = r })
  return { promise, resolve }
}

function hub(nome: string, loopback: boolean, responder: (method: string, params: unknown) => Promise<unknown>) {
  const calls: { method: string; params: unknown }[] = []
  const transport: HostTransport = {
    async call(method, params) {
      calls.push({ method, params })
      if (method === 'hello') return { version: '2.20.3', minApp: '2.20.1', name: nome, backends: [] }
      if (method === 'teamSettings') return { primaryBotId: null }
      return responder(method, params)
    },
    stream(_path, onData) {
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: nome, rev: 0, usage: { providers: [] } })))
      return () => {}
    },
  }
  const store = new m.BotSync({ id: nome, name: nome } as Computer, loopback, () => transport)
  store.start()
  return { store, calls, close: () => store.retire() }
}

const so = (provider: 'openai' | 'gemini'): VoiceStatus => ({ providers: [{ provider, configured: true }] })

test('estado por Hub: a resposta de um Hub nunca muda o estado de outro, e leitura antiga é descartada', async () => {
  const lenta = deferred<VoiceStatus>()
  let primeira = true
  const a = hub('servidor', true, async (method) => {
    if (method === 'voiceStatus') { if (primeira) { primeira = false; return lenta.promise } return so('gemini') }
    if (method === 'voiceModels') return { models: [], transcribe: [], speech: [] }
    if (method === 'setVoiceKey') return so('gemini')
    throw new Error(method)
  })
  const b = hub('nuvem', false, async (method) => {
    if (method === 'voiceStatus') return so('openai')
    if (method === 'voiceModels') return { models: [], transcribe: [], speech: [] }
    throw new Error(method)
  })
  try {
    await tick(); await tick()
    const ea = m.estadoVozHub(a.store), eb = m.estadoVozHub(b.store)
    assert.equal(m.estadoVozHub(a.store), ea, 'um estado por Hub, compartilhado pelas telas')
    const pa = ea.observar(), pb = eb.observar()
    await tick(); await tick()
    assert.equal(JSON.stringify(eb.status), JSON.stringify(so('openai')))
    assert.equal(ea.status, null, 'o Hub servidor ainda não respondeu')
    // Uma gravação no Hub servidor invalida a leitura em andamento.
    await m.guardarChave(a.store, a.store.connectedClient, 'gemini', CHAVE, () => {})
    await tick(); await tick()
    assert.equal(JSON.stringify(ea.status), JSON.stringify(so('gemini')))
    lenta.resolve({ providers: [] })
    await tick(); await tick()
    assert.equal(JSON.stringify(ea.status), JSON.stringify(so('gemini')), 'a resposta antiga não volta')
    assert.equal(JSON.stringify(eb.status), JSON.stringify(so('openai')), 'o outro Hub não muda')
    pa(); pb()
  } finally {
    a.close(); b.close()
  }
})

test('setVoiceKey só sai para Hub local/SSH; pela nuvem, nem é chamado', async () => {
  const nuvem = hub('nuvem', false, async () => so('openai'))
  try {
    await tick(); await tick()
    let limpo = 0
    const r = await m.guardarChave(nuvem.store, nuvem.store.connectedClient, 'openai', CHAVE, () => { limpo++ })
    assert.equal(r.ok, false)
    assert.equal(JSON.stringify(r.partes), JSON.stringify([{ chave: 'voz.ajustes.hub.semGestao' }]))
    assert.equal(limpo, 1, 'o campo é limpo mesmo quando recusado')
    const l = await m.limparChave(nuvem.store, nuvem.store.connectedClient, 'openai')
    assert.equal(l.ok, false)
    assert.ok(!nuvem.calls.some((c) => c.method === 'setVoiceKey'), 'nenhum setVoiceKey pela nuvem')
  } finally {
    nuvem.close()
  }
})

test('a chave sai do campo antes da rede e não aparece em resultado, erro ou log', async () => {
  const resposta = deferred<VoiceStatus>()
  let recusar = false
  const local = hub('servidor', true, async (method, params) => {
    if (method === 'setVoiceKey') {
      if (recusar) throw new Error(`OpenAI rejected the key ${(params as { key: string }).key}`)
      return resposta.promise
    }
    if (method === 'voiceStatus') return { providers: [] }
    throw new Error(method)
  })
  try {
    await tick(); await tick()
    let campo = CHAVE
    const pendente = m.guardarChave(local.store, local.store.connectedClient, 'openai', campo, () => { campo = '' })
    await tick()
    assert.equal(campo, '', 'limpo antes de o Hub responder')
    const enviado = local.calls.find((c) => c.method === 'setVoiceKey')
    assert.equal(JSON.stringify(enviado?.params), JSON.stringify({ provider: 'openai', key: CHAVE }), 'a chave vai só ao Hub, pelo setVoiceKey de sempre')
    resposta.resolve(so('openai'))
    const ok = await pendente
    assert.equal(ok.ok, true)
    assert.ok(!JSON.stringify(ok).includes(CHAVE.slice(8, 20)))

    recusar = true
    const falha = await m.guardarChave(local.store, local.store.connectedClient, 'openai', CHAVE, () => {})
    assert.equal(falha.ok, false)
    assert.ok(!JSON.stringify(falha.partes).includes(CHAVE.slice(8, 20)), JSON.stringify(falha.partes))
    assert.ok(!logs.some((linha) => JSON.stringify(linha).includes(CHAVE.slice(8, 20))), 'nenhum log com a chave')
  } finally {
    local.close()
  }
})

test('o mini não sobe sozinho quando o Hub indica um padrão completo (preferências reais)', () => {
  // Leitura do padrão embutido: gpt-4o-mini-tts, sem escolha da pessoa.
  assert.equal(m.chosenModel('openai', 'speech'), 'gpt-4o-mini-tts')
  m.rememberDefaults('openai', { realtime: 'gpt-realtime-3', transcribe: 'gpt-transcribe-2', speech: 'gpt-tts-2' })
  assert.equal(m.chosenModel('openai', 'speech'), 'gpt-4o-mini-tts', 'o mini em uso fica')
  assert.equal(m.chosenModel('openai', 'realtime'), 'gpt-realtime-3', 'completo segue o mais novo')
  // Mini escolhido igual ao padrão do Gemini fica gravado e não sobe depois.
  m.rememberDefaults('gemini', { realtime: 'gemini-3.9-live-lite', transcribe: 'gemini-3.8-flash', speech: 'gemini-3.8-flash-tts' })
  m.saveModel('gemini', 'realtime', 'gemini-3.9-live-lite')
  m.rememberDefaults('gemini', { realtime: 'gemini-4-live', transcribe: 'gemini-3.8-flash', speech: 'gemini-3.8-flash-tts' })
  assert.equal(m.chosenModel('gemini', 'realtime'), 'gemini-3.9-live-lite')
})


test('abrir observação com provedor configurado consulta modelos; observadores compartilham e desmontagem encerra', async () => {
  const local = hub('hub-teste', true, async (method) => {
    if (method === 'voiceStatus') return so('openai')
    if (method === 'voiceModels') return { models: ['modelo-ficticio'], transcribe: [], speech: [] }
    throw new Error(method)
  })
  try {
    await tick(); await tick()
    const estado = m.estadoVozHub(local.store)
    assert.equal(local.calls.filter(c => c.method === 'voiceStatus').length, 0)
    const parar = estado.observar()
    await tick(); await tick()
    assert.deepEqual(local.calls.filter(c => c.method.startsWith('voice')).map(c => c.method), ['voiceStatus', 'voiceModels'])
    assert.equal(JSON.stringify(local.calls.find(c => c.method === 'voiceModels')?.params), JSON.stringify({ provider: 'openai' }))
    const pararOutro = estado.observar()
    await tick()
    assert.equal(local.calls.filter(c => c.method === 'voiceModels').length, 1, 'a segunda tela compartilha a consulta')
    parar(); pararOutro()
    estado.atualizar()
    await tick()
    assert.equal(local.calls.filter(c => c.method === 'voiceStatus').length, 1, 'sem observadores não consulta')
    const pararReabertura = estado.observar()
    await tick(); await tick()
    pararReabertura()
    assert.equal(local.calls.filter(c => c.method === 'voiceModels').length, 2, 'reabrir pode consultar novamente')
    assert.ok(!local.calls.some(c => ['voiceSession', 'voiceSpeak', 'voiceTranscribe', 'setVoiceKey'].includes(c.method)))
  } finally { local.close() }
})

test('observação sem provedor configurado não solicita modelos', async () => {
  const local = hub('hub-sem-provedor', true, async (method) => {
    if (method === 'voiceStatus') return { providers: [] }
    throw new Error(method)
  })
  try {
    await tick(); await tick()
    const parar = m.estadoVozHub(local.store).observar()
    await tick(); await tick()
    parar()
    assert.deepEqual(local.calls.filter(c => c.method.startsWith('voice')).map(c => c.method), ['voiceStatus'])
  } finally { local.close() }
})
