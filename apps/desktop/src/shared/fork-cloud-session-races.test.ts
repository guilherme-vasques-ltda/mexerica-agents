import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

function fixture() {
  let handler: (...args: any[]) => Promise<any> = async () => { throw new Error('not registered') }
  let epoch = 1
  let sent = 0
  const token = deferred<string>()
  const response = deferred<{ ok: boolean; text(): Promise<string> }>()
  const seal = { epoch: 1, sessionId: 'session-a', userId: 'user-a' }
  const account = {
    deviceIdentityScope: () => ({ origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }), state: { cloudURL: 'https://cloud.example.test' },
    captureCloudSession: () => seal,
    isCloudSessionCurrent: (s: typeof seal) => s.epoch === epoch,
    tokenForCloud: () => token.promise,
    setDeviceRevoker: () => {}, setReconciliationRegistrar: () => {}, setReconciliationKeyReader: () => {}, setReconciliationKeyVerifier: () => {}, setBindingRestorer: () => {}, on: () => {},
  }
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: true } },
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: {} },
    electron: { app: {}, safeStorage: {}, ipcMain: { handle: (_channel: string, fn: typeof handler) => { handler = fn } } },
  }, { Buffer, URL, process: { platform: 'darwin' }, fetch: async () => { sent++; return response.promise } })
  cloud.registerCloud(account as never)
  return { handler, token, response, change: () => { epoch++ }, sent: () => sent }
}

test('troca durante obtenção de token impede envio da requisição antiga', async () => {
  const f = fixture()
  const pending = f.handler(null, 'GET', '/v1/computers', undefined, false)
  f.change()
  f.token.resolve('fixture-jwt')
  const result = await pending
  assert.equal(result.error.code, 'notSent')
  assert.equal(f.sent(), 0)
})

test('troca após envio descarta retorno e informa resultado desconhecido', async () => {
  const f = fixture()
  const pending = f.handler(null, 'GET', '/v1/computers', undefined, false)
  f.token.resolve('fixture-jwt')
  await settle()
  assert.equal(f.sent(), 1)
  f.change()
  f.response.resolve({ ok: true, text: async () => '{"fixture":true}' })
  const result = await pending
  assert.equal(result.error.code, 'unknown')
})

test('DELETE usa somente o aparelho e a identidade capturados no registro concluído', async () => {
  let handler: (...args: any[]) => Promise<any> = async () => { throw new Error('not registered') }
  let capture: ((seal: any, binding: any) => ((jwt: string, current: () => boolean) => Promise<any>) | null) | null = null
  const seal = { epoch: 1, sessionId: 'fixture-session', userId: 'fixture-user' }
  const key = crypto.generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' })
  const calls: { method: string; pathname: string }[] = []
  const account = {
    deviceIdentityScope: () => ({ origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }), state: { cloudURL: 'https://cloud.example.test' }, captureCloudSession: () => seal,
    isCloudSessionCurrent: () => true, tokenForCloud: async () => 'fixture-jwt',
    setDeviceRevoker: (fn: typeof capture) => { capture = fn }, setReconciliationRegistrar: () => {}, setReconciliationKeyReader: () => {}, setReconciliationKeyVerifier: () => {}, setBindingRestorer: () => {}, saveDeviceBinding: async (_seal: unknown, deviceId: string) => ({ deviceId }), on: () => {},
  }
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: true } },
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: { readFile: async (name: string) => { if (name.endsWith('.generation')) throw Object.assign(new Error('missing'), { code: 'ENOENT' }); return Buffer.from('sealed') }, open: async () => ({ sync: async () => {}, close: async () => {} }) } },
    electron: { app: { getPath: () => '/fixture' }, safeStorage: { isEncryptionAvailable: () => true, decryptString: () => key.d }, ipcMain: { handle: (_channel: string, fn: typeof handler) => { handler = fn } } },
  }, { Buffer, URL, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string }) => {
    calls.push({ method: init.method, pathname: url.pathname })
    return { ok: true, text: async () => JSON.stringify({ device: { deviceId: 'dev_fixture', deviceKey: key.x } }), json: async () => ({ devices: [{ deviceId: 'dev_fixture', deviceKey: key.x, revoked: true }] }) }
  } })
  cloud.registerCloud(account as never)
  const registration = await handler(null, 'POST', '/v1/devices', {}, true)
  assert.equal(registration.ok, true)
  const revoke = (capture as unknown as (value: typeof seal, binding: unknown) => ((jwt: string, current: () => boolean) => Promise<any>) | null)(seal, { userId: seal.userId, sessionId: seal.sessionId, deviceId: 'dev_fixture', deviceKey: key.x, origin: 'https://cloud.example.test' })
  assert.ok(revoke)
  assert.equal((await revoke('fixture-jwt', () => true)).kind, 'revoked')
  assert.deepEqual(calls, [{ method: 'POST', pathname: '/v1/devices' }, { method: 'DELETE', pathname: '/v1/devices/dev_fixture' }, { method: 'GET', pathname: '/v1/devices' }])
})

test('restauração só aceita a chave existente correspondente e nunca gera outra', async () => {
  let restore: ((seal: any, binding: any) => Promise<void>) | undefined
  let capture: ((seal: any, binding: any) => ((jwt: string, current: () => boolean) => Promise<any>) | null) | undefined
  const seal = { epoch: 1, sessionId: 'session-a', userId: 'user-a' }
  const key = crypto.generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' })
  let writes = 0
  const account = { deviceIdentityScope: () => ({ origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }), state: { cloudURL: 'https://cloud.example.test' }, isCloudSessionCurrent: () => true,
    setDeviceRevoker: (fn: typeof capture) => { capture = fn }, setReconciliationRegistrar: () => {}, setReconciliationKeyReader: () => {}, setReconciliationKeyVerifier: () => {}, setBindingRestorer: (fn: typeof restore) => { restore = fn }, on: () => {} }
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: true } }, 'node:crypto': crypto, 'node:path': path,
    'node:fs': { promises: { readFile: async (name: string) => { if (name.endsWith('.generation')) throw Object.assign(new Error('missing'), { code: 'ENOENT' }); return Buffer.from('sealed') }, open: async () => ({ sync: async () => {}, close: async () => {} }), writeFile: async () => { writes++ } } },
    electron: { app: { getPath: () => '/fixture' }, safeStorage: { isEncryptionAvailable: () => true, decryptString: () => key.d }, ipcMain: { handle: () => {} } },
  }, { Buffer, URL, process: { platform: 'darwin' } })
  cloud.registerCloud(account as never)
  await restore!(seal, { origin: account.state.cloudURL, deviceId: 'dev_fixture', deviceKey: 'x'.repeat(43) })
  assert.equal(capture!(seal, { userId: seal.userId, sessionId: seal.sessionId, deviceId: 'dev_fixture', deviceKey: key.x, origin: account.state.cloudURL }), null)
  await restore!(seal, { origin: account.state.cloudURL, deviceId: 'dev_fixture', deviceKey: key.x })
  assert.equal(typeof capture!(seal, { userId: seal.userId, sessionId: seal.sessionId, deviceId: 'dev_fixture', deviceKey: key.x, origin: account.state.cloudURL }), 'function')
  assert.equal(writes, 0)
})
