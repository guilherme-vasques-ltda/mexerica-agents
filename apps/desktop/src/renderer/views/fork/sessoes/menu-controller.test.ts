import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { HostTransport } from '../../../client/host-client'
import type { Bot } from '../../../../shared/models'

// AppModel.start -> setHost -> new BotStore -> LoopbackTransport -> HostClient reais.
// Só ponte Electron e Host em memória são falsos. Nada usa inSpace nem constructor alternativo.
const code = buildSync({ stdin: { contents: `
  export { AppModel } from './store/app-model';
  export { normalizeBot } from '../shared/models';
  export { mirrorCacheStamp } from '../shared/fork-mirror-cache';
  export { createSessionMenuController } from './views/fork/sessoes/menu-controller';`,
  resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"' },
}).outputFiles[0]!.text
const settle = async () => { for (let i = 0; i < 15; i++) await tick() }
const plain = (v: unknown) => JSON.parse(JSON.stringify(v))
const row = () => ({ id: 'extra', chatId: 'byte', principal: false, state: 'open', title: 'Extra', revision: 1 })
const snapshot = { version: 1, personalSpaceId: 'pessoal-real', revision: 1,
  spaces: [{ id: 'pessoal-real', kind: 'personal' }, { id: 'space-b', kind: 'space' }] }
function fakeHost(space?: string) {
  const state = { space, identified: true, actions: ['close'], holdDelete: null as Promise<void> | null, deleted: false, row: row(), holdHello: null as Promise<void> | null, holdClose: null as Promise<void> | null }
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const transport: HostTransport = {
    async call(method, raw) {
      const body = raw as Record<string, unknown>; calls.push({ method, body })
      if (method === 'hello') {
        await state.holdHello
        return { hostId: 'mesmo-hub', name: 'Sintético', version: '2.20.3', minApp: '2.20.1', backends: [],
          ...(state.space ? { spaces: structuredClone(snapshot), ...(state.identified ? { spacesContract: 1, spaceId: state.space } : {}) } : {}) }
      }
      if (method === 'sessions') return { sessions: state.deleted ? [] : [{ ...state.row }], sessionActions: state.actions }
      if (method === 'sessionDelete') {
        assert.equal(body.hubId, 'mesmo-hub')
        assert.equal(body.spaceId, state.space)
        assert.equal(body.sessionId, 'extra')
        assert.equal(body.expectedRevision, 1)
        await state.holdDelete
        state.deleted = true
        return { type: 'sessionDeleted', spaceId: state.space, chatId: 'byte', sessionId: 'extra', rev: 3 }
      }
      if (method === 'sessionClose') {
        assert.equal(body.hubId, 'mesmo-hub')
        assert.equal(body.spaceId, state.space, 'contexto original explícito no cliente de produção sem inSpace')
        assert.equal(body.sessionId, 'extra')
        await state.holdClose
        state.row = { ...state.row, state: 'closed', revision: 2 }
        return { session: { ...state.row } }
      }
      return {}
    },
    stream(_path, receive) {
      queueMicrotask(() => receive(JSON.stringify({ type: 'hello', hostId: 'mesmo-hub', rev: 1, screen: null, usage: { providers: [] } })))
      return () => {}
    },
  }
  return { state, calls, transport }
}
async function factory(fake: ReturnType<typeof fakeHost>, cached = false) {
  const module = { exports: {} }
  const bridge: unknown = new Proxy(function () {}, { get: (_t, k) => k === 'then' ? undefined : bridge, apply: () => bridge })
  let cache: string | null = null
  // E2: a fixture usa o cache atual, identificado por Hub/Space; expectativas de Copiar/Arquivar não mudam.
  const storage = { getItem: (k: string) => k === 'codync-mirror-hub-["local","device"]' && cached ? 'mesmo-hub'
    : k === 'codync-mirror-["local","device","mesmo-hub",null]' ? cache : null, setItem() {}, removeItem() {} }
  runInNewContext(code, { module, queueMicrotask, URLSearchParams, URL, Date, setTimeout, clearTimeout, crypto, btoa, Uint8Array,
    localStorage: storage, document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, location: { search: '' }, codync: Object.assign(Object.create(bridge as object), {
      appVersion: '2.20.3', platform: 'darwin',
      host: { snapshot: async () => ({ state: { kind: 'running' }, dev: false, logPath: '', local: { computerId: 'device', baseURL: 'http://fixture.invalid', token: '' } }),
        onChange() {}, call: (_url: string, _token: string, method: string, body: unknown) => fake.transport.call(method, body, 1000),
        stream: (_url: string, _token: string, receive: (s: string) => void) => fake.transport.stream('', receive, () => {}) },
      app: { onCommand() {}, syncScreenAgent: async () => ({ needsApproval: false }) },
    }) },
  })
  const exports = module.exports as {
    AppModel: typeof import('../../../store/app-model').AppModel
    normalizeBot: typeof import('../../../../shared/models').normalizeBot
    mirrorCacheStamp: typeof import('../../../../shared/fork-mirror-cache').mirrorCacheStamp
    createSessionMenuController: typeof import('./menu-controller').createSessionMenuController
  }
  if (cached) cache = JSON.stringify({ stamp: exports.mirrorCacheStamp('2.20.3', '2.20.3'), hostId: 'mesmo-hub', rev: 1, bots: [], entries: [], spaces: snapshot })
  const app = new exports.AppModel()
  await app.start()
  const store = app.local!
  assert.ok(store)
  assert.equal(store.spaceId, undefined, 'mesma factory de produção, sem Space no constructor')
  store.bots.set('byte', exports.normalizeBot({ id: 'byte', name: 'Byte', backend: 'codex' }))
  let current = { store, chat: store.bots.get('byte') as Bot | null }
  const controller = exports.createSessionMenuController(() => current)
  return { app, store, controller, context: () => current, switchTo: (next: typeof current) => { current = next }, close: () => app.detach('device') }
}

