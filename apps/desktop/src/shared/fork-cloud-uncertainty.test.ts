import assert from 'node:assert/strict'
import test from 'node:test'
import { randomBytes } from 'node:crypto'
import { promises as fs } from 'node:fs'
import * as wire from '../main/fork-cloud/crypto.ts'
import { loadMainModule, deferred } from './test-support/main-module.ts'
import { fixture, type FixtureReply } from './test-support/fork-cloud-ceremony.ts'

async function serviceFixture() {
  const x = await fixture()
  const binding = { deviceId: 'dev_fixture', deviceKey: wire.b64url(x.signer.publicKey) }
  const account = { sessionEpoch: 1, on() {}, off() {}, isCloudSessionCurrent: () => x.principal.current(), verifiedDeviceBinding: () => binding }
  const connection = loadMainModule<typeof import('../main/fork-cloud/connection.ts')>('fork-cloud/connection', {
    'node:crypto': { randomBytes }, './account-api': { AccountApi: x.api }, './device-keys': { deviceSigner: async () => x.signer },
    './crypto': wire, './pins': x.pins, './relay-socket': { RelayLink: class { constructor() { throw new Error('Unexpected relay before pin recovery') } } },
  }, { Buffer })
  const { ForkCloudService } = loadMainModule<typeof import('../main/fork-cloud/service.ts')>('fork-cloud/service', {
    // Fronteira de efeito do Main: captura nativa do runtime pessoal (import só de efeito em service.ts).
    './personal-runtime': {},
    'node:crypto': { randomBytes }, electron: { app: {}, ipcMain: {} }, './principal': { capturePrincipal: () => x.principal },
    './account-api': { AccountApi: x.api }, './device-keys': { deviceSigner: async () => x.signer }, './crypto': wire,
    './enroll': x.mod, './connection': connection, './pins': x.pins,
  }, { Buffer, Object, process, TextEncoder, URL })
  const service = new ForkCloudService(account as never, { registerDevice: async () => ({ kind: 'registered' }), reconcileRegistration: async () => ({ kind: 'registered' }) }, { captureLocal: () => x.local })
  const waiting = async () => {
    assert.equal((await service.invoke({ kind: 'discover' })).state.kind, 'needsLink')
    assert.equal((await service.invoke({ kind: 'startRemote', computerId: x.cid })).state.kind, 'linking')
    const result = await service.invoke({ kind: 'check', attemptId: (await service.invoke({ kind: 'state' })).state.attemptId })
    assert.equal(result.state.kind, 'waitingOwnerApproval'); return result.state
  }
  return { ...x, service, waiting }
}

test('falha de leitura não cerca a conta nem perde o código da mesma tentativa A2', async () => {
  const x = await serviceFixture()
  try {
    const waiting = await x.waiting(); x.approve()
    x.replyOnce('getAccess', async () => ({ kind: 'unknown' }))
    const retry = await x.service.invoke({ kind: 'check', attemptId: waiting.attemptId })
    assert.equal(retry.recovery, null)
    assert.equal(retry.state.kind, 'waitingOwnerApproval')
    assert.equal(retry.state.code, waiting.code)
    assert.equal((await x.service.invoke({ kind: 'confirm', attemptId: waiting.attemptId, code: waiting.code! })).state.kind, 'linked')
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.apiCalls('revealAccess'), 1)
  } finally { x.service.dispose(); await x.clear() }
})

test('cancelar durante GET não deixa cerca falsa nem aceita a resposta tardia', async () => {
  const x = await serviceFixture()
  try {
    const waiting = await x.waiting(); const started = deferred(); const reply = deferred<FixtureReply>()
    x.replyOnce('getAccess', async () => { started.resolve(); return reply.promise })
    const checking = x.service.invoke({ kind: 'check', attemptId: waiting.attemptId }); await started.promise
    const canceled = await x.service.invoke({ kind: 'cancel', attemptId: waiting.attemptId })
    assert.equal(canceled.recovery, null)
    reply.resolve({ kind: 'unknown' }); await checking
    assert.equal((await x.service.invoke({ kind: 'state' })).recovery, null)
    assert.equal((await x.service.invoke({ kind: 'discover' })).state.kind, 'needsLink')
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.apiCalls('revealAccess'), 1)
    assert.equal(await x.pins.readPin(x.principal, x.cid), null)
  } finally { x.service.dispose(); await x.clear() }
})

test('hello Local comprovadamente não enviado não vira mutação incerta', async () => {
  const x = await serviceFixture()
  try {
    x.localFailure('hello', 'notSent')
    const result = await x.service.invoke({ kind: 'startLocal', profileId: 'fixture', claim: false })
    assert.equal(result.recovery, null); assert.equal(result.state.kind, 'notSent')
    assert.equal(x.apiCalls('requestAccess'), 0)
    assert.equal((await x.service.invoke({ kind: 'startLocal', profileId: 'fixture', claim: false })).state.kind, 'linking')
    assert.equal(x.apiCalls('requestAccess'), 1)
  } finally { x.service.dispose(); await x.clear() }
})


