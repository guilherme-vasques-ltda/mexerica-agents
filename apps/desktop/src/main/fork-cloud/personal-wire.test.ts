import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { b64url, ephemeral, signerFromSeed, unb64url } from './crypto.ts'
import { PersonalVerifier, personalBoxOpen, personalBoxSeal, personalDomain, personalHash, verifyPersonalCeremony, type PersonalKind } from './personal-wire.ts'
import { WIRE_SHAPES } from './personal-shapes.ts'
import type { PersonalContext } from './personal-protocol.ts'
const vector = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8')).personalBootstrap
const v = vector.wireVectors, raw = (x: unknown) => Buffer.from(JSON.stringify(x))
const verifier = () => new PersonalVerifier(vector.context as PersonalContext, v.keys)
const now = { kind: 'current', now: v.now } as const
const signer = (role: 'app' | 'hub' | 'issuer') => signerFromSeed(unb64url(v.seeds[role], 32)!)
const seal = (role: 'app' | 'hub' | 'issuer', p: unknown[]) => [p, b64url(signer(role).sign(raw(p)))]

test('PL1 C2: todos os 30 envelopes, T, cadeia e POP têm bytes criptográficos compartilhados', () => {
  const c = verifier(), packet = verifyPersonalCeremony(c, Buffer.from(v.packetCanonical), now)
  assert.deepEqual(packet.tuple, v.tuple); assert.equal(packet.hash, v.tupleHash)
  assert.equal(Object.keys(WIRE_SHAPES).length, 30)
  for (const kind of Object.keys(WIRE_SHAPES) as PersonalKind[]) {
    const wire = v.packet[kind] ?? v.alternative[kind]
    const checked = c.verify(kind, raw(wire), now)
    assert.equal(checked.hash, v.payloadHashes[kind]); assert.equal(checked.wire, JSON.stringify(wire))
  }
  assert.equal(c.verify('channel-open', raw(v.hostOpen), now).payload[2], 'host')
  assert.deepEqual(c.verify('read', raw(v.readHub), now, 'hub').payload, v.packet.read[0])
  const request = { role: 'host' as const, kid: v.keys.hub, httpNonce: v.hostOpen[0][8], pathAndQuery: v.hostOpen[0][9], httpUntil: v.hostOpen[0][12] }
  assert.equal(c.verifyOpen(raw(v.hostOpen), now, request).hash, c.verify('channel-open', raw(v.hostOpen), now).hash)
  for (const patch of [{ kid: v.keys.app }, { httpNonce: 'invalid' }, { pathAndQuery: '/v1/personal-relay/device' }]) assert.throws(() => c.verifyOpen(raw(v.hostOpen), now, { ...request, ...patch }))
  assert.equal(verifyPersonalCeremony(c, raw(v.alternative), now).tuple[2], 'device')
  const B = packet.originals['box-challenge']!, CA = packet.originals['app-consent']!, CH = packet.originals['local-consent']!
  assert.equal(personalBoxOpen(unb64url(v.seeds.box, 32)!, B, v.tupleHash, CA.hash, CH.hash), v.pop.secret)
  assert.equal(personalBoxSeal(ephemeral(unb64url(v.seeds.ephemeral, 32)!), v.keys.box, v.pop.aad, v.pop.secret), v.pop.ciphertext)
})

test('PL1 C2: assinatura, C, domínio, role, caps, tupla e antecedente trocados recusam', () => {
  const c = verifier()
  for (const [kind, role, index, value] of [
    ['channel-open', 'app', 2, 'host'], ['channel-open', 'app', 3, 'possession'],
    ['host-state', 'issuer', 12, ['owner.devices', 'control']], ['host-state', 'issuer', 14, 1],
    ['activation', 'hub', 9, v.keys.app], ['proof', 'app', 5, '0'.repeat(64)],
    ['local-consent', 'hub', 3, '0'.repeat(64)],
  ] as const) {
    const packet = structuredClone(v.packet), p = packet[kind][0]; p[index] = value
    packet[kind] = seal(role, p)
    assert.throws(() => verifyPersonalCeremony(c, raw(packet), now), kind)
  }
  const p = structuredClone(v.packet['host-state'][0]); p[1][0] = 'ACS4'
  assert.throws(() => c.verify('host-state', raw(seal('issuer', p)), now))
  p[1] = vector.context; p[0] = personalDomain('state')
  assert.throws(() => c.verify('host-state', raw(seal('issuer', p)), now))
  assert.throws(() => c.verify('host-state', raw(seal('hub', v.packet['host-state'][0])), now))
  assert.throws(() => c.verify('channel-open', raw([...v.hostOpen, null]), now))
  const current = { kind: 'current', now: v.now + 600000 } as const
  assert.throws(() => c.verify('grant', raw(v.packet.grant), current))
  assert.equal(c.verify('grant', raw(v.packet.grant), { kind: 'committed', at: v.packet.receipt[0][7] }).hash, v.payloadHashes.grant)
  // Não há resultado de permissão a partir do S histórico.
  assert.equal(c.verify('state', raw(v.packet.state), current).payload[5], 'revoked')
  assert.equal(verifyPersonalCeremony(c, Buffer.from(v.packetCanonical), { kind: 'historical', now: current.now }).hash, v.tupleHash)
})