test('D545 R2: factory real antes do hello/catálogo e Hub sem Spaces permitem Copiar; cache nunca permite Arquivar', async () => {
  for (const cached of [false, true]) {
    const host = fakeHost()
    let release = () => {}
    host.state.holdHello = new Promise<void>((r) => { release = r })
    const f = await factory(host, cached)
    try {
      assert.equal(f.store.connectedClient, null)
      const origin = f.controller.capture('byte', 'Byte')!
      assert.ok(origin, 'principal antes do catálogo')
      assert.equal(await f.controller.copy(origin, true, async (text) => { assert.equal(text, 'Byte') }), 'copied')
      assert.equal(origin.spaceId, undefined)
      assert.equal(f.controller.canArchive(origin), false)
      release(); await settle(); await f.store.sessions.load('byte')
      assert.ok(f.store.connectedClient)
      if (cached) assert.ok(f.store.spaces, 'snapshot realmente restaurado pela factory')
      const extra = f.controller.capture('extra', 'Extra')!
      assert.equal(await f.controller.copy(extra, false, async (text) => { assert.equal(text, 'extra') }), 'copied')
      assert.equal(await f.controller.copy(extra, false, async () => { throw new Error('clipboard sintético') }), 'copyFailed')
      assert.equal(f.controller.canArchive(extra), false)
      assert.equal(await f.controller.archive(extra), null)
      assert.equal(host.calls.some((c) => c.method === 'sessionClose'), false)
    } finally { release(); f.close() }
  }
})

test('D545 R2: controller do hook com factory sem Space captura hello vivo e envia Hub/Space; outro Space fica intacto', async () => {
  const a = fakeHost('pessoal-real'), b = fakeHost('space-b')
  const first = await factory(a), second = await factory(b)
  try {
    await settle(); await Promise.all([first.store.sessions.load('byte'), second.store.sessions.load('byte')])
    assert.equal(first.store.connectedClient!.spaceId, undefined)
    const origin = first.controller.capture('extra', 'Extra')!
    assert.equal(origin.spaceId, 'pessoal-real'); assert.equal(origin.hubId, 'mesmo-hub')
    assert.equal(first.controller.canArchive(origin), true)
    assert.equal(first.controller.canArchive(first.controller.capture('byte', 'Byte')), false, 'principal não é arquivada')
    const other = second.controller.capture('extra', 'Extra')!
    assert.equal(other.spaceId, 'space-b')
    assert.equal(second.controller.canArchive(other), false, 'extras continuam só no Pessoal')
    first.switchTo(second.context())
    assert.equal(await first.controller.archive(origin), null, 'confirmação aberta no store anterior não migra')
    first.switchTo({ store: first.store, chat: first.store.bots.get('byte')! })
    let release = () => {}
    a.state.holdClose = new Promise<void>((r) => { release = r })
    const pending = first.controller.archive(origin)
    assert.equal(first.store.sessions.of('byte')![0]!.state, 'open', 'sem confirmação otimista')
    first.switchTo(second.context()); release()
    assert.equal(await pending, null, 'resposta tardia não vira aviso/confirmação no outro contexto')
    assert.equal(first.store.sessions.of('byte')![0]!.state, 'closed')
    assert.equal(second.store.sessions.of('byte')![0]!.state, 'open')
    assert.deepEqual(plain(a.calls.filter((c) => c.method === 'sessionClose').map((c) => c.body)), [
      { botId: 'byte', sessionId: 'extra', expectedRevision: 1, hubId: 'mesmo-hub', spaceId: 'pessoal-real' },
    ])
    assert.equal(b.calls.some((c) => c.method === 'sessionClose'), false)
  } finally { first.close(); second.close() }
})

