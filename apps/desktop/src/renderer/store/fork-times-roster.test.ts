import test from 'node:test'
import assert from 'node:assert/strict'
import { TimesRoster, timesOriginKey, type TimesRosterOrigin } from './fork-times-roster.ts'
import type { TimesSnapshot } from '../../shared/fork-times.ts'

function snapshot(spaceId = 'personal', catalogVersion = 'c1', revision = 7): TimesSnapshot {
  return { version: 1, spaceId, revision, catalogVersion, primaryBotId: null, nodes: [], members: [], routes: [], bots: [] }
}
function deferredClient() {
  const calls: { method: string; body: unknown; resolve: (v: unknown) => void; reject: (e: unknown) => void }[] = []
  return { calls, call(method: string, body?: unknown): Promise<unknown> {
    return new Promise((resolve, reject) => calls.push({ method, body, resolve, reject }))
  } }
}
const stateSnapshot = (model: TimesRoster) => model.snapshot
const settle = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() }
function fixture() {
  const client = deferredClient()
  let origin: TimesRosterOrigin | null = { contextId: 'account', computerId: 'computer', hostId: 'hub-A', spaceId: 'personal', client, epoch: 1 }
  const model = new TimesRoster(() => origin)
  return { model, client, get: () => origin!, set: (next: TimesRosterOrigin | null) => { origin = next } }
}

test('hello aplicado força leitura do Space exato; sem origem não adivinha Hub/Space', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  const origin = f.get()
  f.set(null); f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 0)
  f.set(origin); f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 1)
  assert.deepEqual(f.client.calls[0]!.body, { spaceId: 'personal' })
  assert.equal(f.client.calls[0]!.method, 'donnaTimes')
  f.client.calls[0]!.resolve(snapshot()); await settle()
  assert.equal(f.model.phase, 'ready')
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 2)
  f.model.retire(); f.client.calls[1]!.resolve(snapshot()); await settle()
  assert.equal(stateSnapshot(f.model), null)
})

test('rajada coalesce antes e durante await; resposta invalidada não pinta', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  for (let i = 0; i < 20; i++) f.model.sync({ type: 'bot' })
  t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 1)
  for (let i = 0; i < 20; i++) f.model.sync({ type: 'times', spaceId: 'personal' })
  t.mock.timers.tick(1000)
  assert.equal(f.client.calls.length, 1)
  f.client.calls[0]!.resolve(snapshot('personal', 'antigo')); await settle()
  assert.equal(stateSnapshot(f.model), null)
  t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 2)
  f.client.calls[1]!.resolve(snapshot('personal', 'novo')); await settle()
  assert.equal(stateSnapshot(f.model)?.catalogVersion, 'novo')
  t.mock.timers.tick(10000)
  assert.equal(f.client.calls.length, 2) // sem polling
  f.model.retire()
})

test('outro Space não invalida e catálogo novo com mesma revisão substitui o salvo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.client.calls[0]!.resolve(snapshot()); await settle()
  f.model.sync({ type: 'times', spaceId: 'outro' }); t.mock.timers.tick(250)
  assert.equal(f.client.calls.length, 1)
  f.model.sync({ type: 'botDeleted' }); t.mock.timers.tick(250)
  f.client.calls[1]!.resolve(snapshot('personal', 'c2')); await settle()
  assert.equal(stateSnapshot(f.model)?.catalogVersion, 'c2')
  assert.equal(stateSnapshot(f.model)?.revision, 7)
  f.model.retire()
})

test('A→B→A no mesmo client/epoch descarta a primeira resposta e não herda títulos', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture(), a = f.get()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.set({ ...a, hostId: 'hub-B' }); f.model.sync({ type: 'hello' })
  const bDomain = f.model.domain
  f.set(a); f.model.sync({ type: 'hello' })
  assert.notEqual(f.model.domain, bDomain)
  f.client.calls[0]!.resolve(snapshot('personal', 'A-atrasada')); await settle()
  assert.equal(stateSnapshot(f.model), null)
  t.mock.timers.tick(250)
  f.client.calls[1]!.resolve(snapshot('personal', 'A-atual')); await settle()
  assert.equal(stateSnapshot(f.model)?.catalogVersion, 'A-atual')
  f.model.retire()
})

