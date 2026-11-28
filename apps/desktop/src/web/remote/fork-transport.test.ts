import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'
import { nobleBackend } from '../../shared/relay-protocol/backend-noble.ts'
import { createProtocol, type ChannelKeys } from '../../shared/relay-protocol/protocol.ts'
import { createRemoteController } from './controller.ts'
import { createRemoteHub } from './fork-transport.ts'
import { openRemoteChannel } from './socket.ts'
import { createWorkspaceSession, type WorkspaceContext } from './workspace-session.ts'

const protocol = createProtocol(nobleBackend)
const host = protocol.signerFromSeed(new Uint8Array(32).fill(9))
const device = protocol.signerFromSeed(new Uint8Array(32).fill(7))
const webDevice = { publicKey: device.publicKey, sign: async (input: Uint8Array) => device.sign(input) }
const personal = { id: 'personal:user_fixture', kind: 'personal' } as const
const context: WorkspaceContext = {
  schema: 1, kind: 'personal', hubId: 'fixture-hub', userId: 'user_fixture', workspaceId: personal.id,
  role: 'owner', workspaceEpoch: 5, capabilities: ['chat'],
}
const origin = 'https://hub.example.invalid'
const target = { origin: 'wss://hub.example.invalid', hubId: context.hubId, signKey: protocol.b64url(host.publicKey) }
const relayOrigin = 'wss://relay.example.invalid'
const replyToken = 'fixture-token'
const b64 = (data: Uint8Array) => Buffer.from(data).toString('base64url')
const raw = (data: string) => Buffer.from(data, 'base64url')
const bytes = (label: string, ...parts: Uint8Array[]) => Buffer.concat([Buffer.from(label), ...parts])
const be = (n: number) => {
  const out = Buffer.alloc(8)
  out.writeBigUInt64BE(BigInt(n))
  return out
}

type Envelope = { id: number; m?: string; sub?: string; cancel?: boolean; b: Record<string, unknown>; pop: { v: number; ts: number; nonce: string; sig: string } }

