import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { EstadoChaves, VozBridge } from '../../../shared/fork-voz'
import type { Computer } from '../../../shared/models'
import type { HostTransport } from '../../client/host-client'
import type { VoiceStatus } from '../settings/voice-settings'

// O store, o cliente e o estado de voz são reais; só o transporte e a ponte são fictícios.
const bundle = buildSync({
  stdin: {
    contents: `export { EstadoVozAparelho } from './views/fork/voz/estado-aparelho';
      export { BotSync } from './store/bot-sync';
      export { CallAvailabilityState } from './views/call/call-availability-state';
      export { setVoiceKey } from './views/settings/voice-settings';`,
    resolveDir: new URL('../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  define: { 'process.env.NODE_ENV': '"production"' },
})
const empty: VoiceStatus = { providers: [] }
const gemini: VoiceStatus = { providers: [{ provider: 'gemini', configured: true }] }
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => { resolve = r })
  return { promise, resolve }
}

function fixture() {
  const module = { exports: {} }
  const storage = { getItem: () => null, setItem() {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, setTimeout, clearTimeout, URLSearchParams,
    localStorage: storage, window: { addEventListener() {}, codync: { appVersion: '2.20.3' } },
  })
  const { BotSync, CallAvailabilityState, setVoiceKey, EstadoVozAparelho } = module.exports as {
    EstadoVozAparelho: typeof import('../fork/voz/estado-aparelho').EstadoVozAparelho
    BotSync: typeof import('../../store/bot-sync').BotSync
    CallAvailabilityState: typeof import('./call-availability-state').CallAvailabilityState
    setVoiceKey: typeof import('../settings/voice-settings').setVoiceKey
  }
  const calls: string[] = []
  let version = '2.20.3'
  let read = async () => gemini
  let write = async () => empty
  let end!: Parameters<HostTransport['stream']>[2]
  let emit!: Parameters<HostTransport['stream']>[1]
  const transport: HostTransport = {
    async call(method) {
      calls.push(method)
      if (method === 'hello') return { version, minApp: version, name: 'Fixture', backends: [] }
      if (method === 'teamSettings') return { primaryBotId: null }
      if (method === 'voiceStatus') return read()
      if (method === 'setVoiceKey') return write()
      throw new Error(`Operação não autorizada na fixture: ${method}`)
    },
    stream(_path, onData, onEnd) {
      end = onEnd; emit = onData
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'fixture', rev: 0, usage: { providers: [] } })))
      return () => {}
    },
  }
  const store = new BotSync({ id: 'fixture', name: 'Fixture' } as Computer, false, () => transport)
  const state = new CallAvailabilityState(store)
  const stop = state.start(false)
  store.start()
  return {
    store, state, calls, setVoiceKey, EstadoVozAparelho,
    version(next: string) { version = next },
    read(next: typeof read) { read = next }, write(next: typeof write) { write = next },
    drop() { end(null) },
    streamEnd() { return end },
    entry() { emit(JSON.stringify({ type: 'usage', usage: { providers: [] } })) },
    available: () => state.available(false, 'gemini'),
    close() { stop(); store.retire() },
  }
}

test('link real invalida na queda curta, inclusive EOF, e reconsulta na geração nova', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  try {
    await tick()
    assert.equal(f.available(), true)
    const client = f.store.client
    const count = f.calls.filter((m) => m === 'voiceStatus').length
    f.entry(); await tick()
    assert.equal(f.calls.filter((m) => m === 'voiceStatus').length, count, 'eventos comuns não repetem a consulta')
    f.read(async () => empty)
    f.drop(); await tick()
    assert.equal(f.store.connection.kind, 'online', 'a tolerância visual continua')
    assert.equal(f.available(), false, 'o transporte já caiu, mesmo sem render intermediário')
    t.mock.timers.tick(1100); await tick()
    assert.notEqual(f.store.client, client)
    assert.equal(f.available(), false)
    assert.equal(f.calls.filter((m) => m === 'voiceStatus').length, count + 1)
    f.read(async () => gemini)
    f.drop(); await tick(); t.mock.timers.tick(1100); await tick()
    assert.equal(f.available(), true, 'provedor novo é descoberto após reconexão')
  } finally { f.close() }
})

