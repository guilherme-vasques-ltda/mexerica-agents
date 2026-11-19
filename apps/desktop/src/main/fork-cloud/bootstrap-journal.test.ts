import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import { mkdtempSync, rmSync } from 'node:fs'
import * as wire from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

test('journal conserva uma intenção e fase incerta sem criar substituto', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'bootstrap-journal-'))
  try {
    const codec = loadMainModule<typeof import('./bootstrap-journal.ts')>('fork-cloud/bootstrap-journal', {
      'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
      electron: { app: { getPath: () => dir }, safeStorage: { encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
      './crypto': wire, './pins': { readPinBytes: (file: string) => fs.promises.readFile(file), securePinsAvailable: () => true },
    }, { Buffer, process })
    let live = true
    const deployment = { deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.example', origin: 'https://cloud.example', manifestHash: 'a'.repeat(64) }
    const p = { namespace: 'fixture', deployment, seal: { userId: 'user_1', sessionId: 'sess_1', epoch: 1 }, current: () => live, token: async () => 'fixture' }
    const binding = { version: 1 as const, userId: 'user_1', sessionId: 'sess_1', origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_1', deviceKey: wire.b64url(new Uint8Array(32)) }
    const first = await codec.createBootstrapJournal(p, binding)
    const uncertain = await codec.advanceBootstrapJournal(p, binding, first, 'createUnknown')
    // Upgrade compatibility: preserve the exact pre-device filename for its old key.
    const bound = fs.readdirSync(dir).find(n => n.startsWith('bootstrap-journal-device-'))
    const legacy = path.join(dir, `bootstrap-journal-${crypto.createHash('sha256').update(p.namespace).digest('hex')}.bin`)
    if (bound) fs.renameSync(path.join(dir,bound),legacy)
    const oldBytes = fs.readFileSync(legacy)
    const restarted = await codec.readBootstrapJournal(p, binding)
    assert.equal(restarted?.bootstrapId, first.bootstrapId); assert.equal(restarted?.nonceApp, first.nonceApp); assert.equal(restarted?.phase, 'createUnknown')
    await assert.rejects(codec.createBootstrapJournal(p, binding))
    await assert.rejects(codec.advanceBootstrapJournal(p, binding, first, 'createUnknown'))
    const replacement = { ...binding,deviceId:'dev_2',deviceKey:wire.b64url(new Uint8Array(32).fill(2)) }
    assert.equal(await codec.readBootstrapJournal(p,replacement),null)
    const newDevice = await codec.createBootstrapJournal(p,replacement)
    assert.notEqual(newDevice.bootstrapId,first.bootstrapId)
    assert.equal((await codec.readBootstrapJournal(p,binding))?.phase,'createUnknown')
    assert.equal((await codec.readBootstrapJournal(p,replacement))?.bootstrapId,newDevice.bootstrapId)
    assert.deepEqual(fs.readFileSync(legacy),oldBytes,'retired identity history is never overwritten')
    const current = fs.readdirSync(dir).find(n => n.startsWith('bootstrap-journal-device-'))!
    fs.writeFileSync(path.join(dir,current),Buffer.from(JSON.stringify({...newDevice,deviceKey:binding.deviceKey})))
    await assert.rejects(codec.readBootstrapJournal(p,replacement),'a corrupt current-key journal still fails closed')
    live = false
    await assert.rejects(codec.advanceBootstrapJournal(p, binding, uncertain, 'requested'))
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
