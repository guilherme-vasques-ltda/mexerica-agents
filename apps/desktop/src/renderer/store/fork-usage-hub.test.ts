import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer, Usage } from '../../shared/models'
import type { HostError, HostTransport } from '../client/host-client'

// MX-158 (R2, P2): o `usage` do store não carrega o Hub de origem. Trocar de Hub no ciclo real do BotSync (`hello` RPC
// com outro hostId → `resetMirror`) tem de esquecê-lo antes de o hello do stream novo chegar; no MESMO Hub, offline
// conserva o último conhecido. BotStore, HostClient e o ciclo são reais; só o transporte e a ponte do Electron são falsos.

const bundle = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'`, resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
const settle = async () => { for (let i = 0; i < 12; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })

const ids = (store: { usage: Usage }) => store.usage.providers.map((p) => p.id)
/** Relógio falso do BotSync (DROP_GRACE = 5000 ms mantém "online" depois de o stream cair): só anda quando o teste manda. */
function relogio() {
  let agora = 0, seq = 0
  const fila = new Map<number, { em: number; fn: () => void }>()
  return {
    setTimeout: (fn: () => void, ms = 0) => { fila.set(++seq, { em: agora + ms, fn }); return seq },
    clearTimeout: (id: number) => { fila.delete(id) },
    avancar(ms: number) {
      const fim = agora + ms
      for (;;) {
        const [id, t] = [...fila].sort((a, b) => a[1].em - b[1].em)[0] ?? []
        if (!t || t.em > fim) break
        fila.delete(id!); agora = t.em; t.fn()
      }
      agora = fim
    },
  }
}
const limite = (id: string): Usage => ({ providers: [{ id, name: id, windows: [], source: 'fixture', updatedAt: 1 }] })

/** Um Hub fictício controlável: o `hello` RPC responde o Hub de agora; o stream só emite o que o teste mandar. */
function rede() {
  const clock = relogio()
  const state = { rpc: { hostId: 'hub-a', name: 'Hub A' }, emit: (_e: unknown) => {}, end: (_e: HostError | null) => {}, streams: 0 }
  const transport: HostTransport = {
    async call(method) { return method === 'hello' ? { version: '2.20.3', minApp: '2.20.1', backends: [], ...state.rpc } : {} },
    stream(_path, onData, onEnd) {
      state.streams++
      state.emit = (event) => onData(JSON.stringify(event))
      state.end = onEnd
      return () => {}
    },
  }
  return { state, clock, transport, hello: (hostId: string, usage: Usage) => state.emit({ type: 'hello', hostId, rev: 1, usage }) }
}

async function abrir(net: ReturnType<typeof rede>) {
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date, setTimeout: net.clock.setTimeout, clearTimeout: net.clock.clearTimeout, crypto, btoa, Uint8Array,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const { BotStore } = module.exports as { BotStore: typeof import('./bot-store').BotStore }
  const store = new BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, false, () => net.transport)
  store.start()
  await settle()
  net.hello('hub-a', limite('claude-do-hub-a'))
  await settle()
  return store
}

test('A → B: com o hello do RPC resolvido e o do stream pendente, nenhum limite do Hub A sob o nome do Hub B', async () => {
  const net = rede()
  const store = await abrir(net)
  assert.equal(store.hostName, 'Hub A')
  assert.deepEqual([...ids(store)], ['claude-do-hub-a'])

  net.state.rpc = { hostId: 'hub-b', name: 'Hub B' }
  store.restartStream() // Reconectar: o hello RPC já volta como Hub B; o stream novo abre e o hello dele não chega
  await settle()
  assert.equal(store.hello?.hostId, 'hub-b')
  assert.equal(store.hostName, 'Hub B')
  assert.deepEqual([...ids(store)], [], 'usage do Hub A não pode aparecer sob o nome do Hub B')

  // Quando o hello do stream do Hub B chega, só o dele vale.
  net.hello('hub-b', limite('codex-do-hub-b'))
  await settle()
  assert.deepEqual([...ids(store)], ['codex-do-hub-b'])
  store.retire()
})

test('A → B com o stream novo falhando: o usage do Hub A não volta nem fica', async () => {
  const net = rede()
  const store = await abrir(net)
  net.state.rpc = { hostId: 'hub-b', name: 'Hub B' }
  store.restartStream()
  await settle()
  net.state.end({ status: 502, message: 'bad_gateway' } as HostError) // o stream do Hub B cai antes do hello
  await settle()
  net.clock.avancar(5000) // passa a tolerância de queda (DROP_GRACE)
  assert.equal(store.connection.kind, 'offline')
  assert.equal(store.hostName, 'Hub B')
  assert.deepEqual([...ids(store)], [])
  store.retire()
})

test('mesmo Hub offline: conserva o último usage conhecido', async () => {
  const net = rede()
  const store = await abrir(net)
  net.state.end({ status: 502, message: 'bad_gateway' } as HostError) // cai o stream do Hub A
  await settle()
  assert.equal(store.connection.kind, 'online', 'dentro da tolerância de queda ainda se mostra online')
  net.clock.avancar(5000) // DROP_GRACE: só agora o app fica offline de verdade
  assert.equal(store.connection.kind, 'offline')
  assert.equal(store.hostName, 'Hub A')
  assert.deepEqual([...ids(store)], ['claude-do-hub-a'])
  // Reconectar no mesmo Hub (hello RPC com o mesmo hostId, stream pendente) também conserva.
  store.restartStream()
  await settle()
  assert.deepEqual([...ids(store)], ['claude-do-hub-a'])
  store.retire()
})
