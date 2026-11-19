import test from 'node:test'
import assert from 'node:assert/strict'
import { signerFromSeed, computerIdOf } from './crypto.ts'
import { FakeSocket, hostAccept } from './fake-hub.ts'
import { RelayLink, type LinkState } from './relay-socket.ts'
import type { Channel } from './channel.ts'

const host = signerFromSeed(new Uint8Array(32).fill(1))
const device = signerFromSeed(new Uint8Array(32).fill(3))
function fixture(callbacks: { state?: (s: LinkState, link: RelayLink) => void; channel?: (c: Channel | null, link: RelayLink) => void } = {}) {
  const sockets: FakeSocket[] = []
  const channels: (Channel | null)[] = []
  const link = new RelayLink({ cloudURL: 'https://api.example.test', computerId: computerIdOf(host.publicKey), hostSignPub: host.publicKey, signer: device,
    timings: { pingMs: 1000, silenceMs: 3000, handshakeMs: 500, backoffBaseMs: 10, backoffMaxMs: 40 }, random: () => 1,
    socket: (url, headers) => { const s = new FakeSocket(url, headers); sockets.push(s); return s },
    onState: s => callbacks.state?.(s, link), onChannel: c => { channels.push(c); callbacks.channel?.(c, link) },
  })
  const ready = () => { const s = sockets.at(-1)!; s.open(); s.deliver({ t: 'presence', online: true }); s.deliver(hostAccept(host, s.lastHello()).welcome) }
  return { link, sockets, channels, ready }
}

test('reject pós-ready: lease expira, chamada incerta, geração antiga ignorada e novo handshake sem replay', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  const h = fixture(); h.link.start(); h.ready()
  const old = h.sockets[0]!
  const pending = h.channels.at(-1)!.call('send', { text: 'sintético' })
  old.deliver({ t: 'reject', code: 'leaseExpired' }); old.drop(4001)
  assert.deepEqual(await pending, { kind: 'unknown', reason: 'closed' })
  assert.equal(h.link.state.kind, 'retrying')
  t.mock.timers.tick(50)
  assert.equal(h.sockets.length, 2)
  old.deliver({ t: 'presence', online: true }); old.drop(4001)
  h.ready()
  assert.equal(h.link.state.kind, 'ready')
  assert.equal(h.sockets[1]!.messages().filter(m => m.t === 'hello').length, 1)
  assert.equal(h.sockets[1]!.messages().filter(m => m.t === 'f').length, 0)
  h.link.stop()
})

test('reject pós-ready: terminais, recuo e códigos desconhecidos', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  for (const [code, reason] of [['revoked', 'revoked'], ['unsupportedVersion', 'updateRequired'], ['unauthorized', 'unauthorized'], ['badSignature', 'unauthorized'], ['pairingClosed', 'unauthorized']] as const) {
    const h = fixture(); h.link.start(); h.ready(); h.sockets[0]!.deliver({ t: 'reject', code }); assert.deepEqual([h.link.state.kind, (h.link.state as {reason?: string}).reason], ['stopped', reason]); t.mock.timers.tick(25); assert.equal(h.sockets.length, 1)
  }
  for (const [code, cause] of [['rateLimited', 'rateLimited'], ['desconhecido', 'protocol']] as const) {
    const h = fixture(); h.link.start(); h.ready(); h.sockets[0]!.deliver({ t: 'reject', code }); assert.equal(h.link.state.kind, 'retrying'); assert.equal((h.link.state as {cause?: string}).cause, cause); assert.equal(h.link.currentChannel, null); h.link.stop()
  }
})

test('stop reentrante em connecting, retrying, ready e descarte interrompe recursos', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  for (const phase of ['connecting', 'retrying', 'ready', 'null'] as const) {
    const h = fixture({ state: (s, link) => { if (s.kind === phase) { link.stop(); link.start() } }, channel: (c, link) => { if (phase === 'null' && c === null) { link.stop(); link.start() } } })
    h.link.start()
    if (phase !== 'connecting') {
      if (phase === 'retrying') h.sockets[0]!.drop(1006)
      else { h.ready(); if (phase === 'null') h.sockets[0]!.deliver({ t: 'presence', online: true }) }
    }
    t.mock.timers.tick(50)
    assert.deepEqual(h.link.state, { kind: 'stopped', reason: 'stopped' }, phase)
    assert.equal(h.sockets.length, phase === 'connecting' ? 0 : 1, phase)
    if (phase === 'ready') assert.equal(h.channels.filter(Boolean).length, 0)
    if (phase === 'null') assert.equal(h.sockets[0]!.messages().filter(m => m.t === 'hello').length, 1)
  }
})