test('resposta de client/epoch anterior não vence reconexão; resync agenda leitura nova', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture(), next = deferredClient()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.set({ ...f.get(), client: next, epoch: 2 }); f.model.sync({ type: 'hello' })
  f.client.calls[0]!.resolve(snapshot('personal', 'old-link')); await settle()
  assert.equal(stateSnapshot(f.model), null)
  t.mock.timers.tick(250)
  next.calls[0]!.resolve(snapshot('personal', 'new-link')); await settle()
  assert.equal(stateSnapshot(f.model)?.catalogVersion, 'new-link')
  f.model.sync({ type: 'resync' }); t.mock.timers.tick(250)
  assert.equal(next.calls.length, 2)
  f.model.retire(); next.calls[1]!.resolve(snapshot()); await settle()
})

test('offline conserva RAM só da mesma identidade; transitório/cross-Space não substituem snapshot', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture(), a = f.get()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.client.calls[0]!.resolve(snapshot()); await settle()
  const saved = stateSnapshot(f.model)
  f.set(null); f.model.suspend()
  assert.equal(stateSnapshot(f.model), saved)
  assert.equal(f.model.phase, 'offline')
  f.set(a); f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.client.calls[1]!.reject(new Error('transitório')); await settle()
  assert.equal(stateSnapshot(f.model), saved)
  assert.equal(f.model.phase, 'error')
  f.model.retry(); t.mock.timers.tick(250)
  f.client.calls[2]!.resolve(snapshot('outro')); await settle()
  assert.equal(stateSnapshot(f.model), saved)
  f.set({ ...a, contextId: 'outra-conta' }); f.model.sync({ type: 'hello' })
  assert.equal(stateSnapshot(f.model), null)
  f.model.retire()
})

test('recusa de dono remove snapshot, filtro e invalidação pendente; logout/retire encerram timer', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.client.calls[0]!.resolve(snapshot()); await settle()
  const scope = f.model.scope
  f.model.retry(); t.mock.timers.tick(250)
  f.model.sync({ type: 'bot' })
  f.client.calls[1]!.reject(Object.assign(new Error('not_allowed'), { status: 403 })); await settle()
  assert.equal(f.model.phase, 'owner')
  assert.equal(stateSnapshot(f.model), null)
  assert.equal(f.model.domain, null)
  assert.ok(f.model.scope > scope)
  t.mock.timers.tick(1000)
  assert.equal(f.client.calls.length, 2)
  f.model.retry(); f.model.retire(); t.mock.timers.tick(1000)
  assert.equal(f.client.calls.length, 2)
})

test('retire durante await e 401 externo não deixam resposta tardia repor dados', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const f = fixture()
  f.model.sync({ type: 'hello' }); t.mock.timers.tick(250)
  f.model.sync({ type: 'connection' }, true)
  f.client.calls[0]!.resolve(snapshot()); await settle()
  assert.equal(stateSnapshot(f.model), null)
  assert.equal(f.model.phase, 'owner')
  f.model.retry(); t.mock.timers.tick(250)
  f.model.retire(); f.client.calls[1]!.resolve(snapshot()); await settle()
  assert.equal(stateSnapshot(f.model), null)
})

test('namespace usa todos os IDs, inclusive mesmo Pessoal em dois Hubs/computadores', () => {
  const f = fixture(), origin = f.get(), key = timesOriginKey(origin)
  for (const field of ['contextId', 'computerId', 'hostId', 'spaceId'] as const) {
    assert.notEqual(timesOriginKey({ ...origin, [field]: 'outro' }), key)
  }
  const reconnect = { ...origin, epoch: 900 }
  assert.equal(timesOriginKey(reconnect), key) // reconnect não perde fold/ordem
})
