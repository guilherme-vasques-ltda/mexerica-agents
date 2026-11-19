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
const identifier = 'fixture@example.test'
const password = 'fixture-secret-with-spaces '
const credential = 'fixture-opaque'
const session = { id: 'session-a', status: 'active', user: { id: 'user-a' } }
const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()

test('senha recusada libera outra tentativa explícita; resposta perdida não reenvia fator após reinício', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'password-flow-'))
  const key = crypto.randomBytes(32)
  const safeStorage = {
    isEncryptionAvailable: () => true,
    encryptString(value: string) { const iv = crypto.randomBytes(12); const c = crypto.createCipheriv('aes-256-gcm', key, iv); const data = Buffer.concat([c.update(value, 'utf8'), c.final()]); return Buffer.concat([iv, c.getAuthTag(), data]) },
    decryptString(value: Buffer) { const c = crypto.createDecipheriv('aes-256-gcm', key, value.subarray(0, 12)); c.setAuthTag(value.subarray(12, 28)); return Buffer.concat([c.update(value.subarray(28)), c.final()]).toString('utf8') },
  }
  let factor = 0; let completed = false; const calls: string[] = []; const logs: unknown[] = []
  const handlers = new Map<string, (...args: never[]) => unknown>()
  const fetch = async (url: URL, init: { method: string; body?: string; headers: Record<string, string> }) => {
    // register() also reads the instance's social providers; e-mail and password do not depend on it.
    if (url.pathname === '/v1/environment') return { ok: true, status: 200, headers: { get: () => null }, json: async () => ({}) }
    calls.push(`${init.method} ${url.pathname}`)
    const reply = (value: unknown, auth: string | null = null, status = 200) => ({ ok: status === 200, status, headers: { get: () => auth }, json: async () => value })
    if (url.pathname === '/v1/client/sign_ins') {
      assert.equal(new URLSearchParams(init.body).get('identifier'), identifier)
      assert.equal(new URLSearchParams(init.body).has('password'), false)
      return reply({ response: { id: 'attempt-a', status: 'needs_first_factor' } }, credential)
    }
    if (url.pathname === '/v1/client/sign_ins/attempt-a/attempt_first_factor') {
      assert.equal(init.headers.Authorization, credential)
      assert.equal(new URLSearchParams(init.body).get('password'), password)
      factor++
      if (factor === 1) return reply({ errors: [{ code: 'form_password_incorrect' }] }, null, 422)
      completed = true; throw new Error('lost response')
    }
    if (url.pathname === '/v1/client/sign_ins/attempt-a') return reply({ response: { id: 'attempt-a', identifier, status: completed ? 'complete' : 'needs_first_factor', ...(completed ? { created_session_id: 'session-a' } : {}) } })
    if (url.pathname === '/v1/client') return reply({ response: { sessions: [session], last_active_session_id: session.id } })
    throw new Error('unexpected request')
  }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture', isPackaged: true }, ipcMain: { handle: (name: string, cb: (...args: never[]) => unknown) => handlers.set(name, cb) }, safeStorage },
    './auth': { AuthenticationEnded: class extends Error {} }, './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, __dirname: '/fixture/main', process: { platform: 'darwin', env: {} }, fetch, console: { warn: (...args: unknown[]) => logs.push(args) } })
  try {
    const first = new mod.AccountService(); await restore(first)
    assert.equal(await first.signInWithPassword(identifier, password), 'error')
    assert.equal(first.state.user, null)
    assert.equal(await first.signInWithPassword(identifier, password), 'error')
    assert.equal(factor, 2)
    assert.equal(first.reconciliationState().kind, 'session_unverified')
    const second = new mod.AccountService(); await restore(second)
    const currentUserId = () => second.state.user?.userId
    assert.equal(second.state.user, null)
    assert.equal(second.reconciliationState().kind, 'session_unverified')
    assert.equal((await second.reconcile('check')).kind, 'session_unverified')
    assert.equal(currentUserId(), undefined)
    assert.equal(factor, 2, 'nenhum replay de senha')
    assert.equal(calls.filter(c => c === 'POST /v1/client/sign_ins').length, 2)
    for (const name of fs.readdirSync(path.join(dir, 'account-password-attempt'))) {
      const bytes = fs.readFileSync(path.join(dir, 'account-password-attempt', name))
      assert.equal(bytes.includes(Buffer.from(password)), false)
      assert.equal(bytes.includes(Buffer.from(identifier)), false)
    }
    assert.equal(JSON.stringify(logs).includes(password), false)
    assert.equal(JSON.stringify(logs).includes(identifier), false)
    const third = new mod.AccountService()
    const ready = new Promise<void>(resolve => third.on('change', () => { if (third.state.ready) resolve() }))
    const frame = { url: 'file:///fixture/renderer/index.html?window=chat' }
    const sender = { mainFrame: frame, isDestroyed: () => false, send: () => {} }
    const window = { webContents: sender, isDestroyed: () => false }
    third.register(() => [window as never])
    const denied = handlers.get('account:signInWithPassword')!
    assert.equal(await denied({ senderFrame: null, sender: {} } as never, identifier as never, password as never), 'error')
    await ready
    let accepted = 0
    ;(third as unknown as { signInWithPassword: () => Promise<null> }).signInWithPassword = async () => { accepted++; return null }
    assert.equal(await denied({ senderFrame: frame, sender } as never, identifier as never, password as never), null)
    assert.equal(await denied({ senderFrame: { url: frame.url }, sender } as never, identifier as never, password as never), 'error')
    frame.url = 'https://evil.example.test/?window=chat'
    assert.equal(await denied({ senderFrame: frame, sender } as never, identifier as never, password as never), 'error')
    assert.equal(accepted, 1)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
