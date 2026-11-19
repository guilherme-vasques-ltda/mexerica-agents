import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import { accountConfig } from '../main/fork-account/config.ts'
import * as wire from '../main/fork-cloud/crypto.ts'
import { deferred, loadMainModule } from './test-support/main-module.ts'

const { matchesDeployment } = loadMainModule<typeof import('../main/fork-cloud/pins.ts')>('fork-cloud/pins', {
  'node:crypto': crypto, 'node:path': path,
  'node:fs': { promises: { readFile: () => { throw new Error('unexpected pin read') } } },
  electron: {}, './crypto': wire,
  './device-keys': { deviceSigner: () => assert.fail('pure deployment comparison does not read a device key') },
}, { Buffer })

const host = 'identity.example.test'
const build = { environment: 'dev', deploymentEnv: 'dev', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: 'https://issuer.example.test', cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const deployment = { deploymentId: 'fixture', env: 'dev' as const, issuer: build.clerkIssuer, origin: build.cloudURL, manifestHash: build.manifestHash }

test('env próprio e resumo fixo recusam ausência, troca ou health divergente', () => {
  assert.equal(accountConfig(build).kind, 'configured')
  assert.equal(accountConfig({ ...build, deploymentEnv: null }).kind, 'unavailable')
  assert.equal(accountConfig({ ...build, deploymentEnv: 'main' }).kind, 'unavailable')
  assert.equal(matchesDeployment(deployment, { ok: true, deployment }), true)
  assert.equal(matchesDeployment(deployment, { ok: true, deployment: { ...deployment, env: 'main' } }), false)
  assert.equal(matchesDeployment(deployment, { ok: true, deployment: { ...deployment, manifestHash: 'b'.repeat(64) } }), false)
})

test('cliente fechado recusa rota livre e não repete efeito após transição de época', async () => {
  let current = true, calls = 0
  const reply = deferred<Response>()
  const principal = { deployment, seal: { epoch: 1, sessionId: 'fixture', userId: 'fixture' }, namespace: 'fixture', current: () => current, token: async () => 'fixture' }
  const api = loadMainModule<typeof import('../main/fork-cloud/account-api.ts')>('fork-cloud/account-api', {
    './crypto': { unb64url: (s: unknown, n: number) => typeof s === 'string' && Buffer.from(s, 'base64url').length === n ? new Uint8Array(n) : null, signatureHeader: () => 'fixture' },
    './device-keys': { deviceSigner: async () => ({ publicKey: new Uint8Array(32), sign: () => new Uint8Array(64) }) },
    './pins': { matchesDeployment },
  }, { URL, Buffer, fetch: async (_url: URL, init: { method: string }) => { calls++; return init.method === 'GET' ? { ok: true, json: async () => ({ ok: true, deployment }) } : reply.promise } })
  const client = new api.AccountApi(principal)
  assert.equal((await client.execute({ kind: 'health' })).kind, 'ok')
  assert.equal((await client.execute({ kind: 'revokeDevice', deviceId: '../admin' })).kind, 'notSent')
  const pending = client.execute({ kind: 'createClaim' })
  await new Promise<void>((resolve) => setImmediate(resolve))
  current = false
  reply.resolve({ ok: true, json: async () => ({}) } as Response)
  assert.equal((await pending).kind, 'unknown')
  assert.equal(calls, 2)
})

test('troca de época durante aquisição da assinatura impede envio', async () => {
  let current = true, calls = 0
  const signer = deferred<{ publicKey: Uint8Array; sign(message: Uint8Array): Uint8Array }>()
  const principal = { deployment, seal: { epoch: 1, sessionId: 'fixture', userId: 'fixture' }, namespace: 'fixture', current: () => current, token: async () => 'fixture' }
  const api = loadMainModule<typeof import('../main/fork-cloud/account-api.ts')>('fork-cloud/account-api', {
    './crypto': { unb64url: () => null, signatureHeader: () => 'fixture' },
    './device-keys': { deviceSigner: () => signer.promise }, './pins': { matchesDeployment },
  }, { URL, Buffer, fetch: async () => { calls++; return { ok: true, json: async () => ({ ok: true, deployment }) } } })
  const client = new api.AccountApi(principal)
  assert.equal((await client.execute({ kind: 'health' })).kind, 'ok')
  const pending = client.execute({ kind: 'listComputers' })
  await new Promise<void>((resolve) => setImmediate(resolve))
  current = false
  signer.resolve({ publicKey: new Uint8Array(32), sign: () => new Uint8Array(64) })
  assert.equal((await pending).kind, 'notSent')
  assert.equal(calls, 1)
})

test('commit e envios preservam uma captura e leitura não vira registro', async () => {
  let reads = 0, calls = 0
  let keyReadFails = false
  const signedKeys: number[] = []
  const principal = { deployment, seal: { epoch: 1, sessionId: 'fixture', userId: 'fixture' }, namespace: 'fixture', current: () => true, token: async () => 'fixture' }
  const api = loadMainModule<typeof import('../main/fork-cloud/account-api.ts')>('fork-cloud/account-api', {
    './crypto': {
      unb64url: (value: unknown, bytes: number) => typeof value === 'string' && Buffer.from(value, 'base64url').length === bytes ? new Uint8Array(bytes) : null,
      signatureHeader: (signer: { publicKey: Uint8Array }) => { signedKeys.push(signer.publicKey[0]!); return 'fixture' },
    },
    './device-keys': { deviceSigner: async () => { reads++; if (keyReadFails) throw new Error('fixture unavailable'); return { publicKey: new Uint8Array(32).fill(reads), sign: () => new Uint8Array(64) } } },
    './pins': { matchesDeployment },
  }, { URL, Buffer, fetch: async () => { calls++; return { ok: true, json: async () => ({ ok: true, deployment }) } } })
  const client = new api.AccountApi(principal)
  assert.equal((await client.execute({ kind: 'health' })).kind, 'ok')
  const key = await client.captureDeviceKey()
  assert.equal(key[0], 1)
  key.fill(99)
  assert.equal((await client.execute({ kind: 'listComputers' })).kind, 'ok')
  assert.equal((await client.execute({ kind: 'requestAccess', computerId: Buffer.alloc(16).toString('base64url'), commit: Buffer.alloc(32).toString('base64url') })).kind, 'ok')
  assert.deepEqual(signedKeys, [1, 1])
  assert.equal(reads, 1)
  assert.equal((await client.captureDeviceKey())[0], 1)
  assert.equal((await client.execute({ kind: 'registerDevice', name: 'fixture', platform: 'macos' })).kind, 'notSent')
  assert.equal(calls, 3)
  assert.equal(reads, 1)
  keyReadFails = true
  const unavailable = new api.AccountApi(principal)
  assert.equal((await unavailable.execute({ kind: 'health' })).kind, 'ok')
  await assert.rejects(unavailable.captureDeviceKey())
  await assert.rejects(unavailable.captureDeviceKey())
  assert.equal((await unavailable.execute({ kind: 'registerDevice', name: 'fixture', platform: 'macos' })).kind, 'notSent')
  assert.equal(reads, 2)
  assert.equal(calls, 4)
})
