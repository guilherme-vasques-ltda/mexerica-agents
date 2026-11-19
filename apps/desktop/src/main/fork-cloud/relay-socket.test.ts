import test from 'node:test'
import assert from 'node:assert/strict'
import type { Channel, CallResult } from './channel.ts'
import { b64url, computerIdOf, sigCanonical, signerFromSeed, unb64url, verifyEd25519 } from './crypto.ts'
import { FakeSocket, HostCodec, hostAccept } from './fake-hub.ts'
import { deviceRelayURL, RelayLink, type LinkState } from './relay-socket.ts'

const host = signerFromSeed(new Uint8Array(32).fill(1))
const otherHost = signerFromSeed(new Uint8Array(32).fill(7))
const device = signerFromSeed(new Uint8Array(32).fill(3))
const computerId = computerIdOf(host.publicKey)
const CLOUD = 'https://api.example.test'
const fast = { pingMs: 40, silenceMs: 120, handshakeMs: 80, backoffBaseMs: 10, backoffMaxMs: 40, stableMs: 1_000 }

function harness(pinned = host.publicKey) {
  const sockets: FakeSocket[] = []
  const states: LinkState[] = []
  const channels: (Channel | null)[] = []
  const link = new RelayLink({
    cloudURL: CLOUD,
    computerId,
    hostSignPub: pinned,
    signer: device,
    timings: fast,
    random: () => 1,
    socket: (url, headers) => {
      const s = new FakeSocket(url, headers)
      sockets.push(s)
      return s
    },
    onState: (s) => states.push(s),
    onChannel: (c) => channels.push(c),
  })
  const last = () => sockets[sockets.length - 1]!
  /** Abre o socket, anuncia o Hub e responde o hello com `signer` (o Hub verdadeiro por padrão). */
  const ready = (signer = host) => {
    const s = last()
    s.open()
    s.deliver({ t: 'presence', online: true, lastSeenAt: null })
    const { welcome, keys } = hostAccept(host, s.lastHello(), signer)
    s.deliver(welcome)
    return new HostCodec(keys)
  }
  return { link, sockets, states, channels, last, ready }
}

test('URL da ponte: só https (ou loopback de teste) e Codync-Sig verificável do aparelho', () => {
  assert.equal(deviceRelayURL(CLOUD, computerId).toString(), `wss://api.example.test/v1/relay/device/${computerId}?v=1`)
  assert.equal(deviceRelayURL('http://127.0.0.1:8787', computerId).protocol, 'ws:')
  assert.throws(() => deviceRelayURL('http://evil.example', computerId), /httpsRequired/)
  assert.throws(() => deviceRelayURL('https://x.test/caminho', computerId), /invalidCloudURL/)
  assert.throws(() => deviceRelayURL(CLOUD, '../etc'), /invalidComputerId/)

  const h = harness()
  h.link.start()
  const s = h.last()
  const fields = Object.fromEntries(s.headers['Codync-Sig']!.split(',').map((p) => p.split('=') as [string, string]))
  assert.equal(fields.kid, b64url(device.publicKey))
  const url = new URL(s.url)
  const canonical = sigCanonical('GET', url.host, url.pathname + url.search, Number(fields.ts), fields.nonce!, new Uint8Array())
  assert.ok(verifyEd25519(device.publicKey, unb64url(fields.sig, 64)!, new TextEncoder().encode(canonical)))
  h.link.stop()
})

test('fluxo: presença → hello → welcome com chave pinada → RPC e evento cifrados', async () => {
  const h = harness()
  h.link.start()
  const codec = h.ready()
  assert.equal(h.link.state.kind, 'ready')
  const channel = h.channels.at(-1)!
  const events: unknown[] = []
  channel.subscribe('events', { since: 0, client: 'mac' }, (ev) => events.push(ev))
  const call = channel.call('history', { botId: 'b1' })
  const sent = h.last().messages().filter((m) => m.t === 'f')
  const inner = sent.map((f) => codec.open(f as { c: number; d: string })).filter(Boolean) as { id: number }[]
  assert.deepEqual(inner.map((m) => Object.keys(m).sort().join()), ['b,id,sub', 'b,id,m'])
  for (const f of codec.seal({ id: inner[1]!.id, ok: { entries: [] } })) h.last().deliver(f)
  for (const f of codec.seal({ id: inner[0]!.id, ev: { type: 'hello' } })) h.last().deliver(f)
  assert.deepEqual(await call, { kind: 'ok', value: { entries: [] } })
  assert.deepEqual(events, [{ type: 'hello' }])
  h.link.stop()
  assert.equal(h.channels.at(-1), null, 'parar descarta o canal')
})

