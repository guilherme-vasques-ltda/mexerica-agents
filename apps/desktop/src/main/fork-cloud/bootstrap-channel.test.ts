import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import { performance } from 'node:perf_hooks'
import * as wire from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

test('bootstrap usa propósito, mesma chave, persiste antes da única prova e preserva unknown', async () => {
  const root = wire.signerFromSeed(new Uint8Array(32).fill(1)), hub = wire.signerFromSeed(new Uint8Array(32).fill(2)), device = wire.signerFromSeed(new Uint8Array(32).fill(3))
  const key = wire.b64url(device.publicKey), host = wire.b64url(hub.publicKey), nonce = wire.b64url(new Uint8Array(32).fill(4)), generation = wire.b64url(new Uint8Array(32).fill(5)), box = wire.b64url(new Uint8Array(32).fill(6)), now = Date.now()
  const e = { bootstrapId: 'boot_1', provisioningIntentId: 'resource_1', resourceDigest: 'a'.repeat(64), hubGeneration: generation, issuerKeyId: 'root_1', issuer: 'https://issuer.example', deploymentId: 'deploy_1', env: 'main' as const, manifestHash: 'b'.repeat(64), ownerUserId: 'user_1', deviceId: 'dev_1', deviceKey: key, nonceApp: nonce, computerId: wire.computerIdOf(hub.publicKey), signKey: host, boxKey: box, createdAt: now - 5000, expiresAt: now + 120_000 }
  const digest = (v: unknown) => crypto.createHash('sha256').update(JSON.stringify(v)).digest('hex')
  const sign = (v: unknown[], signer: wire.DeviceSigner) => [v, wire.b64url(signer.sign(Buffer.from(JSON.stringify(v))))] as const
  const g = ['donna/bootstrap-grant/v1', e.bootstrapId, e.provisioningIntentId, e.resourceDigest, generation, e.issuerKeyId, e.issuer, e.deploymentId, e.env, e.manifestHash, e.ownerUserId, e.deviceId, key, nonce, e.computerId, host, box, nonce, now - 1000, now + 60000, ['control', 'owner.devices']]
  const ch = ['donna/bootstrap-challenge/v1', e.bootstrapId, generation, digest(g), key, nonce, nonce, now, now + 40000]
  const codec = loadMainModule<typeof import('./bootstrap-codec.ts')>('fork-cloud/bootstrap-codec', { 'node:crypto': crypto, './crypto': wire }, { Buffer })
  const events = new Map<string, (...args: any[]) => void>(); const sent: any[] = []; let socketCount = 0, proofs = 0, persisted = false
  const socket = { send: (data: string) => sent.push(JSON.parse(data)), close: () => {}, addEventListener: (name: string, fn: (...args: any[]) => void) => events.set(name, fn) }
  const channel = { close() {}, receive() {}, call: async (method: string, body: unknown) => {
    if (method === 'bootstrap.challenge') return { kind: 'ok', value: sign(ch, hub) }
    assert.equal(method, 'bootstrap.prove'); assert.equal(persisted, true); assert.ok(Array.isArray(body)); proofs++
    return { kind: 'unknown', reason: 'timeout' }
  } }
  class FakeHandshake {
    result: { kind: string; channel?: unknown } | null = null
    constructor(_signer: unknown, _host: unknown, out: { sendRaw: (v: object) => void }) { out.sendRaw({ t: 'hello', v: 1, dk: key }) }
    receive() { this.result = { kind: 'ready', channel }; return true }
    abandon() {}
  }
  const mod = loadMainModule<typeof import('./bootstrap-channel.ts')>('fork-cloud/bootstrap-channel', {
    'node:crypto': crypto, 'node:perf_hooks': { performance }, './device-keys': { deviceSigner: async () => device }, './crypto': wire,
    './bootstrap-codec': codec, './channel': { Handshake: FakeHandshake }, './relay-socket': { deviceRelayURL: (origin: string, cid: string) => new URL(`/v1/relay/device/${cid}?v=1`, origin.replace('https:', 'wss:')), nodeSocket: () => { throw new Error('unexpected') } },
  }, { Buffer, URL, setTimeout, clearTimeout })
  const binding = { version: 1 as const, userId: e.ownerUserId, sessionId: 'sess_1', origin: 'https://cloud.example', issuer: e.issuer, deploymentId: e.deploymentId, manifestHash: e.manifestHash, deviceId: e.deviceId, deviceKey: e.deviceKey }
  let live = true
  const principal = { namespace: 'fixture', seal: { epoch: 1, userId: e.ownerUserId, sessionId: binding.sessionId }, deployment: { deploymentId: e.deploymentId, env: e.env, issuer: e.issuer, origin: binding.origin, manifestHash: e.manifestHash }, current: () => live, token: async () => 'fixture' }
  const account = { isCloudSessionCurrent: () => live, verifiedDeviceBinding: () => binding, on() {}, off() {} }
  await assert.rejects(mod.BootstrapChannel.create(account as never, principal, { ...e, deviceId: 'dev_other' }, wire.b64url(root.publicKey), sign(g, root), async () => {}, () => { throw new Error('must not open') }))
  const stream = await mod.BootstrapChannel.create(account as never, principal, e, wire.b64url(root.publicKey), sign(g, root), async value => { persisted = true; assert.equal(value.grant[0][1], e.bootstrapId) }, (url, headers) => {
    socketCount++; const parsed = new URL(url); assert.equal(parsed.searchParams.get('purpose'), 'bootstrap'); assert.equal(parsed.searchParams.get('bootstrapId'), e.bootstrapId); assert.match(headers['Codync-Sig'], new RegExp(`kid=${key}`)); return socket as never
  })
  const starting = stream.start()
  events.get('message')?.({ data: JSON.stringify({ t: 'presence', online: true }) })
  events.get('message')?.({ data: JSON.stringify({ t: 'welcome' }) })
  assert.equal((await starting).kind, 'ok'); assert.equal(sent[0].purpose, 'bootstrap'); assert.equal(sent[0].bootstrapId, e.bootstrapId); assert.equal(sent[0].pair, true)
  assert.equal((await stream.challenge()).kind, 'ok')
  assert.equal((await stream.prove()).kind, 'unknown')
  assert.equal((await stream.prove()).kind, 'notSent'); assert.equal((await stream.start()).kind, 'notSent')
  assert.equal(proofs, 1); assert.equal(socketCount, 1)
  live = false; stream.close()
})
