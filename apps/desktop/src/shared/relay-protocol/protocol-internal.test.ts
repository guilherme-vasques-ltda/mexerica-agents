import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { nodeBackend } from './backend-node.ts'
import { nobleBackend } from './backend-noble.ts'
import { createProtocolInternals } from './protocol-internal.ts'
import * as publicApi from './protocol.ts'

const { keys, requestSig, handshake } = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8'))
const bytes = (s: string) => publicApi.unb64url(s)!
const welcome = { t: 'welcome', ek: handshake.hostEphPub, sig: handshake.hs2Sig }

function pendingSignature() {
  let resolve!: (signature: Uint8Array) => void
  const promise = new Promise<Uint8Array>((done) => { resolve = done })
  return { promise, resolve }
}

function fixture(backend = nodeBackend) {
  const { protocol, createSigningSlot } = createProtocolInternals({
    ...backend,
    random(size) { return bytes(size === 16 ? requestSig.nonce : handshake.n) },
    x25519: { ephemeral: () => backend.x25519.ephemeral(bytes(handshake.deviceEphPriv)) },
  })
  let now = 10
  const slot = createSigningSlot(() => now, () => requestSig.ts)
  const signer = {
    publicKey: bytes(keys.deviceSignPub),
    async sign(input: Uint8Array) {
      const encoded = protocol.b64url(input)
      if (encoded === handshake.hs1SignInput) return bytes(handshake.hs1Sig)
      const requestInput = new TextEncoder().encode(protocol.sigCanonical(requestSig.method, requestSig.authority, requestSig.pathAndQuery, requestSig.ts, requestSig.nonce, new Uint8Array()))
      assert.deepEqual(input, requestInput)
      return bytes(requestSig.header.split(',sig=')[1]!)
    },
  }
  return { protocol, slot, signer, pin: bytes(keys.hostSignPub), expire: () => { now += 10 } }
}

for (const [name, backend] of [['node', nodeBackend], ['noble', nobleBackend]] as const) {
  test(`assinatura assíncrona preserva bytes, ordem e chaves no backend ${name}`, async () => {
    const { protocol, slot, signer, pin } = fixture(backend)
    const syncSigner = protocol.signerFromSeed(bytes(keys.deviceSignSeed))
    const attempt = slot.begin(signer, pin, 10)
    const url = new URL(`https://${requestSig.authority}${requestSig.pathAndQuery}`)
    const syncHello = protocol.deviceHello(syncSigner, pin, { caps: ['browserFramesV1'] })
    const effects: string[] = []
    const syncHeader = protocol.signatureHeader(syncSigner, 'GET', url, new Uint8Array(), requestSig.ts, requestSig.nonce)
    assert.equal(attempt.receiveWelcome(welcome, () => { effects.push('welcome cedo') }), false)
    assert.equal(await attempt.sendUpgrade(url, (href, header) => {
      assert.equal(href, url.href)
      assert.equal(header, syncHeader)
      assert.equal(`Codync-Sig: ${header}`, requestSig.header)
      effects.push('upgrade')
    }), true)
    assert.equal(await attempt.sendHello((hello) => {
      assert.equal(JSON.stringify(hello), JSON.stringify(syncHello.hello))
      effects.push('hello')
      assert.equal(attempt.receiveWelcome(welcome, (channelKeys, context) => {
        assert.deepEqual(channelKeys, syncHello.finish(welcome))
        assert.equal(attempt.ownsContext(context), true)
        assert.deepEqual(Reflect.ownKeys(context), [])
        effects.push('welcome')
        attempt.cancel()
        assert.equal(attempt.ownsContext(context), false)
      }), true)
    }, ['browserFramesV1']), true)
    assert.deepEqual(effects, ['upgrade', 'hello', 'welcome'])
  })
}

