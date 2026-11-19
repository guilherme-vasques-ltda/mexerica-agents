import { test } from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { BotStore } from './bot-store.ts'
import { strict as assert } from 'node:assert'
import { ContratoSpace, capacidadeSpace, chamarComSpace, somenteDoSpace } from './fork-space-contrato.ts'

function fixture() {
  const calls: { method: string; params: unknown }[] = []
  let respond: (value: unknown) => void = () => {}
  let reject: (error: Error) => void = () => {}
  const client = { call<T>(method: string, params: unknown): Promise<T> {
    calls.push({ method, params })
    return new Promise<T>((resolve, fail) => { respond = resolve as (value: unknown) => void; reject = fail })
  } }
  const root = { computer: { id: 'c' }, hello: { hostId: 'hub', spaceMemoryProfileContract: 1 }, spaces: { personalSpaceId: 'p', spaces: [] }, connectedClient: client, principalEpoch: 1, spaceId: undefined as string | undefined }
  const other = { ...root, spaceId: 'b' }
  const mirrors = { active: null as { computerId: string; hostId: string | null; spaceId: string } | null, spaceEpoch: 0,
    root: () => root, current: () => mirrors.active ? other : root,
    activate(id?: string) { this.spaceEpoch++; this.active = id ? { computerId: 'c', hostId: 'hub', spaceId: id } : null } }
  const app = { selection: { computerId: 'c' }, spaceMirrors: mirrors }
  return { root, other, mirrors, app, calls, resolve: (v: unknown) => respond(v), reject: (e: Error) => reject(e) }
}

test('capacidades ausentes ou inválidas são zero', () => {
  assert.equal(capacidadeSpace(null, 'spaceUsageContract'), 0)
  for (const n of [0, -1, 1.5, NaN, Infinity]) assert.equal(capacidadeSpace({ spaceUsageContract: n }, 'spaceUsageContract'), 0)
  assert.equal(capacidadeSpace({ spaceUsageContract: 2 }, 'spaceUsageContract'), 2)
})
test('moderno envia Space capturado e recusa respostas e eventos alheios ou sem Space', async () => {
  const f = fixture(), c = new ContratoSpace(f.root, f.app)
  const p = chamarComSpace(c, 'saveMemoryProfile', { content: 'x', spaceId: 'b' })
  assert.deepEqual(f.calls[0], { method: 'saveMemoryProfile', params: { content: 'x', spaceId: 'p' } })
  f.resolve({ spaceId: 'b' }); assert.deepEqual(await p, { kind: 'descartado' })
  assert.equal(c.eventoDoSpace('memoryProfile', {}), false)
  assert.equal(c.eventoDoSpace('memoryProfile', { spaceId: 'p' }), true)
  c.dispose()
})
test('legado só D6/D7 no Pessoal; D2–D5 indisponíveis', async () => {
  const f = fixture(); f.root.hello.spaceMemoryProfileContract = 0
  const c = new ContratoSpace(f.root, f.app)
  for (const m of ['functionModels', 'donnaCostReport', 'donnaStorageReport', 'donnaEngineAccounts'] as const) assert.deepEqual(await c.chamarComSpace(m), { kind: 'indisponivel' })
  for (const m of ['memoryProfile', 'donnaDelegationRules'] as const) {
    const p = c.chamarComSpace(m, { spaceId: 'b' }); assert.deepEqual(f.calls.at(-1)?.params, {})
    f.resolve({ content: 'p' }); assert.equal((await p).kind, 'ok')
    assert.equal(c.eventoDoSpace(m, {}), true)
  }
  f.mirrors.activate('b'); const b = new ContratoSpace(f.other, f.app)
  assert.deepEqual(await b.chamarComSpace('memoryProfile'), { kind: 'indisponivel' })
  assert.equal(b.eventoDoSpace('memoryProfile', {}), false)
  c.dispose(); b.dispose()
})
test('A → B → A síncrono e dispose descartam resposta e falha tardias', async () => {
  const f = fixture(), c = new ContratoSpace(f.root, f.app)
  const p = c.chamarComSpace('memoryProfile'); f.mirrors.activate('b'); f.mirrors.activate()
  f.resolve({ spaceId: 'p' }); assert.deepEqual(await p, { kind: 'descartado' })
  const q = c.chamarComSpace('memoryProfile'); f.mirrors.activate('b'); f.mirrors.activate()
  f.reject(new Error('falha')); assert.deepEqual(await q, { kind: 'descartado' })
  const r = c.chamarComSpace('memoryProfile'); c.dispose(); f.resolve({ spaceId: 'p' })
  assert.deepEqual(await r, { kind: 'descartado' })
})
test('sem ID pessoal conhecido ou após reconexão não aceita resposta', async () => {
  const f = fixture(), c = new ContratoSpace(f.root, f.app)
  const p = c.chamarComSpace('memoryProfile'); f.root.principalEpoch++
  f.resolve({ spaceId: 'p' }); assert.deepEqual(await p, { kind: 'descartado' })
  f.root.spaces = null as never
  assert.deepEqual(await c.chamarComSpace('memoryProfile'), { kind: 'indisponivel' })
})