test('PL1 C2: POP liga CA/CH/AAD, rejeita DH zero e conserva originais privados imutáveis', () => {
  const c = verifier(), buffer = raw(v.packet['box-challenge']), B = c.verify('box-challenge', buffer, now)
  const original = B.wire; buffer.fill(0); assert.equal(B.wire, original)
  assert.throws(() => { (B.payload as unknown[])[2] = 'changed' })
  const CA = v.payloadHashes['app-consent'], CH = v.payloadHashes['local-consent']
  for (const [a, b] of [[CA, '0'.repeat(64)], ['0'.repeat(64), CH]]) assert.throws(() => personalBoxOpen(unb64url(v.seeds.box, 32)!, B, v.tupleHash, a, b))
  assert.throws(() => personalBoxSeal(ephemeral(unb64url(v.seeds.ephemeral, 32)!), b64url(new Uint8Array(32)), v.pop.aad, v.pop.secret))
  const p = structuredClone(v.packet['box-challenge'][0]); p[8] = b64url(new Uint8Array(32))
  const corrupt = c.verify('box-challenge', raw(seal('app', p)), now)
  assert.throws(() => personalBoxOpen(unb64url(v.seeds.box, 32)!, corrupt, v.tupleHash, CA, CH))
})

test('PL1 C2-1: W com R recusa Y null/null assinado por H', () => {
  const c = verifier(), packet = structuredClone(v.packet)
  // S guardaria hY antigo e mascararia a relação W/Y que esta regressão isola.
  delete packet.state
  assert.equal(verifyPersonalCeremony(c, raw(packet), now).hash, v.tupleHash)
  const y = packet['fence-ack'][0]
  y[3] = personalHash(packet.fence[0])
  y[4] = null; y[5] = null
  packet['fence-ack'] = seal('hub', y)
  const W = c.verify('fence', raw(packet.fence), now)
  const Y = c.verify('fence-ack', raw(packet['fence-ack']), now)
  assert.equal(Y.payload[3], W.hash)
  assert.equal(W.payload[4], packet.receipt[0][6])
  assert.equal(W.payload[5], personalHash(packet.receipt[0]))
  assert.equal(Y.payload[4], null); assert.equal(Y.payload[5], null)
  assert.throws(() => verifyPersonalCeremony(c, raw(packet), now), 'W com R deve recusar Y null/null, mesmo com assinatura H e hW corretos')
})

test('PL1 C2-1: W null/null recusa Y com R assinado por H', () => {
  const c = verifier(), packet = structuredClone(v.packet)
  delete packet.state
  assert.equal(verifyPersonalCeremony(c, raw(packet), now).hash, v.tupleHash)
  const w = packet.fence[0]
  w[4] = null; w[5] = null
  packet.fence = seal('issuer', w)
  const y = packet['fence-ack'][0]
  y[3] = personalHash(w)
  packet['fence-ack'] = seal('hub', y)
  const W = c.verify('fence', raw(packet.fence), now)
  const Y = c.verify('fence-ack', raw(packet['fence-ack']), now)
  assert.equal(Y.payload[3], W.hash)
  assert.equal(W.payload[4], null); assert.equal(W.payload[5], null)
  assert.equal(Y.payload[4], packet.receipt[0][6])
  assert.equal(Y.payload[5], personalHash(packet.receipt[0]))
  assert.throws(() => verifyPersonalCeremony(c, raw(packet), now), 'W null/null deve recusar Y com R, mesmo com assinaturas E/H e hW corretos')
})
