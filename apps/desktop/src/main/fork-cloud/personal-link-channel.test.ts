import assert from 'node:assert/strict'
import test from 'node:test'
import * as nodeCrypto from 'node:crypto'
import * as perf from 'node:perf_hooks'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import { createServer, type Socket } from 'node:net'
import { EventEmitter } from 'node:events'
import * as crypto from './crypto.ts'
import * as w from './personal-link-wire.ts'
import * as b from './personal-link-channel-wire.ts'
import { PersonalLinkSocket } from './personal-link-socket.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type { Principal } from './principal.ts'
// Golden público independente (Python SHA/HMAC); bytes originais SHA256 3b46d2b735de82ff72fd0b912e396d826dee0fa5de5f93758eff9c073d5a651f.
const VECTOR_JSON = String.raw`{
  "schema": "donna.personal-link/pr-b-vectors/v1",
  "note": "Material sintético público; transcrição mínima para KDF/AAD, não cadeia admitida. Gerado offline com SHA256/HMAC RFC5869, sem executar implementação B.",
  "transcript": [
    "donna.personal-link/channel-final/v1",
    1,
    [
      "CH"
    ],
    [
      "CR"
    ],
    [
      "W"
    ]
  ],
  "transcriptHex": "5b22646f6e6e612e706572736f6e616c2d6c696e6b2f6368616e6e656c2d66696e616c2f7631222c312c5b224348225d2c5b224352225d2c5b2257225d5d",
  "TH": "521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07",
  "saltHex": "521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07",
  "sharedHex": "0707070707070707070707070707070707070707070707070707070707070707",
  "sessionId": "CQkJCQkJCQkJCQkJCQkJCQ",
  "nonce0Hex": "000000000000000000000000",
  "directions": {
    "c2h": {
      "info": "[\"donna.personal-link/channel-key/v1\",1,\"manager\",\"c2h\",\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\"]",
      "keyHex": "d9ed37d89b87ce92de40d20bd950898b2aac1f4dcab2cde7894087bebdde1509",
      "aad0": "[\"donna.personal-link/frame/v1\",1,\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\",\"manager\",\"c2h\",0]"
    },
    "h2c": {
      "info": "[\"donna.personal-link/channel-key/v1\",1,\"manager\",\"h2c\",\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\"]",
      "keyHex": "60be1195dd027a681b50eaa1b55f24d6240f2f1469c59ca1d56a2220baf9398a",
      "aad0": "[\"donna.personal-link/frame/v1\",1,\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\",\"manager\",\"h2c\",0]"
    }
  }
}
`
const id = (n: number) => crypto.b64url(nodeCrypto.randomBytes(n))
const boot = 'c5386ca1-bad0-4cfb-b19b-88badf199264'
const C = 'c'.repeat(64)
function records() {
  const H = crypto.signerFromSeed(crypto.newDeviceSeed()), T = crypto.signerFromSeed(crypto.newDeviceSeed()), h = crypto.b64url(H.publicKey), t = crypto.b64url(T.publicKey), G = id(32), R = id(32)
  const d: w.J = [w.D430.deadline, 1, boot, R, 100, 30000]
  const ch = w.signT(H, [w.D430.challenge, 1, h, G, C, R, id(16), t, id(32), id(32), d, id(16), 1, w.P0])
  const ac = w.accept(T, ch), ra = w.signT(H, [w.D430.admission, 1, ...ch[0].slice(2, 8), ch[0][11], 1, id(16), w.hashJ(ch), w.hashJ(ac), w.P0])
  const pc = w.persisted(T, ra, [id(32), d, R]), rc = w.signT(H, [w.D430.complete, 1, h, G, C, R, ra[0][10], w.hashJ(ra), w.hashJ(pc), w.P0])
  const m = w.signT(T, [w.D430.management, 1, h, G, C, R, id(16), t, w.hashJ(ra), 1, w.P1, id(32), d, 1]), mr = w.signT(H, [w.D430.receipt, 1, h, G, C, R, m[0][6], t, ra[0][8], w.hashJ(ra), w.hashJ(m), w.P1])
  const map = new Map<string, Buffer>([['admission', w.j(ra)], ['complete', w.j(rc)], ['management', w.j(m)], ['receipt', w.j(mr)], [`accept.${w.hashJ(ac).slice(0, 32)}`, w.j([ch, ac])], [`persisted.${w.hashJ(pc).slice(0, 32)}`, w.j(pc)], [`management.${w.hashJ(m).slice(0, 32)}`, w.j(m)]])
  return { H, T, R, root: b.chain([ra, rc, m, mr, [ch, ac], pc, m], t, C), map }
}
/** Hub fixture fala TCP/WebSocket real. Sem cofre de motores nem qualificação física. */
async function fixture() {
  const r = records(), sockets = new Set<Socket>(), power = new EventEmitter()
  const state = { available: true, signatures: 0, reads: 0, opens: 0, writes: 0, beforeChallenge: () => {}, wrong: false, hold: null as null | (() => void), release: null as null | (() => void), ops: [] as string[] }
  const server = createServer(socket => {
    sockets.add(socket); socket.on('close', () => sockets.delete(socket)); socket.on('error', () => {})
    let buffer = Buffer.alloc(0), upgraded = false, stage = 0, ch: w.Signed, cr: w.Signed, welcome: w.Signed, incoming: b.Cipher, outgoing: b.Cipher, sid = '', th = '', d: w.J
    const eph = crypto.ephemeral()
    const send = (bytes: Buffer) => {
      const head = Buffer.alloc(bytes.length < 126 ? 2 : 4); head[0] = 0x82
      if (head.length === 2) head[1] = bytes.length
      else { head[1] = 126; head.writeUInt16BE(bytes.length, 2) }
      socket.write(Buffer.concat([head, bytes]))
    }
    const handle = (bytes: Buffer) => {
      if (stage === 0) {
        assert.ok(b.same(w.readJ(bytes), b.open(r.root)))
        state.beforeChallenge()
        d = [w.D430.deadline, 1, boot, r.R, 100, 30000]
        ch = w.signT(r.H, [b.domain('channel-challenge'), 1, 'manager', r.root.H, r.root.G, C, r.R, id(32), id(32), crypto.b64url(eph.pub), r.root.T, r.root.ra, r.root.chain, w.P1, 0, d])
        send(w.j(ch)); stage++
      } else if (stage === 1) {
        cr = w.verifySigned(w.readJ(bytes), b.domain('channel-response'), 15, r.root.T)
        sid = id(16); d = [w.D430.deadline, 1, boot, r.R, 100, 300000]
        welcome = w.signT(r.H, [b.domain('channel-welcome'), 1, r.root.H, r.root.G, C, r.R, 'manager', r.root.T, w.hashJ([b.domain('channel-transcript'), 1, ch, cr]), sid, d, w.P1, 0])
        const transcript: w.J = [b.domain('channel-final'), 1, ch, cr, welcome]; th = w.hashJ(transcript)
        const shared = eph.dh(crypto.unb64url(cr[0][9], 32)!)
        incoming = new b.Cipher(shared, transcript, sid, 'c2h'); outgoing = new b.Cipher(shared, transcript, sid, 'h2c'); shared.fill(0)
        send(w.j(welcome)); stage++
      } else {
        const v = incoming.read(bytes) as readonly w.J[]
        state.ops.push(v[0] as string)
        if (stage === 2) {
          assert.ok(b.same(v, [b.domain('channel-finish'), 1, sid, th, w.hashJ(welcome)]))
          send(outgoing.seal([b.domain('channel-ready'), 1, sid, th])); stage++
        } else {
          const reply = () => send(outgoing.seal([b.domain('link-status'), 1, state.wrong ? id(16) : v[2], r.root.H, r.root.G, C, r.R, r.root.T, r.root.ra, r.root.chain, w.P1, 0, sid, th, 'active', d]))
          if (state.hold) { state.release = reply; state.hold() } else reply()
        }
      }
    }
    socket.on('data', bytes => {
      buffer = Buffer.concat([buffer, bytes])
      try {
        if (!upgraded) {
          const end = buffer.indexOf('\r\n\r\n'); if (end < 0) return
          const text = buffer.subarray(0, end).toString(), key = /Sec-WebSocket-Key: ([^\r]+)/.exec(text)?.[1]
          assert.ok(text.startsWith('GET /personal-link/channel HTTP/1.1\r\n')); assert.ok(key)
          const accept = nodeCrypto.createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64')
          socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`)
          buffer = buffer.subarray(end + 4); upgraded = true
        }
        while (buffer.length >= 2) {
          assert.equal(buffer[0], 0x82); assert.ok(buffer[1] & 128)
          const small = buffer[1] & 127; if (small === 126 && buffer.length < 8) return
          const size = small === 126 ? buffer.readUInt16BE(2) : small, head = small === 126 ? 8 : 6
          if (buffer.length < head + size) return
          const mask = buffer.subarray(head - 4, head), body = Buffer.from(buffer.subarray(head, head + size)); buffer = buffer.subarray(head + size)
          for (let i = 0; i < body.length; i++) body[i] ^= mask[i % 4]
          handle(body)
        }
      } catch { socket.destroy() }
    })
  })
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  const port = (server.address() as { port: number }).port
  const vault = { async read(name: string) { state.reads++; return r.map.get(name) ?? null }, ready() { if (!state.available) throw new Error('vault') }, async signer(create: boolean) { assert.equal(create, false); return { publicKey: r.T.publicKey, sign(bytes: Uint8Array) { vault.ready(); state.signatures++; return r.T.sign(bytes) } } } }
  const mod = loadMainModule<typeof import('./personal-link-channel.ts')>('fork-cloud/personal-link-channel', {
    'node:crypto': nodeCrypto, 'node:perf_hooks': perf, electron: { powerMonitor: power }, './crypto': crypto, './personal-link-wire': w, './personal-link-channel-wire': b, './personal-link-socket': { PersonalLinkSocket }, './personal-link-vault': { PersonalLinkVault: { async reopen() { state.opens++; return vault } } },
  }, { Buffer, setTimeout, clearTimeout })
  const channel = new mod.PersonalLinkChannel()
  let current = true
  const p = { namespace: C, current: () => current } as Principal
  const profile = { id: 'fixture', host: '127.0.0.1', user: null, port: null, remotePort: port, identityFile: null }
  return { ...state, state, r, channel, power, p, profile, change() { current = false }, async close() { channel.dispose(); for (const socket of sockets) socket.destroy(); await new Promise<void>(resolve => server.close(() => resolve())) } }
}

test('B Main real: F/ACK/status, DTO reduzido e invalidação em energia', async () => {
  const f = await fixture()
  try {
    const result = await f.channel.query(f.p, 'fixture', f.profile)
    assert.equal(result.kind, 'ownStatus')
    assert.deepEqual(f.state.ops, [b.domain('channel-finish'), b.domain('link-status-request')])
    assert.equal(f.state.signatures, 1); assert.equal(f.state.writes, 0)
    assert.ok(result.kind === 'ownStatus' && result.validForMs > 0 && result.validForMs <= 30000)
    assert.equal('session' in result, false); assert.equal('th' in result, false)
    f.power.emit('suspend'); assert.equal(f.channel.snapshot(), null)
  } finally { await f.close() }
})
test('B Main: perda de cofre antes CR não assina nem envia CR', async () => {
  const f = await fixture()
  try {
    f.state.beforeChallenge = () => { f.state.available = false }
    assert.equal((await f.channel.query(f.p, 'fixture', f.profile)).kind, 'unavailable')
    assert.equal(f.state.signatures, 0); assert.deepEqual(f.state.ops, [])
  } finally { await f.close() }
})
test('B Main: correlação incorreta encerra e resolve Promise', async () => {
  const f = await fixture()
  try { f.state.wrong = true; assert.equal((await f.channel.query(f.p, 'fixture', f.profile)).kind, 'unavailable'); assert.equal(f.channel.snapshot(), null) } finally { await f.close() }
})
test('B Main: retorno tardio depois de stop/conta não publica snapshot', async () => {
  const f = await fixture()
  try {
    let observed!: () => void
    const held = new Promise<void>(resolve => { observed = resolve })
    f.state.hold = observed
    const query = f.channel.query(f.p, 'fixture', f.profile)
    await held; f.change(); f.channel.stop(); f.state.release?.()
    assert.equal((await query).kind, 'unavailable'); assert.equal(f.channel.snapshot(), null)
  } finally { await f.close() }
})

test('B cifra: replay/reflexão/tag/direção e forma estrita', () => {
  const transcript: w.J = [b.domain('channel-final'), 1, ['CH'], ['CR'], ['W']], sid = id(16), shared = nodeCrypto.randomBytes(32)
  const sender = new b.Cipher(shared, transcript, sid, 'c2h'), receiver = new b.Cipher(shared, transcript, sid, 'c2h')
  const bytes = sender.seal(['Q', 1])
  assert.deepEqual(receiver.read(bytes), ['Q', 1]); assert.throws(() => receiver.read(bytes))
  assert.throws(() => new b.Cipher(shared, transcript, sid, 'h2c').read(bytes))
  const frame = JSON.parse(bytes.toString()); frame[2] = `${frame[2].slice(0, -2)}AA`
  assert.throws(() => new b.Cipher(shared, transcript, sid, 'c2h').read(Buffer.from(JSON.stringify(frame))))
  assert.throws(() => new b.Cipher(shared, transcript, sid, 'c2h').read(Buffer.from(` ${bytes}`)))
  sender.dispose(); assert.throws(() => sender.seal(['Q']))
  assert.throws(() => crypto.ephemeral().dh(new Uint8Array(32)))
})


test('B vetor compartilhado: SHA único, salt RAW32, info/AAD próprios e nonce zero', () => {
  const vector = JSON.parse(VECTOR_JSON)
  assert.equal(w.hashJ(vector.transcript), vector.TH)
  for (const direction of ['c2h', 'h2c'] as const) {
    const cipher = new b.Cipher(Buffer.from(vector.sharedHex, 'hex'), vector.transcript, vector.sessionId, direction)
    const raw = JSON.parse(cipher.seal(['Q', 1]).toString())
    const plain = chacha20poly1305(Buffer.from(vector.directions[direction].keyHex, 'hex'), Buffer.from(vector.nonce0Hex, 'hex'), Buffer.from(vector.directions[direction].aad0)).decrypt(Buffer.from(raw[2], 'base64url'))
    assert.deepEqual(w.readJ(plain), ['Q', 1])
    cipher.dispose()
  }
})
