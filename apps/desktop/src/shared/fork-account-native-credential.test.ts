import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { registerHooks } from 'node:module'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

registerHooks({ resolve(specifier, context, nextResolve) {
  if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
    const url = new URL(`${specifier}.ts`, context.parentURL)
    if (existsSync(url)) return nextResolve(url.href, context)
  }
  return nextResolve(specifier, context)
} })

const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const client = { sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' }
type Internal = { restore(): Promise<void>; request(method: 'GET', path: string): Promise<unknown>; deviceToken: string | null; validPending(value: unknown): boolean; persistPending(value: unknown): Promise<void>; pendingExit: { restricted?: { credential: string; verified: boolean } } | null; restricted: unknown }

// Real AccountService and AccountStorage, private temporary directory, authenticated
// fixture encryption and a transport fake that cannot contact the network.
test('credencial nativa opaca percorre create, confirmação, restauração e saída pendente', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'native-credential-'))
  let dataDir = dir
  const extraDirs: string[] = []
  const key = crypto.randomBytes(32)
  const safeStorage = {
    isEncryptionAvailable: () => true,
    encryptString(value: string) { const iv = crypto.randomBytes(12); const cipher = crypto.createCipheriv('aes-256-gcm', key, iv); const data = Buffer.concat([cipher.update(value, 'utf8'), cipher.final()]); return Buffer.concat([iv, cipher.getAuthTag(), data]) },
    decryptString(value: Buffer) { const decipher = crypto.createDecipheriv('aes-256-gcm', key, value.subarray(0, 12)); decipher.setAuthTag(value.subarray(12, 28)); return Buffer.concat([decipher.update(value.subarray(28)), decipher.final()]).toString('utf8') },
  }
  const credential = 'fixture_opaque.client+one/='
  const rotated = 'fixture_rotated.client+two/='
  const restrictedCredential = 'fixture_opaque.restricted+three/='
  const restrictedRotated = 'fixture_rotated.restricted+four/='
  let callback = ''; let phase = 'login'; let calls: { path: string; authorization: string | undefined }[] = []
  let header: string | null = null
  const diagnostics: unknown[] = []
  const fetch = async (url: URL, init: { method: string; headers?: Record<string, string>; body?: string }) => {
    calls.push({ path: url.pathname, authorization: init.headers?.Authorization })
    let response: unknown = {}
    let authorization: string | null = null
    if (phase === 'login') {
      switch (url.pathname) {
        case '/v1/client/sign_ins': callback = new URLSearchParams(init.body).get('redirect_url')!; authorization = credential; response = { id: 'attempt-a', status: 'needs_first_factor' }; break
        case '/v1/client/sign_ins/attempt-a/prepare_first_factor': response = { id: 'attempt-a', first_factor_verification: { external_verification_redirect_url: 'https://identity.example.test/oauth' } }; break
        case '/v1/client/sign_ins/attempt-a': response = { id: 'attempt-a', status: 'complete', created_session_id: 'session-a' }; break
        case '/v1/client': response = client; break
        default: throw new Error('unexpected login request')
      }
    } else if (phase === 'badlogin') { authorization = 'bad\rvalue'; response = { id: 'attempt-a' } }
    else if (phase === 'restore') { assert.equal(url.pathname, '/v1/client'); response = client; authorization = rotated }
    else if (phase === 'exit') { assert.equal(url.pathname, '/v1/client/sessions/session-a/remove'); return { ok: false, status: 503, json: async () => ({}) } }
    else if (phase === 'restricted' || phase === 'restrictedClear') {
      switch (url.pathname) {
        case '/v1/client/sign_ins': callback = new URLSearchParams(init.body).get('redirect_url')!; authorization = restrictedCredential; response = { id: 'restricted-attempt', status: 'needs_first_factor' }; break
        case '/v1/client/sign_ins/restricted-attempt/prepare_first_factor': response = { id: 'restricted-attempt', first_factor_verification: { external_verification_redirect_url: 'https://identity.example.test/oauth' } }; break
        case '/v1/client/sign_ins/restricted-attempt': response = { id: 'restricted-attempt', status: 'complete', created_session_id: 'restricted-session' }; break
        case '/v1/client': authorization = phase === 'restrictedClear' ? 'Bearer' : restrictedRotated; response = { sessions: [{ id: 'restricted-session', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'restricted-session' }; break
        case '/v1/client/sessions/restricted-session/remove': return { ok: false, status: 503, json: async () => ({}) }
        default: throw new Error('unexpected restricted request')
      }
    }
    else { authorization = header; response = {} }
    // A raw header stub also permits testing invalid bytes that Headers itself rejects.
    return { ok: true, status: 200, headers: { get: () => authorization }, json: async () => ({ response }) }
  }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: { app: { getPath: () => dataDir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage },
    './auth': { authenticate: async () => callback, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch, console: { warn: (...args: unknown[]) => diagnostics.push(args) } })
  try {
    const first = new mod.AccountService(); first.state = { ...first.state, socialProviders: ['google'] }; await (first as unknown as Internal).restore()
    assert.equal(await first.signIn('google'), null)
    assert.equal(first.state.user?.userId, 'user-a')
    assert.deepEqual(calls.map((item) => item.authorization), [undefined, credential, credential, credential])
    assert.ok(first.captureCloudSession())
    // The bytes at rest are not plaintext; service still confirms before publication.
    const diskFiles = fs.readdirSync(dir).filter(name => fs.statSync(path.join(dir, name)).isFile()).map((name) => fs.readFileSync(path.join(dir, name)))
    assert.ok(diskFiles.every((bytes) => !bytes.includes(Buffer.from(credential))))
    phase = 'restore'; calls = []
    const second = new mod.AccountService(); await (second as unknown as Internal).restore()
    assert.equal(calls[0]?.authorization, credential)
    assert.equal(second.state.user?.userId, 'user-a')
    assert.equal((second as unknown as Internal).deviceToken, rotated)
    phase = 'exit'
    await second.signOut()
    assert.equal(second.reconciliationState().kind, 'none')
    assert.equal(fs.existsSync(path.join(dir, 'account.guard')), false)
    assert.equal(fs.readdirSync(dir).some(name => /^account.*\.bin$/.test(name)), false)
    assert.equal(second.captureCloudSession(), null)

    // A separate empty store probes absent, rotation, clear and malformed response
    // on the real request path without publishing an unconfirmed session.
    dataDir = mkdtempSync(path.join(tmpdir(), 'native-probe-')); extraDirs.push(dataDir)
    phase = 'probe'; const probe = new mod.AccountService(); const api = probe as unknown as Internal
    api.deviceToken = credential
    header = null; await api.request('GET', '/probe'); assert.equal(api.deviceToken, credential)
    header = `  ${rotated}  `; await api.request('GET', '/probe'); assert.equal(api.deviceToken, rotated)
    header = 'Bearer'; await api.request('GET', '/probe'); assert.equal(api.deviceToken, null)
    api.deviceToken = credential; header = ''; await api.request('GET', '/probe'); assert.equal(api.deviceToken, null)
    api.deviceToken = credential; header = 'invalid\rvalue'; await assert.rejects(api.request('GET', '/probe')); assert.equal(api.deviceToken, credential)
    dataDir = mkdtempSync(path.join(tmpdir(), 'native-bad-')); extraDirs.push(dataDir)
    phase = 'badlogin'; const bad = new mod.AccountService(); bad.state = { ...bad.state, socialProviders: ['google'] }; await (bad as unknown as Internal).restore()
    assert.equal(await bad.signIn('google'), 'error')
    const signInFailures = diagnostics.filter((entry) => Array.isArray(entry) && entry[0] === 'account:sign-in-failed')
    assert.equal(JSON.stringify(signInFailures), JSON.stringify([['account:sign-in-failed', { stage: 'create', status: null, reason: 'native_credential_format' }]]))
    assert.ok(diagnostics.some((entry) => Array.isArray(entry) && entry[0] === 'account:recovery-diagnostic'))
    assert.ok(diagnostics.every((entry) => Array.isArray(entry) && (entry[0] === 'account:sign-in-failed' || entry[0] === 'account:recovery-diagnostic')))
    assert.equal(JSON.stringify(diagnostics).includes('bad\rvalue'), false)
    assert.equal(JSON.stringify(diagnostics).includes(credential), false)
    assert.equal(JSON.stringify(diagnostics).includes(rotated), false)
    assert.equal(JSON.stringify(diagnostics).includes(restrictedCredential), false)
    assert.equal(JSON.stringify(diagnostics).includes(restrictedRotated), false)
  } finally { rmSync(dir, { recursive: true, force: true }); for (const extra of extraDirs) rmSync(extra, { recursive: true, force: true }) }
})