function fixture(options: { enrolled?: boolean; fresh?: boolean; renew?: boolean; mismatch?: boolean; holdSend?: boolean; noToken?: boolean; failMarkPaired?: boolean; relay?: boolean } = {}) {
  let joined = options.enrolled ?? true
  let now = Date.now()
  let epoch = context.workspaceEpoch
  let tokenRequests = 0
  let refuseHello = false
  const messages: Envelope[] = []
  const receivedNonces = new Set<string>()
  const sockets: FakeSocket[] = []
  const routes: string[] = []

  class FakeSocket extends EventTarget {
    readyState: number = WebSocket.CONNECTING
    private keys?: ChannelKeys
    private hello?: { dk: string; ek: string; n: string; sig: string; pair?: boolean }
    private th?: Buffer
    private sendCount = 0
    private received = 0

    constructor(url: string, relay: boolean, headers?: Record<string, string>) {
      super()
      const expected = relay ? `${relayOrigin}/v1/relay/device/${protocol.computerIdOf(host.publicKey)}?v=1`
        : target.origin + '/channel?v=1'
      assert.equal(url, expected)
      assert.equal(Object.hasOwn(headers ?? {}, 'Codync-Sig'), relay)
      routes.push(relay ? 'relay' : 'direct')
      sockets.push(this)
      queueMicrotask(() => {
        this.readyState = WebSocket.OPEN
        this.dispatchEvent(new Event('open'))
        if (relay) this.deliver({ t: 'presence', online: true })
      })
    }
    close() {
      this.readyState = WebSocket.CLOSED
      queueMicrotask(() => this.dispatchEvent(new Event('close')))
    }
    deliver(frame: object) {
      queueMicrotask(() => this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(frame) })))
    }
    cut(stableCode: 'device_revoked' | 'workspace_revoked' | 'role_changed', signed = true) {
      const marker = { device_revoked: 1, workspace_revoked: 2, role_changed: 3 }[stableCode]
      const abertoEm = epoch++
      const sig = b64(host.sign(bytes('codync/corte/v1', this.th!, Uint8Array.of(marker), be(epoch), be(abertoEm))))
      this.deliver({ t: 'reject', code: 'revoked', stableCode, ...(signed && {
        prova: { v: 1, tipo: 'corte', workspaceEpoch: epoch, abertoEm, sig },
      }) })
    }
    private answer(message: object) {
      const c = this.sendCount++
      this.deliver({ t: 'f', c, d: protocol.sealFrame(this.keys!.h2d, c, true, Buffer.from(JSON.stringify(message))) })
    }
    send(data: string) {
      const frame = JSON.parse(data)
      if (frame.t === 'hello') {
        const hello = this.hello = frame
        const cid = protocol.cidRaw(host.publicKey)
        const [dk, ek, n] = [raw(hello.dk), raw(hello.ek), raw(hello.n)]
        assert.ok(protocol.verifyEd25519(device.publicKey, raw(hello.sig), bytes('codync/hs1/v1', cid, dk, ek, n)))
        if (!joined && !hello.pair) {
          const sig = b64(host.sign(bytes('codync/recusa/v1', cid, dk, ek, n, Uint8Array.of(4))))
          this.deliver({ t: 'reject', code: 'unauthorized', stableCode: 'device_unpaired', prova: { v: 1, tipo: 'recusa', sig } })
          return
        }
        const eph = protocol.ephemeral()
        const th = this.th = createHash('sha256').update(bytes('codync/hs2/v1', cid, dk, ek, n, eph.pub)).digest()
        this.keys = protocol.channelKeys(eph.dh(ek), th)
        this.deliver({ t: 'welcome', v: 1, ek: b64(eph.pub), sig: b64(host.sign(th)) })
        return
      }
      assert.equal(frame.t, 'f')
      assert.equal(frame.c, this.received++)
      const plain = protocol.openFrame(this.keys!.d2h, frame.c, frame.d)
      const m = JSON.parse(Buffer.from(plain.chunk).toString()) as Envelope
      if (m.cancel) return
      messages.push(m)
      const label = m.m ?? 'sub:' + m.sub
      assert.equal(m.pop.v, 1)
      assert.ok(Math.abs(Date.now() - m.pop.ts) < 30_000)
      assert.ok(raw(m.pop.nonce).length >= 16 && raw(m.pop.nonce).length <= 64)
      assert.ok(!receivedNonces.has(m.pop.nonce))
      receivedNonces.add(m.pop.nonce)
      const packed = Buffer.concat([
        Buffer.from('mexerica-pop-v1'),
        ...[context.hubId, b64(this.th!), label, String(m.pop.ts), m.pop.nonce].map((s) => Buffer.concat([Buffer.of(0), Buffer.from(s)])),
      ])
      assert.ok(protocol.verifyEd25519(device.publicKey, raw(m.pop.sig), packed))
      if (m.m === 'pair') {
        assert.equal(this.hello?.pair, true)
        assert.equal(m.b.platform, 'web')
        assert.equal(m.b.sessionToken, replyToken)
        joined = true
        return this.answer({ id: m.id, ok: { computerId: protocol.computerIdOf(host.publicKey) } })
      }
      if (m.m === 'sessionAuthenticate') {
        assert.equal(m.b.sessionToken, replyToken)
        return this.answer({ id: m.id, ok: { userId: context.userId, role: context.role, expiresAt: now + (options.renew && tokenRequests === 1 ? 60 : 60_000) } })
      }
      if (m.m === 'workspaceContext') {
        assert.ok(!('workspaceEpoch' in m.b))
        return this.answer({ id: m.id, ok: { ...context, workspaceEpoch: epoch } })
      }
      assert.equal(m.b.workspaceEpoch, epoch)
      if (m.m === 'hello' && refuseHello) {
        refuseHello = false
        return this.answer({ id: m.id, err: { status: 409, code: 'workspace_epoch_mismatch' } })
      }
      if (m.sub === 'events') {
        assert.equal(m.b.since, 12)
        return this.answer({ id: m.id, ev: { type: 'entry', value: 'fixture', extension: true } })
      }
      if (m.m === 'send' && options.holdSend) return
      if (m.m === 'send' && options.mismatch) return this.answer({ id: m.id, err: { status: 409, code: 'workspace_epoch_mismatch' } })
      this.answer({ id: m.id, ok: { value: m.b.value ?? 7, extra: true } })
    }
  }

  let record: 'new' | 'paired' | 'revoked' | undefined = options.fresh ? undefined : options.failMarkPaired ? 'new' : 'paired'
  const remote = createRemoteHub(() => ({
    ...target, ...(options.relay && { relayOrigin }), userId: context.userId, getSessionToken: async () => {
      tokenRequests++
      return options.noToken ? null : replyToken
    },
  }), {
    device: async (_scope, create) => {
      if (!record && create) record = 'new'
      return record ? { signer: webDevice, state: record } : null
    },
    mark: async (_scope, state) => {
      if (options.failMarkPaired && state === 'paired') throw new Error('fixture_storage_failed')
      record = state
    },
    now: () => now,
    connect: (hub, signer, signal, pair, end) =>
      openRemoteChannel(hub, signer, signal, pair, end, (url, headers) =>
        new FakeSocket(url, !!options.relay && !pair, headers) as unknown as WebSocket),
  })
  const states: string[] = []
  const controller = createRemoteController(({ status }) => states.push(status), remote, createWorkspaceSession(async () => {}))
  return {
    remote, controller, messages, states, sockets, routes, advance: () => { epoch++; now++ }, advanceClock: (ms: number) => { now += ms },
    refuseNextHello: () => { refuseHello = true }, restoreEpoch: () => { epoch = context.workspaceEpoch },
    open: () => controller.setIdentity(context.userId, [], [personal]),
    close: () => controller.setIdentity(null, [], []),
    last: () => states.at(-1),
  }
}