test('D6/D7 sem capacidade não aceitam nem eventos carimbados fora do Pessoal', () => {
  const f = fixture(); f.root.hello.spaceMemoryProfileContract = 0
  f.mirrors.activate('b')
  const c = new ContratoSpace(f.other, f.app)
  for (const method of ['memoryProfile', 'donnaDelegationRules'] as const) {
    assert.equal(c.eventoDoSpace(method, { spaceId: 'b' }), false)
  }
})

test('D6/D7 legados: escrita sem spaceId e resposta atrasada descartada após troca ou desmontagem', async () => {
  for (const method of ['saveMemoryProfile', 'donnaDelegationRulesSave'] as const) {
    const f = fixture(); f.root.hello.spaceMemoryProfileContract = 0
    const c = new ContratoSpace(f.root, f.app)
    const pending = c.chamarComSpace(method, { content: 'nota', spaceId: 'b' })
    assert.deepEqual(f.calls[0]?.params, { content: 'nota' })
    f.mirrors.activate('b')
    f.resolve({ content: 'antiga' })
    assert.deepEqual(await pending, { kind: 'descartado' })
    f.mirrors.activate()
    const later = c.chamarComSpace(method)
    c.dispose(); f.resolve({})
    assert.deepEqual(await later, { kind: 'descartado' })
  }
})

test('Space B moderno: envia seu ID, aceita B e descarta resposta sem escopo', async () => {
  const f = fixture(); f.mirrors.activate('b')
  const c = new ContratoSpace(f.other, f.app)
  const pending = c.chamarComSpace('memoryProfile')
  assert.deepEqual(f.calls[0]?.params, { spaceId: 'b' })
  f.resolve({ spaceId: 'b' }); assert.equal((await pending).kind, 'ok')
  const missing = c.chamarComSpace('memoryProfile')
  f.resolve({}); assert.deepEqual(await missing, { kind: 'descartado' })
})


test('SpaceMirrors real invalida Pessoal → B → Pessoal antes da notificação da janela', async () => {
  const code = buildSync({ entryPoints: [new URL('./fork-space-mirrors.ts', import.meta.url).pathname],
    bundle: true, platform: 'node', format: 'cjs', write: false }).outputFiles[0]!.text
  const module = { exports: {} }
  runInNewContext(code, { module })
  const { SpaceMirrors } = module.exports as typeof import('./fork-space-mirrors.ts')
  const f = fixture()
  const root = f.root as unknown as BotStore
  const mirrors = new SpaceMirrors({ stores: new Map([['c', root]]), selection: null, select() {} }, () => {})
  const c = new ContratoSpace(root, { selection: null, spaceMirrors: mirrors })
  const pending = c.chamarComSpace('memoryProfile')
  mirrors.activate('c', 'b'); mirrors.activate('c')
  f.resolve({ spaceId: 'p' })
  assert.deepEqual(await pending, { kind: 'descartado' })
  const detached = c.chamarComSpace('memoryProfile')
  mirrors.detach('c'); f.resolve({ spaceId: 'p' })
  assert.deepEqual(await detached, { kind: 'descartado' })
})


test('guard público conserva modo e geração; eventos modernos recusam outro Space', () => {
  const f = fixture(), c = new ContratoSpace(f.root, f.app)
  const ativo = c.capturar('memoryProfile')
  assert.equal(somenteDoSpace(ativo, { spaceId: 'p' }), true)
  assert.equal(c.eventoDoSpace('memoryProfile', { spaceId: 'b' }), false)
  f.mirrors.activate('b'); f.mirrors.activate()
  assert.equal(somenteDoSpace(ativo, { spaceId: 'p' }), false)
  assert.equal(somenteDoSpace(null, {}), false)
})

test('troca de Hub, conexão, seleção ou capacidade descarta resposta pendente', async () => {
  const mudancas = [
    (f: ReturnType<typeof fixture>) => { f.root.hello.hostId = 'outro-hub' },
    (f: ReturnType<typeof fixture>) => { f.root.connectedClient = { ...f.root.connectedClient } },
    (f: ReturnType<typeof fixture>) => { f.app.selection = { computerId: 'outro-computador' } },
    (f: ReturnType<typeof fixture>) => { f.root.hello.spaceMemoryProfileContract = 0 },
  ]
  for (const mudar of mudancas) {
    const f = fixture(), c = new ContratoSpace(f.root, f.app)
    const pending = c.chamarComSpace('memoryProfile'); mudar(f)
    f.resolve({ spaceId: 'p' }); assert.deepEqual(await pending, { kind: 'descartado' })
  }
})

