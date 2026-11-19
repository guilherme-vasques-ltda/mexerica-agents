import assert from 'node:assert/strict'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs/promises'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

async function setup() {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'd171-retirement-fixture-'))
  let failDirSync = false
  let afterRename: (() => void) | null = null
  const io = { ...fs, rename: async (source: string, dest: string) => { await fs.rename(source, dest); if (afterRename) { const fn = afterRename; afterRename = null; fn() } }, open: async (name: string, flags: string, mode?: number) => {
    const handle = await fs.open(name, flags, mode)
    return { writeFile: (data: Buffer) => handle.writeFile(data), close: () => handle.close(), sync: async () => {
      if (name === dir && failDirSync) { failDirSync = false; throw new Error('fixture sync failure') }
      await handle.sync()
    } }
  } }
  const load = () => loadMainModule<typeof import('../main/fork-account/device-key.ts')>('fork-account/device-key', {
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: io },
    electron: { app: { getPath: () => dir }, safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
  }, { Buffer, process: { platform: 'darwin' } })
  const scope = { origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
  const makeBinding = (key: string) => ({ version: 1 as const, userId: 'user-a', sessionId: 'session-a', origin: 'https://cloud.example.test', issuer: 'https://issuer.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64), deviceId: 'dev_a', deviceKey: key })
  return { dir, load, makeBinding, scope, failSync: () => { failDirSync = true }, onRename: (fn: () => void) => { afterRename = fn } }
}

test('aposentadoria comprovada sobrevive reinício e só registro elegível cria outra geração', async () => {
  const f = await setup()
  try {
    const first = f.load(); const current = () => true
    const original = await first.deviceIdentity('user-a', current, 'register', f.scope)
    const binding = f.makeBinding(original.publicKey)
    await first.retireDeviceIdentity(binding, current)
    const restarted = f.load()
    assert.equal(await restarted.verifyDeviceGeneration('user-a', original.publicKey, f.scope), true)
    await assert.rejects(restarted.deviceIdentity('user-a', current, 'read', f.scope))
    await assert.rejects(restarted.deviceIdentity('user-a', current, 'sign', f.scope))
    const replacement = await restarted.deviceIdentity('user-a', current, 'register', f.scope)
    assert.notEqual(replacement.publicKey, original.publicKey)
    assert.equal((await f.load().deviceIdentity('user-a', current, 'register', f.scope)).publicKey, replacement.publicKey)
    assert.equal(await restarted.verifyDeviceGeneration('user-b', original.publicKey, f.scope), false)
  } finally { await fs.rm(f.dir, { recursive: true, force: true }) }
})

test('falha após marker permite retomada idempotente; divergência, corrupção e época velha não geram', async () => {
  const f = await setup()
  try {
    const device = f.load(); const yes = () => true
    const key = await device.deviceIdentity('user-a', yes, 'register', f.scope)
    const binding = f.makeBinding(key.publicKey)
    await assert.rejects(device.retireDeviceIdentity({ ...binding, deviceKey: 'x'.repeat(43) }, yes))
    await assert.rejects(device.retireDeviceIdentity(binding, () => false))
    assert.equal((await device.deviceIdentity('user-a', yes, 'read', f.scope)).publicKey, key.publicKey)
    f.failSync(); await assert.rejects(device.retireDeviceIdentity(binding, yes))
    const afterFailure = f.load()
    assert.equal(await afterFailure.verifyDeviceGeneration('user-a', key.publicKey, f.scope), true)
    await assert.rejects(afterFailure.deviceIdentity('user-a', yes, 'sign', f.scope))
    await assert.rejects(afterFailure.deviceIdentity('user-a', yes, 'register', { ...f.scope, deploymentId: 'other' }))
    await afterFailure.retireDeviceIdentity(binding, yes)
    const marker = (await fs.readdir(f.dir)).find((name) => name.endsWith('.generation'))!
    await fs.rm(path.join(f.dir, marker))
    await assert.rejects(afterFailure.deviceIdentity('user-a', yes, 'register', f.scope))
    assert.equal(await afterFailure.verifyDeviceGeneration('user-a', key.publicKey, f.scope), true)
    await afterFailure.retireDeviceIdentity(binding, yes)
    await fs.writeFile(path.join(f.dir, marker), Buffer.from('invalid'))
    await assert.rejects(afterFailure.deviceIdentity('user-a', yes, 'register', f.scope))
    assert.equal(await afterFailure.verifyDeviceGeneration('user-a', key.publicKey, f.scope), false)
  } finally { await fs.rm(f.dir, { recursive: true, force: true }) }
})

test('Cloud confirma DELETE por consulta exata, aposenta e registra nova chave após reinício', async () => {
  const f = await setup()
  try {
    const seal = { userId: 'user-a', sessionId: 'session-a', epoch: 1 }
    let handler!: (...args: any[]) => Promise<any>
    let capture!: (captured: typeof seal, binding: ReturnType<typeof f.makeBinding>) => ((jwt: string, current: () => boolean) => Promise<any>) | null
    let retire!: (binding: ReturnType<typeof f.makeBinding>, current: () => boolean) => Promise<boolean>
    let key = ''; let revoked = false; let hide = false
    const account = { state: { cloudURL: f.scope.origin }, deviceIdentityScope: () => f.scope,
      captureCloudSession: () => seal, isCloudSessionCurrent: () => true, tokenForCloud: async () => 'fixture-jwt',
      setReconciliationKeyReader: () => {}, setReconciliationRegistrar: () => {}, setReconciliationKeyVerifier: () => {},
      setDeviceIdentityRetirer: (fn: typeof retire) => { retire = fn }, setDeviceRevoker: (fn: typeof capture) => { capture = fn },
      setBindingRestorer: () => {}, on: () => {}, saveDeviceBinding: async (_s: unknown, id: string) => ({ deviceId: id }) }
    const cloud = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
      '../shared/fork-features': { forkFeatures: { cloud: true } }, './fork-cloud/account-relay': { openAccountRelay: () => {} },
      'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: fs },
      electron: { app: { getPath: () => f.dir }, safeStorage: { isEncryptionAvailable: () => true, encryptString: (v: string) => Buffer.from(v), decryptString: (v: Buffer) => v.toString() }, ipcMain: { handle: (_: string, fn: typeof handler) => { handler = fn } } },
    }, { Buffer, URL, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string; headers: Record<string, string> }) => {
      if (init.method === 'POST') {
        key = /kid=([^,]+)/.exec(init.headers['Codync-Sig'])![1]!; revoked = false
        return { ok: true, text: async () => JSON.stringify({ device: { deviceId: 'dev_a', deviceKey: key } }) }
      }
      if (init.method === 'DELETE') { revoked = true; return { ok: true } }
      assert.equal(url.pathname, '/v1/devices')
      return { ok: true, json: async () => ({ devices: hide ? [] : [{ deviceId: 'dev_a', deviceKey: key, revoked }] }) }
    } })
    cloud.registerCloud(account as never)
    assert.equal((await handler(null, 'POST', '/v1/devices', {}, true)).ok, true)
    const oldKey = key; const revoke = capture(seal, f.makeBinding(oldKey))!
    hide = true; assert.equal((await revoke('fixture-jwt', () => true)).kind, 'unknown')
    hide = false; assert.equal((await revoke('fixture-jwt', () => true)).kind, 'revoked')
    assert.equal(await retire(f.makeBinding(oldKey), () => true), true)
    assert.equal((await handler(null, 'GET', '/v1/computers', undefined, true)).error.code, 'notSent')
    assert.equal((await handler(null, 'POST', '/v1/devices', {}, true)).ok, true)
    assert.notEqual(key, oldKey)
  } finally { await fs.rm(f.dir, { recursive: true, force: true }) }
})


test('época perdida durante publicação mantém geração preparada sem assinatura até novo registro', async () => {
  const f = await setup()
  try {
    const d = f.load(); const old = await d.deviceIdentity('user-a', () => true, 'register', f.scope)
    await d.retireDeviceIdentity(f.makeBinding(old.publicKey), () => true)
    let current = true; f.onRename(() => { current = false })
    await assert.rejects(d.deviceIdentity('user-a', () => current, 'register', f.scope))
    const restarted = f.load()
    await assert.rejects(restarted.deviceIdentity('user-a', () => true, 'sign', f.scope))
    const resumed = await restarted.deviceIdentity('user-a', () => true, 'register', f.scope)
    assert.notEqual(resumed.publicKey, old.publicKey)
    assert.equal((await restarted.deviceIdentity('user-a', () => true, 'register', f.scope)).publicKey, resumed.publicKey)
  } finally { await fs.rm(f.dir, { recursive: true, force: true }) }
})