test('JSON de saída cercada não vira credencial nem perde a referência', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'native-pending-json-'))
  let calls = 0
  // Decryption is the fake OS boundary; guard, payload, fence and restore are real.
  const safeStorage = { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value), decryptString: (value: Buffer) => value.toString('utf8') }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage },
    './auth': { authenticate: async () => { throw new Error('no browser') }, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async (_url: URL, init: { headers: Record<string, string> }) => { assert.equal(init.headers.Authorization.startsWith('{'), false); calls++; return { ok: false, status: 401, headers: { get: () => null }, json: async () => ({}) } } })
  try {
    const service = new mod.AccountService()
    const internal = service as unknown as Internal & { storage: { quarantine(sealed: Buffer, current: () => void): Promise<void>; fence(exitRequested: boolean): Promise<void>; load(): Promise<{ sealed?: Buffer; exitRequested?: boolean }> } }
    const sealed = safeStorage.encryptString(JSON.stringify({ version: 1, userId: 'user-a', sessionId: 'session-a', credential: 'fixture_retained.client', binding: null, restricted: { credential: 'fixture_retained.restricted', sessionId: 'restricted-a', verified: false } }))
    await internal.storage.quarantine(sealed, () => {})
    await internal.storage.fence(true)
    const before = await internal.storage.load()
    assert.deepEqual(before.sealed, sealed)
    assert.equal(before.exitRequested, true)
    await internal.restore()
    assert.ok(calls <= 2, 'apenas credenciais capturadas, nunca JSON como Authorization')
    assert.equal(internal.deviceToken, null)
    assert.equal(service.state.user, null)
    assert.equal(service.captureCloudSession(), null)
    await service.signOut()
    assert.equal((await internal.storage.load() as { kind: string }).kind, 'empty', 'saída local arquiva a referência sem usá-la como Authorization')
    const restarted = new mod.AccountService(); await (restarted as unknown as Internal).restore()
    assert.equal(restarted.state.user, null)
    assert.equal((await internal.storage.load() as { kind: string }).kind, 'empty')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