async function until(check: () => boolean) {
  for (let i = 0; i < 100 && !check(); i++) await new Promise((resolve) => setTimeout(resolve, 1))
  assert.ok(check())
}

test('pairs using independent Hub transcript/possession bytes and exposes only the events stream', async (t) => {
  const f = fixture({ enrolled: false })
  t.after(f.close)
  await f.open()
  assert.equal(f.last(), 'device_unpaired')
  await f.controller.pair('fixture-code')
  assert.equal(f.last(), 'active')
  const local = (await f.remote.host.snapshot()).local!
  assert.deepEqual(await f.remote.host.call(origin, local.token, 'send', { value: 'hello', workspaceEpoch: 0 }, 1000),
    { value: 'hello', extra: true })
  const event = await new Promise((resolve, reject) => {
    const stop = f.remote.host.stream(origin + '/events?since=12&workspaceEpoch=0', local.token,
      (line) => { stop(); resolve(JSON.parse(line)) }, reject)
  })
  assert.deepEqual(event, { type: 'entry', value: 'fixture', extension: true })
  const terminal = await new Promise((resolve) => {
    f.remote.host.stream(origin + '/term/fixture', local.token, () => assert.fail('unexpected terminal data'), resolve)
  })
  assert.notEqual(terminal, null)
  assert.deepEqual(f.messages.map((m) => m.m ?? m.sub), ['pair', 'sessionAuthenticate', 'workspaceContext', 'hello', 'send', 'events'])
})

test('pairs directly, then confirms through the signed relay without replay or a direct fallback', async (t) => {
  const f = fixture({ enrolled: false, fresh: true, relay: true })
  t.after(f.close)
  await f.open()
  assert.equal(f.last(), 'device_unpaired')
  await f.controller.pair('fixture-code')
  assert.equal(f.last(), 'active')
  assert.deepEqual(f.routes, ['direct', 'relay'])
  f.sockets.at(-1)!.deliver({ t: 'presence', online: false })
  await until(() => f.last() === 'hub_offline')
  assert.equal(f.remote.rendererContext(), undefined)
  await f.controller.retry()
  assert.equal(f.last(), 'active')
  assert.deepEqual(f.routes, ['direct', 'relay', 'relay'])
  assert.equal(f.messages.filter((m) => m.m === 'pair').length, 1)
})

test('renews before the authenticated expiry on the same channel; no mutation is retried', async (t) => {
  const f = fixture({ renew: true })
  t.after(f.close)
  await f.open()
  await until(() => f.messages.filter((m) => m.m === 'sessionAuthenticate').length === 2)
  await new Promise<void>((resolve) => setImmediate(resolve))
  f.advanceClock(61)
  const local = (await f.remote.host.snapshot()).local!
  assert.deepEqual(await f.remote.host.call(origin, local.token, 'history', {}, 1000), { value: 7, extra: true })
  assert.equal(f.last(), 'active')
  assert.equal(f.sockets.length, 1)
})

test('confirms an already paired key even if the local paired marker could not be written', async (t) => {
  const f = fixture({ enrolled: false, failMarkPaired: true })
  t.after(f.close)
  await f.open()
  assert.equal(f.last(), 'device_unpaired')
  await f.controller.pair('fixture-code')
  assert.equal(f.last(), 'active')
  f.sockets.at(-1)!.close()
  await until(() => f.last() === 'hub_offline')
  await f.controller.retry()
  assert.equal(f.last(), 'active')
  assert.equal(f.messages.filter((m) => m.m === 'pair').length, 1)
})

