import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as wire from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

const codec = loadMainModule<typeof import('./bootstrap-codec.ts')>('fork-cloud/bootstrap-codec', { 'node:crypto': crypto, './crypto': wire }, { Buffer })
const sum = (v: unknown) => crypto.createHash('sha256').update(JSON.stringify(v)).digest('hex')
const encode = (value: unknown[], signer: wire.DeviceSigner) => [value, wire.b64url(signer.sign(Buffer.from(JSON.stringify(value))))] as const

test('cadeia original exige raiz, desafio, prova e recibo da mesma geração e aparelho', () => {
  const root = wire.signerFromSeed(new Uint8Array(32).fill(1)), hub = wire.signerFromSeed(new Uint8Array(32).fill(2)), device = wire.signerFromSeed(new Uint8Array(32).fill(3))
  const key = wire.b64url(device.publicKey), host = wire.b64url(hub.publicKey), nonce = wire.b64url(new Uint8Array(32).fill(4)), generation = wire.b64url(new Uint8Array(32).fill(5)), box = wire.b64url(new Uint8Array(32).fill(6))
  const e = { bootstrapId: 'boot_1', provisioningIntentId: 'resource_1', resourceDigest: 'a'.repeat(64), hubGeneration: generation, issuerKeyId: 'root_1', issuer: 'https://issuer.example', deploymentId: 'deploy_1', env: 'main' as const, manifestHash: 'b'.repeat(64), ownerUserId: 'user_1', deviceId: 'dev_1', deviceKey: key, nonceApp: nonce, computerId: wire.computerIdOf(hub.publicKey), signKey: host, boxKey: box, createdAt: 1000, expiresAt: 200000 }
  const g = ['donna/bootstrap-grant/v1', e.bootstrapId, e.provisioningIntentId, e.resourceDigest, generation, e.issuerKeyId, e.issuer, e.deploymentId, e.env, e.manifestHash, e.ownerUserId, e.deviceId, key, nonce, e.computerId, host, box, nonce, 2000, 100000, ['control', 'owner.devices']]
  const c = ['donna/bootstrap-challenge/v1', e.bootstrapId, generation, sum(g), key, nonce, nonce, 3000, 60000]
  const p = ['donna/bootstrap-proof/v1', e.bootstrapId, generation, sum(g), sum(c)]
  const grantId = `grt_${wire.b64url(new Uint8Array(16).fill(7))}`
  const pending = ['donna/bootstrap-pending/v1', grantId, e.bootstrapId, generation, e.ownerUserId, e.deviceId, key, ['control', 'owner.devices'], sum(g), sum(c), sum(p), 4000, 1, 'pendingPublication']
  const r = ['donna/bootstrap-receipt/v1', e.bootstrapId, generation, sum(g), sum(c), sum(p), e.ownerUserId, e.deviceId, key, e.computerId, host, grantId, ['control', 'owner.devices'], 4000, 1, sum(pending), 0, 'c'.repeat(64)]
  const originals = { grant: encode(g, root), challenge: encode(c, hub), proof: encode(p, device), receipt: encode(r, hub) }
  assert.equal(codec.verifyBootstrapChain(originals, e, wire.b64url(root.publicKey), 300000).grantId, grantId, 'prazo de grant é verificado no commit, não no restart')
  assert.throws(() => codec.verifyBootstrapChain(originals, e, host, 5000))
  assert.throws(() => codec.verifyBootstrapChain(originals, { ...e, deviceId: 'dev_other' }, wire.b64url(root.publicKey), 5000))
  assert.throws(() => codec.verifyBootstrapGrant(originals.grant, e, wire.b64url(root.publicKey), 100000))
  assert.throws(() => codec.verifyBootstrapChain({ ...originals, proof: encode([...p.slice(0, 4), 'd'.repeat(64)], device) }, e, wire.b64url(root.publicKey), 5000))
})
