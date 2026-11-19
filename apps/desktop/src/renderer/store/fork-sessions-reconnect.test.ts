import assert from 'node:assert/strict'
import test from 'node:test'
import type { HostClient, HostEvent } from '../client/host-client'
import { ChatSessionsModel } from './fork-sessions.ts'

const hello = { type: 'hello', hostId: 'hub', rev: 1, usage: { providers: [] }, screen: null } satisfies HostEvent
// Formato de host/src/fork/sessoes/api.rs no Hub 0.1.0: ações no topo da resposta.
const catalog = { sessions: [], sessionActions: ['create', 'send', 'close', 'continue', 'engine', 'choice', 'delete'] }
const tick = async () => { for (let i = 0; i < 12; i++) await Promise.resolve() }
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

test('primeiro catálogo do Pessoal pedido antes do hello é refeito na geração confirmada', async () => {
  const ready = deferred<HostClient>()
  const calls: string[] = []
  const client = { call: async (method: string) => { calls.push(method); return catalog } } as unknown as HostClient
  const model = new ChatSessionsModel(() => ready.promise, () => client)
  const pending = model.load('berga')
  model.receive(hello)
  ready.resolve(client)
  await pending
  await tick()
  assert.equal(model.can('berga', 'create'), true)
  assert.equal(model.can('berga', 'send'), true)
  assert.deepEqual(model.of('berga'), [])
  assert.equal(calls.length, 2, 'a resposta anterior ao hello não substitui a leitura da geração atual')
})

for (const actions of [catalog.sessionActions, []]) {
  test(`reconexão durante o primeiro catálogo usa só as ações do link novo (${actions.length})`, async () => {
    const oldReply = deferred<typeof catalog>()
    const oldClient = { call: () => oldReply.promise } as unknown as HostClient
    let reads = 0
    const newClient = { call: async () => { reads++; return { ...catalog, sessionActions: actions } } } as unknown as HostClient
    let live = oldClient
    const model = new ChatSessionsModel(async () => live, () => live)
    model.receive(hello)
    const pending = model.load('berga')
    await tick()
    assert.equal(model.of('berga'), null, 'a primeira leitura ainda não virou lista')
    live = newClient
    model.receive(hello)
    await tick()
    assert.equal(reads, 1, 'reconexão precisa refazer também a leitura que ainda não terminou')
    oldReply.resolve(catalog)
    await pending
    assert.equal(model.can('berga', 'create'), actions.includes('create'))
    assert.equal(model.can('berga', 'send'), actions.includes('send'))
  })
}
