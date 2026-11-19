import assert from 'node:assert/strict'
import { test } from 'node:test'
import { RoutinesPauseModel, type PauseClient, type PauseSource } from '../store/routines-pause.ts'

const deferred = <T>() => {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}
const flush = () => new Promise<void>((done) => setImmediate(done))

class Source implements PauseSource {
  hello: { hostId: string } | null = { hostId: 'A' }
  connectedClient: PauseClient | null
  principalEpoch = 1
  private listeners = new Set<() => void>()
  nextReady: Promise<PauseClient> | null = null
  constructor(client: PauseClient) { this.connectedClient = client }
  ready() { return this.nextReady ?? Promise.resolve(this.connectedClient!) }
  subscribe(listener: () => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  restart(client: PauseClient) {
    this.connectedClient = client
    for (const listener of this.listeners) listener()
  }
  switch(hostId: string, client: PauseClient) {
    this.hello = { hostId }
    this.connectedClient = client
    this.principalEpoch++
    for (const listener of this.listeners) listener()
  }
}

test('mesma store troca Hub enquanto mutação anterior responde', async () => {
  const mutation = deferred<{ paused: boolean }>()
  const readB = deferred<{ paused: boolean }>()
  const a: PauseClient = { call: async <T>(method: string) => (method === 'routinesPause' ? { paused: false } : await mutation.promise) as T }
  const b: PauseClient = { call: async <T>() => await readB.promise as T }
  const source = new Source(a)
  const model = new RoutinesPauseModel(source)
  model.start(); await flush()
  assert.equal(model.paused, false)
  const change = model.setPaused(true)
  await flush()
  source.connectedClient = b
  assert.equal(model.paused, null, 'invalida antes da notificação da store')
  source.switch('B', b)
  assert.equal(model.paused, null)
  readB.resolve({ paused: false }); await flush()
  assert.equal(model.paused, false)
  mutation.resolve({ paused: true }); await change
  assert.equal(model.paused, false)
  model.stop()
})

test('intenção de A não é enviada a B quando ready aguarda a troca', async () => {
  const wait = deferred<PauseClient>()
  let sent = 0
  const a: PauseClient = { call: async <T>(method: string) => { if (method === 'setRoutinesPaused') sent++; return { paused: false } as T } }
  const b: PauseClient = { call: async <T>(method: string) => { if (method === 'setRoutinesPaused') sent++; return { paused: false } as T } }
  const source = new Source(a)
  const model = new RoutinesPauseModel(source)
  model.start(); await flush()
  source.nextReady = wait.promise
  const change = model.setPaused(true)
  source.nextReady = null
  source.switch('B', b)
  wait.resolve(b); await change; await flush()
  assert.equal(sent, 0)
  assert.equal(model.paused, false)
  model.stop()
})

for (const [initial, next] of [[false, true], [true, false]] as const) {
  for (const pollFirst of [true, false]) test(`poll ${pollFirst ? 'antes' : 'depois'} da confirmação ${initial} → ${next}`, async () => {
    const poll = deferred<{ paused: boolean }>()
    const mutation = deferred<{ paused: boolean }>()
    let reads = 0
    const client: PauseClient = { call: async <T>(method: string) => {
      if (method === 'setRoutinesPaused') return await mutation.promise as T
      return (++reads === 1 ? { paused: initial } : await poll.promise) as T
    } }
    const source = new Source(client)
    const model = new RoutinesPauseModel(source)
    model.start(); await flush()
    const old = model.refresh(); await flush()
    const change = model.setPaused(next); await flush()
    await model.refresh()
    assert.equal(reads, 2, 'poll não começa durante mutação')
    if (pollFirst) { poll.resolve({ paused: initial }); await old }
    mutation.resolve({ paused: next }); await change
    if (!pollFirst) { poll.resolve({ paused: initial }); await old }
    assert.equal(model.paused, next)
    model.stop()
  })
}

test('erro e cancelamento de contexto não restauram confirmação antiga', async () => {
  const old = deferred<{ paused: boolean }>()
  let calls = 0
  const a: PauseClient = { call: async <T>() => (++calls === 1 ? { paused: false } : await old.promise) as T }
  const b: PauseClient = { call: async () => { throw new Error('indisponível') } }
  const source = new Source(a)
  const model = new RoutinesPauseModel(source)
  model.start(); await flush()
  const poll = model.refresh(); await flush()
  source.switch('B', b); await flush()
  assert.equal(model.paused, null)
  assert.equal(model.error, 'indisponível')
  old.reject(new Error('erro antigo')); await poll
  assert.equal(model.error, 'indisponível')
  model.stop()
})

test('falha de mutação deixa estado desconhecido e descarta poll antigo', async () => {
  const poll = deferred<{ paused: boolean }>()
  const mutation = deferred<{ paused: boolean }>()
  let reads = 0
  const client: PauseClient = { call: async <T>(method: string) => {
    if (method === 'setRoutinesPaused') return await mutation.promise as T
    return (++reads === 1 ? { paused: false } : await poll.promise) as T
  } }
  const model = new RoutinesPauseModel(new Source(client))
  model.start(); await flush()
  const old = model.refresh(); await flush()
  const change = model.setPaused(true); await flush()
  mutation.reject(new Error('falha de gravação')); await change
  poll.resolve({ paused: false }); await old
  assert.equal(model.paused, null)
  assert.equal(model.error, 'falha de gravação')
  model.stop()
})

for (const helloFirst of [false, true]) {
  test(`cliente B com hello A: resposta ${helloFirst ? 'depois' : 'antes'} do hello B`, async () => {
    const oldRead = deferred<{ paused: boolean }>()
    const calls: { method: string; body: Record<string, unknown> }[] = []
    let effects = 0
    const a: PauseClient = { call: async <T>() => ({ paused: false }) as T }
    const b: PauseClient = { call: async <T>(method: string, body: Record<string, unknown>) => {
      calls.push({ method, body })
      if (body.hubId !== 'B') {
        if (helloFirst) return await oldRead.promise as T
        throw new Error('hub_context_mismatch')
      }
      if (method === 'setRoutinesPaused') effects++
      return { paused: method === 'setRoutinesPaused' ? body.paused : false } as T
    } }
    const source = new Source(a)
    const model = new RoutinesPauseModel(source)
    model.start(); await flush()
    assert.equal(model.paused, false)
    source.restart(b) // cliente novo; hello e epoch ainda são de A
    assert.equal(model.paused, null)
    await flush()
    assert.deepEqual(calls[0], { method: 'routinesPause', body: { hubId: 'A' } })
    assert.equal(model.paused, null)
    await model.setPaused(true)
    assert.equal(effects, 0)
    source.switch('B', b)
    await flush()
    assert.equal(model.paused, false)
    assert.deepEqual(calls[1], { method: 'routinesPause', body: { hubId: 'B' } })
    if (helloFirst) { oldRead.reject(new Error('hub_context_mismatch')); await flush() }
    assert.equal(model.paused, false)
    await model.setPaused(true)
    assert.deepEqual(calls[2], { method: 'setRoutinesPaused', body: { hubId: 'B', paused: true } })
    assert.equal(effects, 1)
    model.stop()
  })
}
