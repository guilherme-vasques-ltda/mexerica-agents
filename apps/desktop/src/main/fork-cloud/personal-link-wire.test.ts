import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { b64url, newDeviceSeed, signerFromSeed } from './crypto.ts'
import * as w from './personal-link-wire.ts'

// D70 PR-A (lado T): bytes/arrays exatos, trocas recusadas e chainDigestT sem J duplo.
const H = signerFromSeed(newDeviceSeed()), T = signerFromSeed(newDeviceSeed()), outro = signerFromSeed(newDeviceSeed())
const k = (s: { publicKey: Uint8Array }) => b64url(s.publicKey)
const id = (n: number) => b64url(new Uint8Array(n).fill(7))
const C = 'c'.repeat(64), R = id(32), nT = id(32)
const challenge = (over: Partial<Record<number, w.J>> = {}) => {
  const p: w.J[] = [w.D430.challenge, 1, k(H), id(32), C, R, id(16), k(T), nT, id(32), [w.D430.deadline, 1, 'c5386ca1-bad0-4cfb-b19b-88badf199264', R, 5, 30000], id(16), 1, w.P0]
  for (const [i, v] of Object.entries(over)) if (v !== undefined) p[Number(i)] = v
  return w.signT(H, p)
}

test('digest da política é o dos bytes exatos do Host', () => {
  const bytes = readFileSync(new URL('../../../../../host/src/fork/personal_link/policy-v1.json', import.meta.url))
  assert.equal(createHash('sha256').update(bytes).digest('hex'), w.POLICY_DIGEST)
})

test('J recusa objeto, null, float, negativo, espaço e literal fora do alfabeto', () => {
  for (const raw of ['{"a":1}', '[null]', '[1.5]', '[-1]', '[ 1]', '["a b"]', '[9007199254740992]', '"x"']) assert.throws(() => w.readJ(Buffer.from(raw)))
  assert.deepEqual(w.readJ(Buffer.from('["a",1,true,[]]')), ['a', 1, true, []])
})

test('desafio com H/T/C/N_T/P ou prazo trocados recusa antes do gesto', () => {
  const req = { T: k(T), C, nT }
  assert.ok(w.checkChallenge(challenge() as w.J, req))
  for (const over of [{ 7: k(outro) }, { 4: 'd'.repeat(64) }, { 8: id(32).replace('H', 'I') }, { 13: w.P1 }, { 10: [w.D430.deadline, 1, 'x', R, 5, 300000] }, { 2: k(T) }]) {
    assert.throws(() => w.checkChallenge(challenge(over) as w.J, req))
  }
  const forjado: w.J = [challenge()[0], b64url(outro.sign(w.j(challenge()[0])))]
  assert.throws(() => w.checkChallenge(forjado, req))
})

test('aceite, RA, PC, RC, gestão, MR e chainDigestT seguem os arrays do v2', () => {
  const ch = w.checkChallenge(challenge() as w.J, { T: k(T), C, nT }), p = ch[0]
  const ac = w.accept(T, ch)
  assert.deepEqual(ac[0], [w.D430.accept, 1, ...p.slice(2, 10), w.hashJ(ch), w.P0])
  const admission = id(16)
  const ra = w.signT(H, [w.D430.admission, 1, ...p.slice(2, 8), p[11], p[12], admission, w.hashJ(ch), w.hashJ(ac), w.P0])
  assert.ok(w.checkAdmission(ra as w.J, ch, ac))
  assert.throws(() => w.checkAdmission(w.signT(H, [...ra[0].slice(0, 13), w.P1]) as w.J, ch, ac))
  const R2 = id(32).replace('H', 'J'), resposta: w.J = [id(32), [w.D430.deadline, 1, 'b', R2, 9, 30000], R2]
  const pc = w.persisted(T, ra, resposta)
  assert.deepEqual(pc[0], [w.D430.persisted, 1, p[2], p[3], p[4], R2, p[6], p[7], admission, w.hashJ(ra), id(32), (resposta as w.J[])[1]])
  const rc = w.signT(H, [w.D430.complete, 1, p[2], p[3], p[4], R2, admission, w.hashJ(ra), w.hashJ(pc), w.P0])
  assert.ok(w.checkComplete(rc as w.J, ra, pc))
  const payload: w.J[] = [w.D430.management, 1, p[2], p[3], p[4], R2, id(16), p[7], w.hashJ(ra), 1, w.P1, id(32), [w.D430.deadline, 1, 'b', R2, 9, 30000], p[12]]
  assert.ok(w.checkManagement(payload, ra))
  assert.throws(() => w.checkManagement([...payload.slice(0, 10), ['donna.personal-link/core', 1, w.POLICY_DIGEST, ['devices.delegate', 'devices.manage', 'link.connect'], true, 2], ...payload.slice(11)], ra))
  const m = w.signT(T, payload)
  const mr = w.signT(H, [w.D430.receipt, 1, p[2], p[3], p[4], R2, payload[6], p[7], p[11], w.hashJ(ra), w.hashJ(m), w.P1])
  assert.ok(w.checkReceipt(mr as w.J, ra, m))
  const bytes = Buffer.from(JSON.stringify([ra, rc, m, mr]))
  assert.equal(w.chainDigestT(ra, rc, m, mr), createHash('sha256').update(bytes).digest('hex'))
  assert.notEqual(w.chainDigestT(ra, rc, m, mr), createHash('sha256').update(JSON.stringify(bytes.toString())).digest('hex'))
})

test('impressão de conferência tem 128 bits em 8 grupos de 4 hex', () => {
  const f = w.fingerprint(k(H))
  assert.match(f, /^[a-f0-9]{4}( [a-f0-9]{4}){7}$/)
  assert.equal(f.replaceAll(' ', ''), createHash('sha256').update(H.publicKey).digest('hex').slice(0, 32))
})
