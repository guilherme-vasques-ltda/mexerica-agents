import assert from 'node:assert/strict'
import test from 'node:test'
import { createRemoteHub } from './transport.ts'
import { createWebBridge } from '../platform.ts'
import type { CodyncBridge } from '../../shared/ipc.ts'
import { createRemoteController } from './controller.ts'
import { createWorkspaceSession, type WorkspaceContext } from './workspace-session.ts'
import type { HubWorkspaceGrant } from './workspaces.ts'

const origin = 'https://hub.example.invalid'
const personal: HubWorkspaceGrant = { id: 'personal:u1', kind: 'personal' }
const workspace: WorkspaceContext = {
  schema: 1, hubId: 'hub-fixture', kind: 'personal', workspaceId: personal.id,
  userId: 'u1', role: 'owner', workspaceEpoch: 2, capabilities: [],
}

function sse(events: unknown[], keepOpen = false): Response {
  const encoder = new TextEncoder()
  const body = new ReadableStream<Uint8Array>({
    start(stream) {
      for (const event of events) stream.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`))
      if (!keepOpen) stream.close()
    },
  })
  return new Response(body, { headers: { 'content-type': 'text/event-stream' } })
}

function hello(epoch = workspace.workspaceEpoch) {
  return { type: 'hello', hostId: workspace.hubId, computerId: 'computer-fixture', workspace: { ...workspace, workspaceEpoch: epoch } }
}

async function activeFixture(fetcher: typeof fetch, cleared: string[] = []) {
  const remote = createRemoteHub(() => ({ origin, authorize: async () => ({}), fetch: fetcher }))
  const states: { status: string; activeId: string | null }[] = []
  const controller = createRemoteController((state) => states.push(state), remote,
    createWorkspaceSession(async (id) => { cleared.push(id) }))
  await controller.setIdentity('u1', [], [personal])
  return { remote, states, controller }
}

async function until(predicate: () => boolean) {
  for (let i = 0; i < 100 && !predicate(); i++) await new Promise((resolve) => setTimeout(resolve, 0))
  assert.ok(predicate(), 'fixture state did not settle')
}

test('host transport passes RPC results through the generic web bridge unchanged', async () => {
  const requests: Request[] = []
  const hub = createRemoteHub(() => ({
    origin,
    authorize: async () => ({}),
    fetch: async (request) => {
      requests.push(request)
      if (new URL(request.url).pathname === '/events') return sse([hello(), { type: 'donnaAttentionChanged', fixture: true }], true)
      return Response.json({ ok: true, result: { value: 7 } })
    },
  }))
  const session = createWorkspaceSession(async () => {})
  const ctl = createRemoteController(() => {}, hub, session)
  await ctl.setIdentity('u1', [], [personal])
  const host: CodyncBridge['host'] = hub.host
  const bridge = createWebBridge({ host })
  const snapshot = await bridge.host.snapshot()
  assert.equal(snapshot.local?.token, (await hub.host.snapshot()).local?.token)
  const result = await bridge.host.call(origin, snapshot.local!.token, 'donnaAttention', { prompt: 'fixture' }, 1000)
  assert.deepEqual(result, { ok: true, result: { value: 7 } })
  const rpc = requests.find((request) => request.method === 'POST')!
  assert.equal(new URL(rpc.url).origin, origin)
  assert.equal(new URL(rpc.url).pathname, '/api/donnaAttention')
  assert.deepEqual(await rpc.json(), { prompt: 'fixture', workspaceEpoch: 2 })
  const events: unknown[] = []
  bridge.host.stream(`${origin}/events?since=12&spaceId=personal&workspaceEpoch=999`, snapshot.local!.token,
    (line) => events.push(JSON.parse(line)), () => {})
  await until(() => events.length === 2)
  assert.deepEqual(events, [hello(), { type: 'donnaAttentionChanged', fixture: true }])
  assert.equal(new URL(requests[0].url).searchParams.has('workspaceEpoch'), false)
  assert.deepEqual(Object.fromEntries(new URL(requests[2].url).searchParams), { since: '12', spaceId: 'personal', workspaceEpoch: '2' })
  assert.ok(requests.every((request) => request.credentials === 'omit' && request.redirect === 'error' && request.cache === 'no-store'))
  await ctl.setIdentity(null, [], [])
})

test('missing binding and unavailable authorization fail without sending a request', async () => {
  let fetchCount = 0
  const configured = createRemoteHub(() => ({
    origin,
    authorize: async () => { throw new Error('fixture proof unavailable') },
    fetch: async () => { fetchCount++; return Response.json({}) },
  }))
  for (const hub of [createRemoteHub(() => null), configured]) {
    const controller = createRemoteController(() => {}, hub, createWorkspaceSession(async () => {}))
    await controller.setIdentity('u1', [], [personal])
    assert.equal((await hub.host.snapshot()).local, null)
    await assert.rejects(hub.host.call(origin, 'old-lease', 'donnaAttention', {}, 1000), { code: 'workspace_inactive' })
    await controller.setIdentity(null, [], [])
  }
  assert.equal(fetchCount, 0)
})

for (const type of ['workspace_revoked', 'role_changed'] as const) {
  test(`terminal ${type} event revokes lease and resets workspace selector`, async () => {
    const streams: ReadableStreamDefaultController<Uint8Array>[] = []
    const requests: Request[] = []
    const cleared: string[] = []
    const encoder = new TextEncoder()
    const frame = (event: unknown) => encoder.encode(`data: ${JSON.stringify(event)}\n\n`)
    const fetcher: typeof fetch = async (request) => {
      requests.push(request)
      if (request.method === 'POST') return new Promise(() => {})
      return new Response(new ReadableStream<Uint8Array>({
        start(stream) { streams.push(stream); stream.enqueue(frame(hello())) },
      }), { headers: { 'content-type': 'text/event-stream' } })
    }
    const { remote, states } = await activeFixture(fetcher, cleared)
    const token = (await remote.host.snapshot()).local!.token
    const delivered: unknown[] = []
    remote.host.stream(`${origin}/events`, token, (line) => delivered.push(JSON.parse(line)), () => {})
    const pending = assert.rejects(remote.host.call(origin, token, 'send', {}, 1000), { code: 'workspace_inactive' })
    await until(() => delivered.length === 1 && requests.some((r) => r.method === 'POST'))
    streams[1].enqueue(frame({ type, workspaceId: personal.id, workspaceEpoch: 3 }))
    streams[1].enqueue(frame({ type: 'entry' }))
    await pending
    await until(() => cleared.length === 1)
    assert.equal(states.at(-1)?.status, 'revoked')
    assert.equal(states.at(-1)?.activeId, null)
    assert.equal((await remote.host.snapshot()).local, null)
    assert.ok(requests.every((r) => r.signal.aborted))
    assert.deepEqual(delivered, [hello()])
    assert.deepEqual(cleared, [personal.id])
  })
}

test('epoch mismatch clears workspace cache, confirms once again, and never replays an action', async () => {
  const cleared: string[] = []
  const requests: Request[] = []
  let opens = 0
  const fetcher: typeof fetch = async (request) => {
    requests.push(request)
    if (request.method === 'POST' && opens === 1) return Response.json({ error: 'epoch', code: 'workspace_epoch_mismatch' }, { status: 409 })
    if (request.method === 'POST') return Response.json({ ok: true })
    opens++
    return sse([hello(opens === 1 ? 2 : 3)], true)
  }
  const { remote, states, controller } = await activeFixture(fetcher, cleared)
  const token = (await remote.host.snapshot()).local!.token
  await assert.rejects(remote.host.call(origin, token, 'send', {}, 1000), { status: 409, code: 'workspace_epoch_mismatch' })
  await until(() => opens >= 2)
  await until(() => states.at(-1)?.status === 'active')
  assert.deepEqual(cleared, [personal.id])
  assert.equal(opens, 2)
  assert.equal(requests.filter((request) => request.method === 'POST').length, 1)
  assert.ok(requests[0].signal.aborted)
  await assert.rejects(remote.host.call(origin, token, 'send', {}, 1000), { code: 'workspace_inactive' })
  const current = (await remote.host.snapshot()).local!.token
  await remote.host.call(origin, current, 'donnaAttention', {}, 1000)
  assert.deepEqual(await requests.at(-1)!.json(), { workspaceEpoch: 3 })
  await controller.setIdentity(null, [], [])
})

for (const failure of ['network', 'timeout']) test(`${failure} marks hub offline without retrying the action`, async () => {
  let calls = 0
  const fetcher: typeof fetch = async (request) => {
    if (new URL(request.url).pathname === '/events') return sse([hello()], true)
    calls++
    if (failure === 'timeout') return new Promise(() => {})
    throw new TypeError('fixture network failure')
  }
  const { remote, states } = await activeFixture(fetcher)
  const token = (await remote.host.snapshot()).local!.token
  await assert.rejects(remote.host.call(origin, token, 'donnaAttention', {}, failure === 'timeout' ? 30 : 1000), { code: 'hub_offline' })
  await until(() => states.at(-1)?.status === 'hub_offline')
  assert.equal(calls, 1)
})
