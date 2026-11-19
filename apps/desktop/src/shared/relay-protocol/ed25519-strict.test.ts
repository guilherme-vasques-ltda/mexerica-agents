import test from 'node:test'
import assert from 'node:assert/strict'
import { ed25519 } from '@noble/curves/ed25519.js'
import { bytesToNumberLE, numberToBytesLE } from '@noble/curves/utils.js'
import { sha512 } from '@noble/hashes/sha2.js'
import { nodeBackend } from './backend-node.ts'
import { nobleBackend } from './backend-noble.ts'
import type { CryptoBackend } from './backend.ts'
import { createProtocol } from './protocol.ts'

// Verificação Ed25519 estrita: o protocolo dá o MESMO veredito ESTRITO sobre o `node:crypto` e sobre
// o @noble, em cada caso hostil, sem depender da versão do OpenSSL/BoringSSL. A tabela roda contra os
// dois backends; cada linha afirma o veredito esperado e a igualdade node × noble.

const P = ed25519.Point
const L = P.Fn.ORDER
const p = P.Fp.ORDER
const cat = (...parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, x) => n + x.length, 0))
  let at = 0
  for (const x of parts) {
    out.set(x, at)
    at += x.length
  }
  return out
}
const enc = (y: bigint, sign = 0) => {
  const b = numberToBytesLE(y, 32)
  b[31]! |= sign << 7
  return b
}
const hex = (b: Uint8Array) => Buffer.from(b).toString('hex')
const challenge = (r: Uint8Array, a: Uint8Array, m: Uint8Array) => bytesToNumberLE(sha512(cat(r, a, m))) % L

// Os 8 pontos do subgrupo de torção (ordens 1, 2, 4, 4, 8, 8, 8, 8).
const Y8 = 0x7a03ac9277fdc74ec6cc392cfa53202a0f67100d760b3cba4fd84d3d706a17c7n
const lowOrder = (() => {
  const found = new Map<string, InstanceType<typeof P>>()
  for (const y of [0n, 1n, p - 1n, Y8, p - Y8]) {
    for (const sign of [0, 1]) {
      try {
        const pt = P.fromBytes(enc(y, sign), false)
        if (pt.isSmallOrder()) found.set(hex(pt.toBytes()), pt)
      } catch {
        // y sem x na curva
      }
    }
  }
  return [...found.values()]
})()
const T8 = lowOrder.find((t) => !t.double().double().is0())! // ordem 8

// Chave honesta fixa (a escalar `a` do RFC 8032 derivado da semente).
const seed = new Uint8Array(32).fill(7)
const h = sha512(seed).slice(0, 32)
h[0]! &= 248
h[31]! &= 127
h[31]! |= 64
const a = bytesToNumberLE(h) % L
const pub = ed25519.getPublicKey(seed)
const r0 = 12345n
const R0 = P.BASE.multiplyUnsafe(r0)

const msgN = (i: number) => new Uint8Array([i])

/** Assinatura "caseira": `R` dado, `S = r + k·a` (a equação sem cofator, com `a` conhecido). */
const craft = (rPoint: Uint8Array, r: bigint, m: Uint8Array, key: Uint8Array = pub) => cat(rPoint, numberToBytesLE((r + challenge(rPoint, key, m) * a) % L, 32))

/** Procura uma mensagem cujo desafio `k` satisfaz `ok` (ex.: `k` múltiplo de 8 some a torção). */
const grindMsg = (rBytes: Uint8Array, key: Uint8Array, ok: (k: bigint) => boolean) => {
  for (let i = 0; i < 65536; i++) {
    const m = new Uint8Array([i & 255, i >> 8])
    if (ok(challenge(rBytes, key, m))) return m
  }
  throw new Error('sem mensagem')
}

interface Case {
  name: string
  pub: Uint8Array
  sig: Uint8Array
  msg: Uint8Array
  expected: boolean
}
const cases: Case[] = []
const add = (name: string, pubKey: Uint8Array, sig: Uint8Array, msg: Uint8Array, expected: boolean) => cases.push({ name, pub: pubKey, sig, msg, expected })

// ---- positivos ----
{
  const m = msgN(1)
  const sig = ed25519.sign(m, seed)
  add('assinatura honesta', pub, sig, m, true)
  add('assinatura caseira honesta (R=[r]B)', pub, craft(R0.toBytes(), r0, m), m, true)
  add('mensagem trocada', pub, sig, msgN(2), false)
  const flip = sig.slice()
  flip[40]! ^= 1
  add('S adulterado', pub, flip, m, false)
  const flipR = sig.slice()
  flipR[3]! ^= 1
  add('R adulterado', pub, flipR, m, false)
}

// ---- chave pública de ordem baixa (8 pontos) ----
assert.equal(lowOrder.length, 8)
for (const t of lowOrder) {
  const tb = t.toBytes()
  const label = hex(tb).slice(0, 8)
  add(`A ordem baixa ${label}: R=0…0, S=0`, tb, new Uint8Array(64), msgN(1), false)
  // R = identidade, S = 0 e mensagem com k·A = 0: a equação pura (sem cofator) fecharia — o
  // política estrita recusa mesmo assim, por ser chave de ordem baixa.
  const idR = P.ZERO.toBytes()
  const m = grindMsg(idR, tb, (k) => k % 8n === 0n || t.multiplyUnsafe(k % 8n).is0())
  add(`A ordem baixa ${label}: R=identidade, S=0, k·A=0`, tb, cat(idR, new Uint8Array(32)), m, false)
}
// encodings não canônicos dos pontos de ordem baixa como chave (y+p; bit de sinal com x=0)
for (const [name, bytes] of [
  ['A y=p (não canônico de y=0)', enc(p)],
  ['A y=p+1 (não canônico da identidade)', enc(p + 1n)],
  ['A y=p-1 com sinal 1 (x=0)', enc(p - 1n, 1)],
  ['A y=1 com sinal 1 (x=0)', enc(1n, 1)],
  ['A y=2^255-1 (>= p)', enc((1n << 255n) - 1n)],
] as const) {
  add(`${name}: R=0…0, S=0`, bytes, new Uint8Array(64), msgN(1), false)
}