test('does not purge a partition until hello confirms the new epoch', async (t) => {
  const f = fixture()
  t.after(f.close)
  await f.open()
  const old = f.remote.rendererContext()!
  old.storage.setItem('sentinel', 'saved')
  f.sockets[0].close()
  await until(() => f.last() === 'hub_offline')
  const refused = new Promise<unknown>((resolve) => old.host.stream(origin + '/events', '', () => {}, resolve))
  assert.notEqual(await refused, null)
  f.advance()
  f.refuseNextHello()
  await f.controller.retry()
  assert.equal(f.last(), 'hub_offline')
  assert.equal(f.remote.rendererContext(), undefined)
  f.restoreEpoch()
  await f.controller.retry()
  assert.equal(f.remote.rendererContext()?.storage.getItem('sentinel'), 'saved')
})

test('missing binding or session token denies app access without sending authenticated envelopes', async (t) => {
  let lookup = 0
  const disabled = createRemoteHub(() => { lookup++; return null })
  const controller = createRemoteController(() => {}, disabled, createWorkspaceSession(async () => {}))
  await controller.setIdentity(context.userId, [], [personal])
  assert.equal(lookup, 1)
  assert.equal((await disabled.host.snapshot()).local, null)
  const f = fixture({ noToken: true })
  t.after(f.close)
  await f.open()
  assert.equal((await f.remote.host.snapshot()).local, null)
  assert.deepEqual(f.messages, [])
})

for (const code of ['role_changed', 'workspace_revoked', 'device_revoked'] as const) {
  test(`signed ${code} fences responses and purges without replay`, async (t) => {
    const f = fixture({ holdSend: true })
    t.after(f.close)
    await f.open()
    const renderer = f.remote.rendererContext()!
    renderer.storage.setItem('sentinel', 'old')
    const local = (await renderer.host.snapshot()).local!
    const sending = assert.rejects(renderer.host.call(origin, local.token, 'send', {}, 1000))
    await until(() => f.messages.some((m) => m.m === 'send'))
    f.sockets[0].cut(code)
    await sending
    await until(() => f.last() === (code === 'device_revoked' ? 'device_revoked' : 'active'))
    assert.equal(renderer.storage.getItem('sentinel'), null)
    assert.equal((await renderer.host.snapshot()).local, null)
    assert.equal(f.remote.rendererContext()?.storage.getItem('sentinel') ?? null, null)
    assert.equal(f.messages.filter((m) => m.m === 'send').length, 1)
    assert.equal(f.sockets.length, code === 'device_revoked' ? 1 : 2)
  })
}

for (const loss of ['unsigned reject', 'transport close'] as const) {
  test(`${loss} is offline and retains the partition until a new session attests an epoch change`, async (t) => {
    const f = fixture()
    t.after(f.close)
    await f.open()
    f.remote.rendererContext()?.storage.setItem('sentinel', 'saved')
    if (loss === 'unsigned reject') f.sockets[0].deliver({ t: 'reject', code: 'revoked', stableCode: 'device_revoked' })
    else f.sockets[0].close()
    await until(() => f.last() === 'hub_offline')
    assert.equal(f.remote.rendererContext(), undefined)
    await f.controller.retry()
    assert.equal(f.remote.rendererContext()?.storage.getItem('sentinel'), 'saved')
    f.sockets[1].close()
    await until(() => f.last() === 'hub_offline')
    f.advance()
    await f.controller.retry()
    assert.equal(f.remote.rendererContext()?.storage.getItem('sentinel'), null)
  })
}

test('repeated authenticated epoch mismatch has a bounded recovery and never replays send', async (t) => {
  const f = fixture({ mismatch: true })
  t.after(f.close)
  await f.open()
  const send = async () => {
    const local = (await f.remote.host.snapshot()).local!
    await assert.rejects(f.remote.host.call(origin, local.token, 'send', {}, 1000))
  }
  await send()
  await until(() => f.sockets.length === 2 && f.last() === 'active')
  assert.equal(f.messages.filter((m) => m.m === 'send').length, 1)
  await send()
  await until(() => f.last() === 'unavailable')
  assert.equal(f.sockets.length, 2)
})
