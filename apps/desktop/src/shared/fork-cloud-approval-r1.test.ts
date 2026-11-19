import assert from 'node:assert/strict'
import test from 'node:test'
import { decideCapturedAccess } from '../renderer/lib/fork-cloud/approval.ts'
import type { AccessRequest } from './models.ts'

const fixture = (): AccessRequest => ({ requestId: 'req_fixture', deviceKey: 'A'.repeat(43), code: '123456',
  transcript: 'E'.repeat(43), deviceName: 'Fixture', deadline: Date.now() + 60_000, expiresAt: Date.now() + 60_000 })
const request = () => { const r = fixture(); r.expiresAt = r.deadline; return r }

test('R1 captura o pedido antes de ready e exige recibo da mesma chave', async () => {
  const r = request(), seen = { ...r }
  let release!: () => void
  const waiting = new Promise<void>(resolve => { release = resolve })
  const calls: { method: string; body: unknown }[] = []
  const pending = decideCapturedAccess({ ready: async () => {
    await waiting
    return { call: async (method, body) => { calls.push({ method, body }); return { result: 'approved', deviceKey: seen.deviceKey, grantId: 'grt_fixture' } } }
  } }, r, true)
  r.requestId = 'req_other'; r.deviceKey = 'I'.repeat(43); r.code = '654321'; r.transcript = 'M'.repeat(43)
  release(); await pending
  assert.deepEqual(calls, [{ method: 'decideAccessRequest', body: { requestId: seen.requestId, approve: true,
    expect: { deviceKey: seen.deviceKey, code: seen.code, transcript: seen.transcript } } }])
})

test('R1 nunca transforma incerteza, recusa ou identidade divergente em sucesso/replay', async () => {
  for (const answer of [{ result: 'unknown', reason: 'cloudUnreachable' }, { result: 'refused', reason: 'consuming' },
    { result: 'approved', deviceKey: 'I'.repeat(43), grantId: 'grt_other' }, { result: 'denied' }]) {
    let count = 0
    await assert.rejects(decideCapturedAccess({ ready: async () => ({ call: async () => { count++; return answer } }) }, request(), true))
    assert.equal(count, 1)
  }
  let calls = 0
  await assert.rejects(decideCapturedAccess({ ready: async () => ({ call: async () => { calls++; throw new Error('lost response') } }) }, request(), true), /não confirmado/)
  assert.equal(calls, 1)
  let ready = 0
  await assert.rejects(decideCapturedAccess({ ready: async () => { ready++; throw new Error('must not prepare') } },
    { ...request(), transcript: undefined }, true))
  assert.equal(ready, 0)
})

const aprovar = (extra: Record<string, unknown>) => decideCapturedAccess({ ready: async () => ({ call: async () => ({
  result: 'approved', deviceKey: 'A'.repeat(43), grantId: 'grt_fixture', ...extra }) }) }, request(), true)

test('MX-149: confirmCode de 6 dígitos volta ao chamador; ausente (Hub antigo) aprova sem código', async () => {
  assert.deepEqual(await aprovar({ confirmCode: '244257' }), { confirmCode: '244257' })
  assert.deepEqual(await aprovar({}), {})
})

test('MX-149: confirmCode inválido deixa o resultado como não confirmado e nunca sai para a tela', async () => {
  for (const bad of [null, '24425', '2442577', '24425a', ' 244257', '244257\n', '٢٤٤٢٥٧', 244257, '', {}, ['244257']]) {
    await assert.rejects(aprovar({ confirmCode: bad }), /não confirmado/, `recusa ${JSON.stringify(bad)}`)
  }
})

test('MX-149: recusa, expiração e negação nunca carregam código', async () => {
  await assert.rejects(decideCapturedAccess({ ready: async () => ({ call: async () => ({ result: 'refused', reason: 'transcriptMismatch', confirmCode: '244257' }) }) }, request(), true), /recusado/)
  assert.deepEqual(await decideCapturedAccess({ ready: async () => ({ call: async () => ({ result: 'denied', confirmCode: '244257' }) }) }, request(), false), {})
})