test('efeito de pedido ou revelação sem confirmação continua cercado, sem reenvio', async () => {
  for (const phase of ['requestAccess', 'revealAccess'] as const) {
    const x = await serviceFixture()
    try {
      await x.service.invoke({ kind: 'discover' })
      x.replyOnce(phase, async () => ({ kind: 'unknown' }))
      let result = await x.service.invoke({ kind: 'startRemote', computerId: x.cid })
      if (phase === 'revealAccess') result = await x.service.invoke({ kind: 'check', attemptId: result.state.attemptId })
      assert.equal(result.recovery, 'enrollment'); assert.equal(result.state.kind, 'unknown')
      const id = result.state.attemptId
      await x.service.invoke({ kind: 'check', attemptId: id })
      await x.service.invoke({ kind: 'startRemote', computerId: x.cid })
      await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
      assert.equal((await x.service.invoke({ kind: 'state' })).recovery, 'enrollment')
      assert.equal(x.apiCalls(phase), 1); assert.equal(await x.pins.readPin(x.principal, x.cid), null)
    } finally { x.service.dispose(); await x.clear() }
  }
})

test('cancelar uma revelação em voo preserva o efeito incerto e não o repete', async () => {
  const x = await serviceFixture()
  try {
    await x.service.invoke({ kind: 'discover' })
    const initial = await x.service.invoke({ kind: 'startRemote', computerId: x.cid })
    const started = deferred(); const reply = deferred<FixtureReply>()
    x.replyOnce('revealAccess', async () => { started.resolve(); return reply.promise })
    const checking = x.service.invoke({ kind: 'check', attemptId: initial.state.attemptId }); await started.promise
    assert.equal((await x.service.invoke({ kind: 'cancel', attemptId: initial.state.attemptId })).recovery, 'enrollment')
    reply.resolve({ kind: 'unknown' }); await checking
    await x.service.invoke({ kind: 'startRemote', computerId: x.cid })
    assert.equal((await x.service.invoke({ kind: 'state' })).recovery, 'enrollment')
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.apiCalls('revealAccess'), 1)
  } finally { x.service.dispose(); await x.clear() }
})

test('decisão Local distingue zero envio de aprovação possivelmente efetivada', async () => {
  for (const kind of ['notSent', 'unknown'] as const) {
    const x = await serviceFixture()
    try {
      const initial = await x.service.invoke({ kind: 'startLocal', profileId: 'fixture', claim: false })
      await x.service.invoke({ kind: 'check', attemptId: initial.state.attemptId })
      x.localFailure('decideAccessRequest', kind)
      const decided = await x.service.invoke({ kind: 'check', attemptId: initial.state.attemptId })
      assert.equal(decided.recovery, kind === 'unknown' ? 'enrollment' : null)
      assert.equal(x.decisions(), kind === 'unknown' ? 1 : 0)
      await x.service.invoke({ kind: 'check', attemptId: initial.state.attemptId })
      await x.service.invoke({ kind: 'check', attemptId: initial.state.attemptId })
      assert.equal(x.decisions(), 1)
      assert.equal((await x.service.invoke({ kind: 'state' })).state.kind, kind === 'unknown' ? 'unknown' : 'linked')
      assert.equal(x.apiCalls('requestAccess'), 1)
    } finally { x.service.dispose(); await x.clear() }
  }
})


test('reinício com pin pendente oferece a mesma gravação pelo Main, sem novo vínculo', async () => {
  const x = await serviceFixture()
  try {
    const previous = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    await previous.start(); const waiting = await previous.check(); x.approve(); x.failPublication()
    assert.equal((await previous.confirmCode(waiting.code!)).kind, 'unknown')
    const pending = await x.service.invoke({ kind: 'connect', computerId: x.cid })
    assert.equal(pending.state.kind, 'unknown'); assert.equal(pending.recoveryPhase, 'publication')
    assert.equal(pending.state.computerId, x.cid)
    const recovered = await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
    assert.equal(recovered.state.kind, 'linked'); assert.equal(recovered.recovery, null)
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.apiCalls('revealAccess'), 1)
    assert.equal(x.decisions(), 0); assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A2')
  } finally { x.service.dispose(); await x.clear() }
})