// ---- A com torção que NÃO é de ordem baixa: aceito pelo OpenSSL se a equação fecha ----
{
  const Apt = P.fromBytes(pub, false).add(T8)
  const apb = Apt.toBytes()
  const rb = R0.toBytes()
  const m = grindMsg(rb, apb, (k) => k % 8n === 0n)
  add('A com torção (não é de ordem baixa), k%8=0: espelha o OpenSSL', apb, craft(rb, r0, m, apb), m, true)
}

// ---- R com componente de torção, chave honesta ----
for (const t of [T8, T8.double(), T8.double().double()]) {
  const rt = R0.add(t).toBytes()
  const m = msgN(3)
  add('R com componente de torção', pub, craft(rt, r0, m), m, false)
}

// ---- R de ordem baixa (identidade) e encodings não canônicos de R ----
for (const [name, rb] of [
  ['R identidade canônica', enc(1n)],
  ['R identidade não canônica (y=p+1)', enc(p + 1n)],
  ['R identidade com sinal 1', enc(1n, 1)],
  ['R y=2^255-1', enc((1n << 255n) - 1n)],
] as const) {
  const m = msgN(4)
  add(name, pub, cat(rb, numberToBytesLE((challenge(rb, pub, m) * a) % L, 32)), m, false)
}

// ---- S não canônico ----
{
  const m = msgN(5)
  const rb = R0.toBytes()
  const good = craft(rb, r0, m)
  const s = bytesToNumberLE(good.subarray(32))
  add('S canônico', pub, good, m, true)
  add('S + L', pub, cat(rb, numberToBytesLE(s + L, 32)), m, false)
  add('S = L', pub, cat(rb, numberToBytesLE(L, 32)), m, false)
  add('S = 2^256-1', pub, cat(rb, new Uint8Array(32).fill(255)), m, false)
}

// ---- tamanhos ----
add('chave curta', pub.subarray(0, 31), ed25519.sign(msgN(1), seed), msgN(1), false)
add('assinatura curta', pub, ed25519.sign(msgN(1), seed).subarray(0, 63), msgN(1), false)

const backends: [string, CryptoBackend][] = [
  ['node:crypto', nodeBackend],
  ['@noble/*', nobleBackend],
]

// A política estrita vive na camada do protocolo (protocol.ts → ed25519-strict.ts), ANTES do
// backend: o veredito é o ESTRITO, igual nos dois backends e em qualquer OpenSSL/BoringSSL
// (o OpenSSL 3.0 do Node 22 aceita chave de ordem baixa; o 3.5 do Node 24 recusa).
const verifiers: [string, (pub: Uint8Array, sig: Uint8Array, msg: Uint8Array) => boolean][] = [
  ...backends.map(([name, backend]): [string, (pub: Uint8Array, sig: Uint8Array, msg: Uint8Array) => boolean] => [`protocolo sobre ${name}`, createProtocol(backend).verifyEd25519]),
  ['backend @noble/* isolado', nobleBackend.ed25519.verify],
]

for (const [name, verify] of verifiers) {
  test(`Ed25519 estrito: tabela de casos — ${name}`, () => {
    for (const c of cases) assert.equal(verify(c.pub, c.sig, c.msg), c.expected, c.name)
  })
}

test('Ed25519 estrito: node e noble dão o mesmo veredito em cada caso (pelo protocolo)', () => {
  const node = createProtocol(nodeBackend).verifyEd25519
  const noble = createProtocol(nobleBackend).verifyEd25519
  for (const c of cases) assert.equal(noble(c.pub, c.sig, c.msg), node(c.pub, c.sig, c.msg), c.name)
  assert.ok(cases.length >= 40)
})

test('Ed25519 estrito: mutações aleatórias de assinaturas válidas dão o mesmo veredito', () => {
  const node = createProtocol(nodeBackend)
  const noble = createProtocol(nobleBackend)
  for (let i = 0; i < 300; i++) {
    const signer = node.signerFromSeed(node.newDeviceSeed())
    const m = node.random(1 + (i % 40))
    const sig = signer.sign(m)
    assert.equal(noble.verifyEd25519(signer.publicKey, sig, m), true)
    assert.equal(node.verifyEd25519(signer.publicKey, sig, m), true)
    const bad = sig.slice()
    bad[i % 64]! ^= 1 << (i % 8)
    assert.equal(noble.verifyEd25519(signer.publicKey, bad, m), node.verifyEd25519(signer.publicKey, bad, m))
  }
})

for (const [name, backend] of backends) {
  test(`Ed25519/X25519: ${name} não depende dos buffers privados do chamador`, () => {
    const seedBuf = new Uint8Array(32).fill(5)
    const signer = backend.ed25519.signerFromSeed(seedBuf)
    const before = signer.sign(msgN(1))
    seedBuf.fill(0)
    assert.deepEqual(signer.sign(msgN(1)), before)
    assert.equal(backend.ed25519.verify(signer.publicKey, signer.sign(msgN(1)), msgN(1)), true)

    const privBuf = new Uint8Array(32).fill(9)
    const eph = backend.x25519.ephemeral(privBuf)
    const peer = backend.x25519.ephemeral(new Uint8Array(32).fill(3)).pub
    const dh = eph.dh(peer)
    privBuf.fill(0)
    assert.deepEqual(eph.dh(peer), dh)
  })
}
