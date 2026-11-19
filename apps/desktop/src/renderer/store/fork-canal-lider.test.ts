import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Bot, Computer } from '../../shared/models'
import type { HostTransport } from '../client/host-client'

// R350-2: tirar o líder do canal é recusado pelo Hub; o canal mostrado volta ao que era. BotStore e HostClient são
// reais; só o transporte e a ponte do Electron são fictícios (como em fork/computador/screen-epoch.test.ts).

const bundle = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'; export { normalizeBot } from '../shared/models'`, resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
type Bundle = { BotStore: typeof import('./bot-store').BotStore; normalizeBot: typeof import('../../shared/models').normalizeBot }
const settle = async () => { for (let i = 0; i < 8; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
const SWAP_FIRST = 'Troque o líder do canal antes de tirar esse bot.'

test('R350-2: removeMember recusado pelo Hub devolve o canal com o líder', async () => {
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date, setTimeout, clearTimeout,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const { BotStore, normalizeBot } = module.exports as Bundle
  const sent: unknown[] = []
  const transport: HostTransport = {
    async call(method, body) {
      if (method === 'hello') return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
      if (method === 'updateBot') { sent.push(body); throw new Error(SWAP_FIRST) }
      return {}
    },
    stream(_path, onData) {
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 1, usage: { providers: [] } })))
      return () => {}
    },
  }
  const store = new BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => transport)
  store.start()
  try {
    await settle()
    const group: Bot = normalizeBot({ id: 'g', kind: 'group', name: 'Casa', members: ['a', 'b'], leader: 'b' })
    store.bots.set('g', group)
    store.removeMember(group, 'b')
    assert.deepEqual(store.bots.get('g')?.members, ['a'], 'o app mostra a saída na hora')
    await settle()
    assert.equal(sent.length, 1, 'o pedido saiu uma vez')
    assert.equal(store.bots.get('g'), group, 'recusado, o canal volta com o líder')
    assert.ok(store.lastError?.includes(SWAP_FIRST), 'a recusa aparece') // o Error vem de outro realm (vm)
  } finally { store.retire() }
})