test('recusa definitiva de criar pedido não cerca a conta e libera ação explícita posterior', async () => {
  const x = await serviceFixture()
  try {
    await x.service.invoke({ kind: 'discover' })
    x.replyOnce('requestAccess', async () => ({ kind: 'refused', status: 403, code: 'denied' }))
    const refused = await x.service.invoke({ kind: 'startRemote', computerId: x.cid })
    assert.equal(refused.recovery, null); assert.equal(refused.state.kind, 'error')
    assert.equal((await x.service.invoke({ kind: 'discover' })).state.kind, 'needsLink')
    assert.equal(x.apiCalls('requestAccess'), 1)
  } finally { x.service.dispose(); await x.clear() }
})


test('claim confirmado espera atualização do Hub e só reconcilia por leituras explícitas', async () => {
  const x = await serviceFixture()
  try {
    x.setOwner(null)
    const completed = await x.service.invoke({ kind: 'startLocal', profileId: 'fixture', claim: true })
    assert.equal(completed.recoveryPhase, 'claim'); assert.equal(completed.state.kind, 'unknown')
    assert.equal(x.apiCalls('createClaim'), 1); assert.equal(x.apiCalls('completeClaim'), 1)
    const stillWaiting = await x.service.invoke({ kind: 'reconcileClaim' })
    assert.equal(stillWaiting.state.kind, 'unknown')
    x.setOwner('another-fixture-account')
    assert.equal((await x.service.invoke({ kind: 'reconcileClaim' })).recovery, 'enrollment')
    x.setOwner(x.principal.seal.userId)
    const confirmed = await x.service.invoke({ kind: 'reconcileClaim' })
    assert.equal(confirmed.state.kind, 'needsLink'); assert.equal(confirmed.recovery, null)
    assert.equal(x.apiCalls('createClaim'), 1); assert.equal(x.apiCalls('completeClaim'), 1)
    assert.equal(x.apiCalls('requestAccess'), 0)
    assert.equal((await x.service.invoke({ kind: 'startLocal', profileId: 'fixture', claim: false })).state.kind, 'linking')
    assert.equal(x.apiCalls('requestAccess'), 1)
  } finally { x.service.dispose(); await x.clear() }
})


for (const route of ['reader', 'enrollment'] as const) test(`F7: ${route} retém a gravação exata após interromper depois do unlink`, async () => {
  const x = await serviceFixture()
  try {
    if (route === 'reader') {
      const previous = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
      await previous.start(); const waiting = await previous.check(); x.approve(); x.failPublication()
      assert.equal((await previous.confirmCode(waiting.code!)).kind, 'unknown')
      x.afterPublicationUnlink(async () => { await x.service.invoke({ kind: 'disconnect' }) })
      await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
    } else {
      const waiting = await x.waiting(); x.approve()
      x.afterPublicationUnlink(async () => { await x.service.invoke({ kind: 'disconnect' }) })
      await x.service.invoke({ kind: 'confirm', attemptId: waiting.attemptId, code: waiting.code! })
    }
    const path = x.pins.pinPath(x.principal, x.cid), bytes = await fs.readFile(path)
    await assert.rejects(fs.lstat(path + '.pending'), { code: 'ENOENT' })
    assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A2')
    assert.equal((await x.service.invoke({ kind: 'state' })).recoveryPhase, 'publication')
    // A different encrypted publication must not clear the fence, even if its receipt is valid.
    await fs.writeFile(path, Buffer.concat([Buffer.from('\n'), bytes]))
    assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A2')
    const different = await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
    assert.equal(different.recovery, 'enrollment'); assert.notEqual(different.state.kind, 'linked')
    await fs.writeFile(path, bytes)
    const recovered = await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
    assert.equal(recovered.state.kind, 'linked'); assert.equal(recovered.recovery, null)
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.apiCalls('revealAccess'), 1); assert.equal(x.decisions(), 0)
    assert.deepEqual(await fs.readFile(path), bytes)
  } finally { x.service.dispose(); await x.clear() }
})


test('cerca sem identidade capturada não aceita qualquer pin válido após sumir pending', async () => {
  const x = await serviceFixture()
  try {
    const previous = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    await previous.start(); const waiting = await previous.check(); x.approve(); x.failPublication()
    await previous.confirmCode(waiting.code!)
    assert.equal((await x.service.invoke({ kind: 'connect', computerId: x.cid })).recoveryPhase, 'publication')
    await fs.unlink(x.pins.pinPath(x.principal, x.cid) + '.pending')
    assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A2')
    const refused = await x.service.invoke({ kind: 'reconcilePin', computerId: x.cid })
    assert.equal(refused.recovery, 'enrollment'); assert.notEqual(refused.state.kind, 'linked')
    assert.equal((await x.service.invoke({ kind: 'connect', computerId: x.cid })).state.kind, 'notSent')
    assert.equal(x.apiCalls('requestAccess'), 1); assert.equal(x.decisions(), 0)
  } finally { x.service.dispose(); await x.clear() }
})
