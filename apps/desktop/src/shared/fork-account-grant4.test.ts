import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()
type Init = { method: string; body?: string; headers: Record<string, string>; credentials?: string }
function world(dir: string, fetch: (url: URL, init: Init) => Promise<unknown>, authenticate: () => Promise<string> = async () => { throw new Error('browser should not open') }, fsAdapter: typeof fs = fs) {
  const key = crypto.randomBytes(32)
  const safeStorage = {
    isEncryptionAvailable: () => true,
    encryptString(value: string) { const iv = crypto.randomBytes(12); const c = crypto.createCipheriv('aes-256-gcm', key, iv); const data = Buffer.concat([c.update(value, 'utf8'), c.final()]); return Buffer.concat([iv, c.getAuthTag(), data]) },
    decryptString(value: Buffer) { const c = crypto.createDecipheriv('aes-256-gcm', key, value.subarray(0, 12)); c.setAuthTag(value.subarray(12, 28)); return Buffer.concat([c.update(value.subarray(28)), c.final()]).toString('utf8') },
  }
  return loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fsAdapter, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage },
    './auth': { authenticate, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin', env: {} }, fetch })
}

test('perda antes do id encerra apenas preparação local por ação explícita', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'password-create-')); const calls: string[] = []
  const mod = world(dir, async (url, init) => {
    calls.push(`${init.method} ${url.pathname}`)
    const body = new URLSearchParams(init.body)
    assert.equal(url.pathname, '/v1/client/sign_ins'); assert.deepEqual([...body.keys()], ['identifier'])
    assert.equal(init.credentials, 'omit'); assert.equal(init.headers.Authorization, undefined)
    throw new Error('response lost')
  })
  try {
    const account = new mod.AccountService(); await restore(account)
    assert.equal(await account.signInWithPassword('person@example.test', 'fixture password'), 'error')
    assert.equal(account.state.user, null); assert.equal(calls.length, 1)
    const restarted = new mod.AccountService(); await restore(restarted)
    assert.equal(restarted.reconciliationState().kind, 'session_unverified'); assert.equal(calls.length, 1)
    assert.equal((await restarted.reconcile('check')).kind, 'session_unverified')
    await restarted.signOut()
    assert.equal(restarted.reconciliationState().kind, 'none')
    assert.equal(calls.length, 1, 'fechamento local não envia ou repete nada')
    assert.equal(await restarted.signInWithPassword('person@example.test', 'fixture password'), 'error')
    assert.equal(calls.length, 2, 'somente nova ação explícita cria outra preparação')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
