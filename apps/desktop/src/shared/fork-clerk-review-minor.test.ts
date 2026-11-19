import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import * as fs from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { loadMainModule } from './test-support/main-module.ts'

test('IPC de conector não substitui nem recebe retorno da conta', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  let handler!: (_event: unknown, url: string, scheme: string) => Promise<string>
  const auth = loadMainModule<typeof import('../main/auth.ts')>('auth', {
    electron: { ipcMain: { handle: (_name: string, fn: typeof handler) => { handler = fn } } },
    './environment': { identity: { scheme: 'donnabot', appId: 'app.fixture' } },
    './navigation': { openExternalURL: async () => true },
    '../shared/navigation-policy': { parseNavigationURL: (url: string) => new URL(url) },
  }, { URL, setTimeout, clearTimeout })
  auth.registerAuthIPC()
  const callback = 'app.fixture://callback?attempt=fixture'
  const account = auth.authenticate('https://identity.example.test/oauth', 'app.fixture', callback)
  await assert.rejects(handler(null, 'https://connector.example.test/oauth', 'app.fixture'))
  await assert.rejects(handler(null, 'https://connector.example.test/oauth', 'donnabot'))
  auth.handleURL(callback + '&rotating_token_nonce=fixture')
  assert.equal(await account, callback + '&rotating_token_nonce=fixture')
})

test('falha parcial não publica chave e publicação concorrente não sobrescreve identidade', async () => {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'd171-device-key-'))
  let handler!: (...args: any[]) => Promise<any>
  let partial = true
  let collision = false
  const winner = Buffer.from('existing-encrypted-fixture')
  const files = {
    ...fs,
    open: async (file: string, flags: string, mode?: number) => {
      const handle = await fs.open(file, flags, mode)
      if (flags !== 'wx') return handle
      return {
        writeFile: async (data: Buffer) => {
          if (partial) { await handle.writeFile(data.subarray(0, 3)); throw new Error('fixture I/O failure') }
          await handle.writeFile(data)
        },
        sync: () => handle.sync(), close: () => handle.close(),
      }
    },
    link: async (source: string, destination: string) => {
      if (collision) await fs.writeFile(destination, winner, { flag: 'wx', mode: 0o600 })
      await fs.link(source, destination)
    },
  }
  const seal = { epoch: 1, sessionId: 'fixture-session', userId: 'fixture-user' }
  const account = {
    deviceIdentityScope: () => ({ origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }), state: { cloudURL: 'https://cloud.example.test' }, captureCloudSession: () => seal,
    isCloudSessionCurrent: () => true, tokenForCloud: async () => 'fixture-jwt',
    setDeviceRevoker: () => {}, setBindingRestorer: () => {}, setReconciliationKeyVerifier: () => {}, on: () => {},
  }
  let sends = 0
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: true } },
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: files },
    electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value), decryptString: (data: Buffer) => data.toString() },
      ipcMain: { handle: (_name: string, fn: typeof handler) => { handler = fn } } },
  }, { Buffer, URL, process: { platform: 'darwin' }, fetch: async () => { sends++; return { ok: true, text: async () => '{}' } } })
  try {
    cloud.registerCloud(account as never)
    const failed = await handler(null, 'POST', '/v1/devices', {}, true)
    assert.equal(failed.error.code, 'notSent')
    assert.deepEqual(await fs.readdir(dir), [])
    assert.equal(sends, 0)

    partial = false
    collision = true
    const refused = await handler(null, 'POST', '/v1/devices', {}, true)
    assert.equal(refused.error.code, 'notSent')
    const names = await fs.readdir(dir)
    assert.equal(names.length, 1)
    assert.match(names[0]!, /^device-key-[a-f0-9]+\.bin$/)
    assert.deepEqual(await fs.readFile(path.join(dir, names[0]!)), winner)
    assert.equal((await fs.stat(path.join(dir, names[0]!))).mode & 0o777, 0o600)
    assert.equal(sends, 0)
  } finally { await fs.rm(dir, { recursive: true, force: true }) }
})

test('reabertura sincroniza a chave existente antes de assinar e não gera outra', async () => {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'd171-existing-key-'))
  const key = crypto.generateKeyPairSync('ed25519').privateKey.export({ format: 'jwk' })
  const file = path.join(dir, 'device-key-' + crypto.createHash('sha256').update('fixture-user').digest('hex').slice(0, 16) + '.bin')
  const events: string[] = []
  await fs.writeFile(file, Buffer.from(key.d!), { mode: 0o600 })
  let handler!: (...args: any[]) => Promise<any>
  const files = { ...fs, open: async (name: string, flags: string) => {
    const opened = await fs.open(name, flags)
    return { sync: async () => { events.push(name === file ? 'key-sync' : 'dir-sync'); await opened.sync() }, close: () => opened.close() }
  } }
  const seal = { epoch: 1, sessionId: 'fixture-session', userId: 'fixture-user' }
  let verify!: (userId: string, expectedKey: string) => Promise<boolean>
  const account = { deviceIdentityScope: () => ({ origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }), state: { cloudURL: 'https://cloud.example.test' }, captureCloudSession: () => seal,
    isCloudSessionCurrent: () => true, tokenForCloud: async () => 'fixture-jwt',
    setDeviceRevoker: () => {}, setBindingRestorer: () => {}, on: () => {},
    setReconciliationKeyVerifier: (fn: typeof verify) => { verify = fn } }
  const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
    './fork-cloud/account-relay': { openAccountRelay: () => { throw new Error('not used') } },
    '../shared/fork-features': { forkFeatures: { cloud: true } },
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: files },
    electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, decryptString: (data: Buffer) => data.toString() },
      ipcMain: { handle: (_name: string, fn: typeof handler) => { handler = fn } } },
  }, { Buffer, URL, process: { platform: 'darwin' }, fetch: async () => { events.push('send'); return { ok: true, text: async () => '{}' } } })
  try {
    cloud.registerCloud(account as never)
    assert.equal(await verify('fixture-user', key.x!), true)
    assert.equal(await verify('fixture-user', 'x'.repeat(43)), false)
    assert.equal(await verify('missing-user', key.x!), false)
    assert.deepEqual(events, [], 'verificador não cria nem sincroniza outra identidade')
    assert.equal((await handler(null, 'GET', '/v1/computers', undefined, true)).ok, true)
    assert.deepEqual(events, ['key-sync', 'dir-sync', 'send'])
    assert.deepEqual(await fs.readdir(dir), [path.basename(file)])
  } finally { await fs.rm(dir, { recursive: true, force: true }) }
})