test('D545 R2: reconectar retira prova de escrita; hello legado novo não reaproveita Spaces nem intenção antiga', async () => {
  const host = fakeHost('pessoal-real'), f = await factory(host)
  try {
    await settle(); await f.store.sessions.load('byte')
    const origin = f.controller.capture('extra', 'Extra')!
    assert.equal(f.controller.canArchive(origin), true)
    let finishCopy = () => {}
    const copying = f.controller.copy(origin, false, () => new Promise<void>((r) => { finishCopy = r }))
    let release = () => {}
    host.state.holdHello = new Promise<void>((r) => { release = r })
    f.store.restartStream()
    assert.ok(f.store.spaces, 'mirror mantém apresentação')
    assert.equal(f.store.sessionMenuHello, null, 'prova não vem do mirror nem do link anterior')
    assert.equal(await f.controller.archive(origin), null)
    finishCopy(); assert.equal(await copying, null, 'feedback tardio ignorado')
    host.state.space = undefined; release(); await settle(); await f.store.sessions.load('byte')
    const next = f.controller.capture('extra', 'Extra')!
    assert.equal(await f.controller.copy(next, false, async () => {}), 'copied')
    assert.equal(next.spaceId, undefined)
    assert.equal(f.controller.canArchive(next), false)
    assert.equal(host.calls.some((c) => c.method === 'sessionClose'), false)
  } finally { f.close() }
})


test('D545 R2: hello com lista de Spaces, mas sem contexto atual, não autoriza um default Pessoal', async () => {
  const host = fakeHost('pessoal-real')
  host.state.identified = false
  const f = await factory(host)
  try {
    await settle(); await f.store.sessions.load('byte')
    assert.ok(f.store.sessionMenuHello?.spaces, 'resposta é viva, não apenas cache')
    const origin = f.controller.capture('extra', 'Extra')!
    assert.equal(origin.spaceId, undefined)
    assert.equal(f.controller.canArchive(origin), false)
    assert.equal(await f.controller.archive(origin), null)
    assert.equal(await f.controller.copy(origin, true, async (text) => { assert.equal(text, 'Extra') }), 'copied')
    assert.equal(host.calls.some((c) => c.method === 'sessionClose'), false)
  } finally { f.close() }
})


test('D545 exclusão após rebase: controller real captura Space/revisão, recusa troca e ignora resposta na tela nova', async () => {
  const a = fakeHost('pessoal-real'), b = fakeHost('space-b')
  a.state.actions.push('delete'); b.state.actions.push('delete')
  const first = await factory(a), second = await factory(b)
  try {
    await settle(); await Promise.all([first.store.sessions.load('byte'), second.store.sessions.load('byte')])
    const origin = first.controller.capture('extra', 'Extra')!
    assert.equal(origin.revision, 1)
    assert.equal(first.controller.canRemove(origin), true)
    assert.equal(first.controller.canRemove(first.controller.capture('byte', 'Byte')), false)
    assert.equal(second.controller.canRemove(second.controller.capture('extra', 'Extra')), false)
    first.switchTo(second.context())
    assert.equal(await first.controller.remove(origin), null)
    assert.equal(a.calls.some((c) => c.method === 'sessionDelete'), false)
    first.switchTo({ store: first.store, chat: first.store.bots.get('byte')! })
    let release = () => {}
    a.state.holdDelete = new Promise<void>((r) => { release = r })
    const pending = first.controller.remove(origin)
    first.switchTo(second.context()); release()
    assert.equal(await pending, null)
    assert.equal(first.store.sessions.of('byte')?.length, 0)
    assert.equal(second.store.sessions.of('byte')?.length, 1)
    assert.deepEqual(plain(a.calls.filter((c) => c.method === 'sessionDelete').map((c) => c.body)), [
      { hubId: 'mesmo-hub', spaceId: 'pessoal-real', botId: 'byte', sessionId: 'extra', expectedRevision: 1 },
    ])
    assert.equal(b.calls.some((c) => c.method === 'sessionDelete'), false)
  } finally { first.close(); second.close() }
})

test('D545 exclusão após rebase: Hub legado e revisão mudada recusam antes da RPC, sem perder Copiar', async () => {
  const legacy = fakeHost(), live = fakeHost('pessoal-real')
  legacy.state.actions.push('delete'); live.state.actions.push('delete')
  const old = await factory(legacy, true), current = await factory(live)
  try {
    await settle(); await Promise.all([old.store.sessions.load('byte'), current.store.sessions.load('byte')])
    const legacyOrigin = old.controller.capture('extra', 'Extra')!
    assert.equal(old.controller.canRemove(legacyOrigin), false)
    assert.equal(await old.controller.remove(legacyOrigin), null)
    assert.equal(await old.controller.copy(legacyOrigin, false, async () => {}), 'copied')
    const origin = current.controller.capture('extra', 'Extra')!
    const rows = current.store.sessions.of('byte')!
    current.store.sessions.receive({ type: 'session', session: { ...rows[0]!, revision: 2 } })
    assert.deepEqual(plain(await current.controller.remove(origin)), { ok: false, falha: 'conflict' })
    assert.equal(legacy.calls.some((c) => c.method === 'sessionDelete'), false)
    assert.equal(live.calls.some((c) => c.method === 'sessionDelete'), false)
  } finally { old.close(); current.close() }
})