for (const replacement of ['B', 'A→B→A', 'cancel', 'deadline'] as const) {
  for (const stage of ['upgrade', 'hello'] as const) {
    test(`assinatura tardia de ${stage} não produz efeito após ${replacement}`, async () => {
      const { slot, signer, pin, expire } = fixture()
      const late = pendingSignature()
      const a = { ...signer, sign: () => late.promise }
      const frames: unknown[] = []
      const attempt = slot.begin(a, pin, 10)
      const url = new URL(`https://${requestSig.authority}${requestSig.pathAndQuery}`)
      const pending = stage === 'hello'
        ? attempt.sendHello((hello) => { frames.push(hello) })
        : attempt.sendUpgrade(url, (_url, header) => { frames.push(header) })
      if (replacement === 'cancel') attempt.cancel()
      else if (replacement === 'deadline') expire()
      else {
        slot.begin({ ...signer, publicKey: new Uint8Array(32) }, pin, 10)
        if (replacement === 'A→B→A') slot.begin(a, pin, 10)
      }
      late.resolve(bytes(stage === 'hello' ? handshake.hs1Sig : requestSig.header.split(',sig=')[1]!))
      assert.equal(await pending, false)
      assert.equal(attempt.receiveWelcome(welcome, () => { frames.push('commit') }), false)
      assert.deepEqual(frames, [])
    })
  }
}

test('assinatura rejeitada não cai no caminho síncrono nem tenta de novo', async () => {
  const { slot, signer, pin } = fixture()
  let calls = 0
  let sends = 0
  const attempt = slot.begin({ ...signer, async sign() { calls++; throw new Error('fixture') } }, pin, 10)
  const send = (): undefined => { sends++ }
  assert.equal(await attempt.sendHello(send), false)
  assert.equal(await attempt.sendHello(send), false)
  assert.equal(calls, 1)
  assert.equal(sends, 0)
})

for (const [failure, sig] of [['malformado', 'invalid'], ['com assinatura incorreta', handshake.hs1Sig]] as [string, string][]) {
  test(`welcome ${failure} não instala chaves nem entrega contexto`, async () => {
    const { slot, signer, pin } = fixture()
    const attempt = slot.begin(signer, pin, 10)
    assert.equal(await attempt.sendHello(() => {}), true)
    let effects = 0
    const commit = (): undefined => { effects++ }
    assert.equal(await attempt.sendHello(commit), false)
    assert.equal(attempt.receiveWelcome({ ...welcome, sig }, commit), false)
    assert.equal(attempt.receiveWelcome(welcome, commit), false)
    assert.equal(effects, 0)
  })
}

test('pin e chave do aparelho são os da tentativa que iniciou a assinatura', async () => {
  const { slot, signer, pin } = fixture()
  const late = pendingSignature()
  const attempt = slot.begin({ ...signer, sign: () => late.promise }, pin, 10)
  const pending = attempt.sendHello(() => {})
  pin.fill(0)
  signer.publicKey.fill(0)
  late.resolve(bytes(handshake.hs1Sig))
  assert.equal(await pending, true)
  let committed = false
  assert.equal(attempt.receiveWelcome(welcome, () => { committed = true }), true)
  assert.equal(committed, true)
  const differentKey = slot.begin(signer, bytes(keys.hostSignPub), 10)
  let sent = false
  assert.equal(await differentKey.sendHello(() => { sent = true }), false)
  assert.equal(sent, false)
})

test('substituir a tentativa invalida o welcome pendente e o contexto anterior', async () => {
  const { slot, signer, pin } = fixture()
  const active = slot.begin(signer, pin, 10)
  assert.equal(await active.sendHello(() => {}), true)
  assert.equal(active.receiveWelcome(welcome, (_keys, context) => {
    assert.equal(active.ownsContext(context), true)
    const replacement = slot.begin(signer, pin, 10)
    assert.equal(active.ownsContext(context), false)
    assert.equal(replacement.ownsContext(context), false)
  }), true)
  const waiting = slot.begin(signer, pin, 10)
  assert.equal(await waiting.sendHello(() => {}), true)
  slot.begin(signer, pin, 10)
  let committed = false
  assert.equal(waiting.receiveWelcome(welcome, () => { committed = true }), false)
  assert.equal(committed, false)
})

test('a superfície pública continua síncrona', () => {
  assert.deepEqual(Object.keys(publicApi).sort(), ['CHUNK', 'HandshakeError', 'b64url', 'concat', 'createProtocol', 'unb64url'])
  assert.deepEqual(Object.keys(publicApi.createProtocol(nodeBackend)), [
    'b64url', 'unb64url', 'concat', 'sha256', 'random', 'cidRaw', 'computerIdOf', 'signerFromSeed', 'newDeviceSeed', 'verifyEd25519',
    'ephemeral', 'sigCanonical', 'signatureHeader', 'hs1Input', 'transcriptHash', 'hkdfExtract', 'channelKeys', 'deviceHello',
    'HandshakeError', 'CHUNK', 'sealFrame', 'openFrame', 'sasCommit', 'sasCode',
  ])
})
