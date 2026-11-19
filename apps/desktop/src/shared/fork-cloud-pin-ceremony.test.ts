import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as wire from '../main/fork-cloud/crypto.ts'
import { fixture } from './test-support/fork-cloud-ceremony.ts'

test('A2 só publica após código explícito e aprovação da mesma tentativa; pin sobrevive ao restart', async () => {
  const x = await fixture()
  try {
    const attempt = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    assert.equal((await attempt.start()).kind, 'linking')
    const waiting = await attempt.check()
    assert.equal(waiting.kind, 'waitingOwnerApproval')
    x.approve()
    assert.equal((await attempt.check()).kind, 'waitingOwnerApproval')
    assert.equal(await x.pins.readPin(x.principal, x.cid), null)
    assert.equal((await attempt.confirmCode(waiting.code!)).kind, 'linked')
    const restoredPrincipal = { ...x.principal, seal: { ...x.principal.seal, epoch: 2 } }
    assert.equal((await x.pins.readPin(restoredPrincipal, x.cid))?.signKey, wire.b64url(x.host))
    assert.equal(x.writes(), 1)
    const bytes = await fs.promises.readFile(x.pins.pinPath(x.principal, x.cid))
    const altered = JSON.parse(bytes.toString()); altered.receipt.source = 'A1'; altered.receipt.localTranscript = wire.b64url(new Uint8Array(32))
    await fs.promises.writeFile(x.pins.pinPath(x.principal, x.cid), JSON.stringify(altered))
    await assert.rejects(x.pins.readPin(restoredPrincipal, x.cid), /provenance/)
  } finally { await x.clear() }
})

test('A1 confere próprio SAS/transcript e não repete a decisão ao reconciliar o grant', async () => {
  const x = await fixture()
  try {
    const attempt = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host), x.local)
    await attempt.start(); await attempt.check()
    assert.equal((await attempt.check()).kind, 'linking')
    assert.equal(x.decisions(), 1)
    assert.equal((await attempt.check()).kind, 'linked')
    assert.equal((await attempt.check()).kind, 'linked')
    assert.equal(x.decisions(), 1)
    assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A1')
  } finally { await x.clear() }
})

test('código errado e época invalidada não publicam nem reabrem a tentativa', async () => {
  const x = await fixture()
  try {
    const attempt = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    await attempt.start(); const waiting = await attempt.check(); x.approve()
    const wrong = waiting.code === '000000' ? '000001' : '000000'
    assert.equal((await attempt.confirmCode(wrong)).kind, 'hubUnverified')
    assert.equal((await attempt.confirmCode(waiting.code!)).kind, 'hubUnverified')
    assert.equal(await x.pins.readPin(x.principal, x.cid), null)
    x.stop()
    assert.equal((await attempt.check()).kind, 'error')
    assert.deepEqual(await fs.promises.readdir(x.dir), [])
  } finally { await x.clear() }
})


test('publicação incerta fica cercada e check reconcilia a mesma intenção sem novo pedido', async () => {
  const x = await fixture()
  try {
    const attempt = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    await attempt.start(); const waiting = await attempt.check(); x.approve(); x.failPublication()
    assert.equal((await attempt.confirmCode(waiting.code!)).kind, 'unknown')
    await assert.rejects(x.pins.readPin(x.principal, x.cid), /unconfirmed/)
    assert.equal((await attempt.check()).kind, 'linked')
    assert.equal((await x.pins.readPin(x.principal, x.cid))?.source, 'A2')
    assert.equal(x.writes(), 1)
  } finally { await x.clear() }
})


test('restart exige reconciliação explícita do mesmo recibo e não promove pin pendente', async () => {
  const x = await fixture()
  try {
    const attempt = new x.mod.Enrollment(x.principal, x.cid, wire.b64url(x.host))
    await attempt.start(); const waiting = await attempt.check(); x.approve(); x.failPublication()
    assert.equal((await attempt.confirmCode(waiting.code!)).kind, 'unknown')
    const restarted = { ...x.principal, seal: { ...x.principal.seal, epoch: 2 } }
    await assert.rejects(x.pins.readPin(restarted, x.cid), /unconfirmed/)
    assert.equal((await x.pins.reconcilePin(restarted, x.cid))?.source, 'A2')
    assert.equal((await x.pins.readPin(restarted, x.cid))?.signKey, wire.b64url(x.host))
    assert.equal(x.writes(), 1)
  } finally { await x.clear() }
})