test('welcome que não confere com a chave pinada: identidade não verificada, sem afirmar Hub trocado', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  const h = harness()
  h.link.start()
  h.ready(otherHost)
  t.mock.timers.tick(5)
  assert.deepEqual(h.link.state, { kind: 'stopped', reason: 'hubUnverified', message: 'Não foi possível verificar a identidade do Hub.' })
  t.mock.timers.tick(60)
  assert.equal(h.sockets.length, 1, 'nenhuma reconexão para a mesma chave')
  assert.equal(h.channels.length, 0, 'nenhum canal foi entregue')
  h.link.start()
  assert.equal(h.sockets.length, 1, 'parada não reabre')

  const malformed = harness()
  malformed.link.start()
  malformed.last().open()
  malformed.last().deliver({ t: 'presence', online: true })
  malformed.last().deliver({ t: 'welcome', v: 1, ek: 'curto', sig: 'x' })
  assert.equal((malformed.link.state as { reason?: string }).reason, 'hubUnverified', 'welcome malformado também não verifica')
  assert.equal(malformed.channels.length, 0)
  t.mock.timers.tick(30)
  assert.equal(malformed.sockets.length, 1)
})

test('reject antes do welcome: aperto de mão recusado com o código, sem autenticar o Hub nem tentar de novo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  for (const code of ['badSignature', 'unauthorized', 'pairingClosed'] as const) {
    const h = harness()
    h.link.start()
    h.last().open()
    h.last().deliver({ t: 'presence', online: true })
    h.last().deliver({ t: 'reject', code, message: 'texto vindo do outro lado' })
    assert.deepEqual(h.link.state, { kind: 'stopped', reason: 'handshakeRefused', message: 'O Hub recusou o aperto de mão deste Mac.', rejectCode: code }, code)
    assert.equal(h.channels.length, 0, `${code}: nenhum canal`)
    h.last().drop(4001)
    t.mock.timers.tick(30)
    h.link.start()
    assert.equal(h.sockets.length, 1, `${code}: sem nova tentativa e sem reabrir`)
  }
})

test('revogação com socket aberto: chamada em voo vira unknown e não há nova tentativa', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  const h = harness()
  h.link.start()
  h.ready()
  const pending = h.channels.at(-1)!.call('send', { botId: 'b1', text: 'oi', clientNonce: 'n-1' })
  h.last().drop(4003, 'revoked')
  assert.deepEqual(await pending, { kind: 'unknown', reason: 'closed' } satisfies CallResult)
  assert.equal(h.link.state.kind, 'stopped')
  assert.equal((h.link.state as { reason: string }).reason, 'revoked')
  t.mock.timers.tick(60)
  assert.equal(h.sockets.length, 1)
  assert.equal(h.channels.at(-1), null)
})

test('rejeições preservadas: revoked e updateRequired param; leaseExpired tenta de novo com recuo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  for (const [code, expected] of [['revoked', 'revoked'], ['unsupportedVersion', 'updateRequired']] as const) {
    const h = harness()
    h.link.start()
    h.last().open()
    h.last().deliver({ t: 'presence', online: true })
    h.last().deliver({ t: 'reject', code, message: 'x' })
    t.mock.timers.tick(1)
    assert.equal((h.link.state as { reason?: string }).reason, expected, code)
    h.last().drop(4001)
    assert.equal(h.sockets.length, 1, `${code} não reconecta`)
  }
  const h = harness()
  h.link.start()
  h.last().open()
  h.last().deliver({ t: 'presence', online: true })
  h.last().deliver({ t: 'reject', code: 'leaseExpired', message: 'x' })
  t.mock.timers.tick(1)
  assert.equal(h.link.state.kind, 'retrying')
  assert.equal((h.link.state as { cause: string }).cause, 'leaseExpired')
  h.last().drop(4001, 'depois do reject')
  t.mock.timers.tick(30)
  assert.equal(h.sockets.length, 2, 'o fechamento 4001 que segue um leaseExpired não encerra a ligação')
  h.link.stop()
})