test('versões 1 e 2 aceitam respostas locais; legado recusa spaceId alheio; erro atual propaga', async () => {
  for (const version of [1, 2]) {
    const f = fixture(); f.root.hello.spaceMemoryProfileContract = version
    const c = new ContratoSpace(f.root, f.app)
    const pending = c.chamarComSpace('memoryProfile')
    f.resolve({ spaceId: 'p' }); assert.equal((await pending).kind, 'ok')
  }
  const f = fixture(); f.root.hello.spaceMemoryProfileContract = 0
  const c = new ContratoSpace(f.root, f.app)
  const pending = c.chamarComSpace('memoryProfile')
  f.resolve({ spaceId: 'b' }); assert.deepEqual(await pending, { kind: 'descartado' })
  const error = new Error('falha atual')
  const failed = c.chamarComSpace('memoryProfile'); f.reject(error)
  await assert.rejects(failed, value => value === error)
})

// Usa o caminho real de seleção de conversas, sem inicializar serviços da janela.
const appCode = buildSync({ entryPoints: [new URL('./app-model.ts', import.meta.url).pathname],
  bundle: true, platform: 'node', format: 'cjs', write: false,
  loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
}).outputFiles[0]!.text
function selectionFixture() {
  const module = { exports: {} }
  const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
  runInNewContext(appCode, { module, queueMicrotask, URL, URLSearchParams,
    localStorage: { getItem: () => null }, location: { search: '' },
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3' }) },
  })
  const { AppModel } = module.exports as typeof import('./app-model.ts')
  const f = fixture()
  let notifyMirror = () => {}
  const other = Object.assign(f.other, { bots: new Map(), selection: null as string | null,
    subscribe(fn: () => void) { notifyMirror = fn; return () => {} }, start() {}, retire() {} }) as unknown as BotStore
  const root = Object.assign(f.root, { bots: new Map(), subscribe: () => () => {}, start() {},
    mirrorInSpace: () => other }) as unknown as BotStore
  Object.assign(root.hello!, { spacesContract: 1 })
  root.spaces!.spaces = [{ id: 'b', kind: 'space' }] as NonNullable<BotStore['spaces']>['spaces']
  const app = new AppModel(false)
  app.stores.set('c', root)
  app.spaceMirrors.attach(root)
  return { ...f, root, other, app, autoSelect(botId: string) { other.selection = botId; notifyMirror() } }
}

test('AppModel.select conserva leitura e gravação ao selecionar a mesma ou outra conversa no Pessoal', async () => {
  for (const version of [0, 1]) {
    for (const method of ['memoryProfile', 'saveMemoryProfile', 'donnaDelegationRulesSave'] as const) {
      for (const botId of ['primeira', 'outra']) {
        const f = selectionFixture()
        Object.assign(f.root.hello!, { spaceMemoryProfileContract: version, spaceDelegationContract: version })
        f.app.select({ computerId: 'c', botId: 'primeira' })
        const c = new ContratoSpace(f.root, f.app)
        const pending = c.chamarComSpace(method, { content: 'nota' })
        f.app.select({ computerId: 'c', botId, spaceId: 'p' })
        const valor = version ? { spaceId: 'p', revision: 2 } : { revision: 2 }
        f.resolve(valor)
        assert.deepEqual(await pending, { kind: 'ok', valor })
      }
    }
  }
})

test('AppModel.select descarta troca de Space e ida e volta entre computadores no Pessoal', async () => {
  for (const destination of ['space', 'computer']) {
    const f = selectionFixture()
    f.app.stores.set('outro', { ...f.root, computer: { id: 'outro' } } as BotStore)
    f.app.select({ computerId: 'c', botId: 'primeira' })
    const c = new ContratoSpace(f.root, f.app)
    const pending = c.chamarComSpace('saveMemoryProfile')
    f.app.select(destination === 'space' ? { computerId: 'c', spaceId: 'b', botId: 'outra' } : { computerId: 'outro', botId: 'outra' })
    f.app.select({ computerId: 'c', botId: 'primeira' })
    f.resolve({ spaceId: 'p' })
    assert.deepEqual(await pending, { kind: 'descartado' })
  }
})

test('auto-seleção do mirror mantém resultado de gravação no mesmo Space não pessoal', async () => {
  const f = selectionFixture()
  f.app.select({ computerId: 'c', spaceId: 'b', botId: 'primeira' })
  const c = new ContratoSpace(f.other, f.app)
  for (const botId of ['primeira', 'outra']) {
    const pending = c.chamarComSpace('saveMemoryProfile', { content: 'nota' })
    f.autoSelect(botId)
    assert.equal(f.app.selection?.botId, botId)
    assert.equal(f.app.selection?.spaceId, 'b')
    const valor = { spaceId: 'b', revision: 2 }
    f.resolve(valor)
    assert.deepEqual(await pending, { kind: 'ok', valor })
  }
})