test('remoção pendente invalida antes da notificação e leitura antiga não restaura voz', async () => {
  const f = fixture()
  try {
    await tick()
    const old = deferred<VoiceStatus>()
    f.read(() => old.promise)
    f.state.refresh()
    const mutation = deferred<VoiceStatus>()
    f.write(() => mutation.promise)
    const save = f.setVoiceKey(f.store.client!, 'gemini', '')
    assert.equal(f.available(), false)
    old.resolve(gemini); await tick()
    assert.equal(f.available(), false, 'resposta da leitura anterior à gravação é descartada')
    const count = f.calls.filter((m) => m === 'voiceStatus').length
    f.entry(); await tick()
    assert.equal(f.calls.filter((m) => m === 'voiceStatus').length, count, 'não consulta durante a gravação')
    f.read(async () => empty)
    mutation.resolve(empty); await save; await tick()
    assert.equal(f.available(), false)
    assert.equal(f.calls.filter((m) => m === 'voiceStatus').length, count + 1)
  } finally { f.close() }
})

test('respostas de conexão/cliente anterior e falha de voz não reabilitam ações', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  try {
    await tick()
    const oldConnection = deferred<VoiceStatus>()
    f.read(() => oldConnection.promise); f.state.refresh()
    f.drop(); await tick()
    f.read(async () => empty)
    t.mock.timers.tick(1100); await tick()
    oldConnection.resolve(gemini); await tick()
    assert.equal(f.available(), false)
    const oldClient = deferred<VoiceStatus>()
    f.read(() => oldClient.promise); f.state.refresh()
    f.read(async () => { throw new Error('Falha sintética') })
    const endPrevious = f.streamEnd()
    f.store.restartStream()
    oldClient.resolve(gemini); await tick()
    endPrevious(null); await tick()
    assert.ok(f.store.connectedClient, 'fim tardio do stream antigo não derruba o cliente novo')
    assert.equal(f.available(), false)
    assert.equal(f.state.available(true, 'gemini'), true, 'voz local continua sendo fallback válido')
    f.version('9999.0.0'); f.store.restartStream(); await tick()
    assert.equal(f.state.available(true, 'gemini'), false, 'contrato incompatível bloqueia até o fallback local')
  } finally { f.close() }
})

test('seleção e encerramento preservam fronteiras sem trocar de provedor implicitamente', async () => {
  const f = fixture()
  try {
    await tick()
    assert.equal(f.state.available(false, 'gemini'), true)
    assert.equal(f.state.available(false, 'openai'), false)
    assert.equal(f.state.available(false, null), false, 'voz-03: device sem voz local não escolhe provedor pago sozinho')
    const late = deferred<VoiceStatus>()
    f.read(() => late.promise); f.state.refresh()
    f.close(); late.resolve(gemini); await tick()
    assert.equal(f.available(), false)
  } finally { f.close() }
})

test('guardar e limpar a chave do aparelho atualizam a ação observada sem reabrir e ainda exigem o Hub', async () => {
  const f = fixture()
  let estado: EstadoChaves = { cofre: 'seguro', chaves: { openai: { situacao: 'ausente', reserva: false }, gemini: { situacao: 'ausente', reserva: false } } }
  const ouvintes = new Set<() => void>()
  const mudar = (situacao: 'guardada' | 'ausente') => {
    estado = { ...estado, chaves: { ...estado.chaves, gemini: { situacao, reserva: false } } }
    ouvintes.forEach(f => f())
    return Promise.resolve({ ok: true, categoria: null, estado })
  }
  const ponte = { estado: async () => estado, onChange: (f: () => void) => { ouvintes.add(f); return () => { ouvintes.delete(f) } },
    guardar: () => mudar('guardada'), limpar: () => mudar('ausente') } as unknown as VozBridge
  const aparelho = new f.EstadoVozAparelho(() => ponte)
  const parar = aparelho.observar()
  const disponivel = () => f.state.available(false, 'gemini', aparelho.estado?.chaves)
  const acoes: boolean[] = []
  const off = aparelho.subscribe(() => acoes.push(disponivel()))
  try {
    f.read(async () => empty)
    await tick()
    assert.equal(disponivel(), false, 'Hub sem chave e aparelho sem chave')
    await ponte.guardar('gemini', 'demo-chave-ficticia'); await tick()
    assert.equal(disponivel(), true, 'guardar habilita a chamada com chave só daqui')
    assert.equal(acoes.at(-1), true, 'o estado observado publica a mudança para a tela')
    await ponte.limpar('gemini'); await tick()
    assert.equal(disponivel(), false, 'Limpar retira a ação sem remontar o observador')
    assert.equal(acoes.at(-1), false)
    await ponte.guardar('gemini', 'demo-chave-ficticia'); await tick()
    f.drop(); await tick()
    assert.equal(disponivel(), false, 'chave daqui não dispensa o Hub conectado')
  } finally { off(); parar(); f.close() }
})