test('Hub sai da ponte: chamadas viram unknown; ao voltar, novo hello e novo canal', async () => {
  const h = harness()
  h.link.start()
  h.ready()
  const first = h.channels.at(-1)!
  const pending = first.call('createBot', { name: 'x' })
  h.last().deliver({ t: 'presence', online: false, lastSeenAt: 123 })
  assert.deepEqual(await pending, { kind: 'unknown', reason: 'closed' })
  assert.deepEqual(h.link.state, { kind: 'hubOffline', lastSeenAt: 123 })
  assert.equal(h.channels.at(-1), null)
  const hellos = h.last().messages().filter((m) => m.t === 'hello').length
  h.last().deliver({ t: 'presence', online: true })
  const { welcome } = hostAccept(host, h.last().lastHello())
  h.last().deliver(welcome)
  assert.equal(h.last().messages().filter((m) => m.t === 'hello').length, hellos + 1)
  assert.notEqual(h.channels.at(-1), first)
  assert.equal(h.link.state.kind, 'ready')
  h.link.stop()
})

test('quadro repetido (replay) quebra o canal e reconecta; respostas antigas não chegam ao canal novo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  const h = harness()
  h.link.start()
  const codec = h.ready()
  const pending = h.channels.at(-1)!.call('history', {})
  const [frame] = codec.seal({ id: 99, ev: {} })
  h.last().deliver(frame)
  h.last().deliver(frame)
  assert.deepEqual(await pending, { kind: 'unknown', reason: 'closed' })
  assert.equal(h.link.state.kind, 'retrying')
  assert.equal(h.sockets[0]!.closedWith, 4002)
  t.mock.timers.tick(30)
  h.sockets[0]!.deliver({ t: 'presence', online: true })
  assert.equal(h.sockets[0]!.messages().filter((m) => m.t === 'hello').length, 1, 'socket velho é ignorado')
  h.link.stop()
})

test('silêncio, rekey e falha antes de abrir', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'], now: 1_000 })
  const quiet = harness()
  quiet.link.start()
  quiet.last().open()
  // Primeiro há ping; só depois o teste vence o limite de silêncio.
  t.mock.timers.tick(fast.pingMs)
  assert.ok(quiet.sockets[0]!.sent.includes('{"t":"ping"}'), 'ping exato')
  t.mock.timers.tick(fast.silenceMs - fast.pingMs + fast.pingMs / 2)
  assert.ok(quiet.states.some((s) => s.kind === 'retrying' && s.cause === 'silent'))
  assert.ok(quiet.sockets[0]!.sent.includes('{"t":"ping"}'), 'ping exato')
  quiet.link.stop()

  const rekey = harness()
  rekey.link.start()
  rekey.ready()
  rekey.last().drop(4011)
  const retry = rekey.states.at(-1) as { kind: string; delayMs: number; cause: string }
  assert.deepEqual([retry.kind, retry.cause, retry.delayMs], ['retrying', 'rekey', 0])
  t.mock.timers.tick(5)
  assert.equal(rekey.sockets.length, 2)
  rekey.link.stop()

  const refused = harness()
  refused.link.start()
  refused.last().drop(1006)
  t.mock.timers.tick(30)
  refused.last().drop(1006)
  const state = refused.states.at(-1) as { connectFailures: number }
  assert.equal(state.connectFailures, 2, 'falhas antes de abrir ficam visíveis para a camada de conta conferir o acesso')
  refused.link.stop()
})

test('parar ignora qualquer coisa que o socket antigo ainda mande, e a mesma ligação não reabre', () => {
  const h = harness()
  h.link.start()
  const codec = h.ready()
  h.link.stop()
  h.link.start()
  assert.equal(h.sockets.length, 1, 'uma ligação parada é uma encarnação encerrada')
  const before = h.channels.length
  for (const f of codec.seal({ id: 1, ok: {} })) h.last().deliver(f)
  h.last().deliver({ t: 'presence', online: true })
  h.last().drop(1006)
  assert.equal(h.channels.length, before)
  assert.deepEqual(h.link.state, { kind: 'stopped', reason: 'stopped' })
})
