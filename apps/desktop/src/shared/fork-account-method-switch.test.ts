import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import type { PasswordAttemptStore } from '../main/fork-account/password-attempt.ts'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
class AuthenticationEnded extends Error {
  readonly reason: 'cancelled' | 'timeout'
  constructor(reason: 'cancelled' | 'timeout') { super('fixture browser ended'); this.reason = reason }
}
function world(dir: string, cancellation?: 'cancelled' | 'timeout') {
  const attempts: { id: string; credential: string; password: boolean; reads: number; sessionId: string; callback: string }[] = []
  const posts: { strategy: string | null; authorization: string | undefined }[] = []
  const releaseOld = deferred(); const oldStarted = deferred(); const removed = deferred()
  let browser = 0; let factors = 0
  const reply = (response: unknown, authorization?: string, status = 200) => ({ ok: status === 200, status, headers: new Headers(authorization ? { authorization } : {}), json: async () => ({ response }) })
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
    './auth': { AuthenticationEnded, authenticate: async () => { browser++; if (cancellation) throw new AuthenticationEnded(cancellation); return attempts.at(-1)!.callback } },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, Headers, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string; body?: string; headers: Record<string, string> }) => {
    const body = new URLSearchParams(init.body)
    if (url.pathname === '/v1/client/sign_ins' && init.method === 'POST') {
      posts.push({ strategy: body.get('strategy'), authorization: init.headers.Authorization })
      const n = attempts.length + 1
      const attempt = { id: `attempt-${n}`, credential: `fixture-client-${n}`, password: body.has('identifier'), reads: 0, sessionId: `session-${n}`, callback: body.get('redirect_url') ?? '' }
      attempts.push(attempt); return reply({ id: attempt.id, status: 'needs_first_factor' }, attempt.credential)
    }
    const attempt = attempts.find(a => a.credential === init.headers.Authorization)!
    assert.ok(attempt, 'somente a credencial capturada pode ser usada')
    if (url.pathname.endsWith('/attempt_first_factor')) {
      factors++
      if (!cancellation) return { ok: false, status: 422, headers: new Headers(), json: async () => ({ errors: [{ code: 'form_password_incorrect' }] }) }
      return reply({ id: attempt.id, status: 'complete', created_session_id: attempt.sessionId })
    }
    if (url.pathname.endsWith('/prepare_first_factor')) return reply({ id: attempt.id, first_factor_verification: { external_verification_redirect_url: `https://${host}/oauth` } })
    if (url.pathname === `/v1/client/sign_ins/${attempt.id}`) {
      attempt.reads++
      if (attempt.password && !cancellation && attempt.reads === 1) return reply({ id: attempt.id, status: 'needs_first_factor', created_session_id: null })
      if (attempt.password && !cancellation) { oldStarted.resolve(); await releaseOld.promise }
      return reply({ id: attempt.id, status: 'complete', created_session_id: attempt.sessionId })
    }
    if (url.pathname === `/v1/client/sessions/${attempt.sessionId}/remove`) { assert.equal(attempt.id, 'attempt-1'); removed.resolve(); return reply({}, 'old-rotation') }
    if (url.pathname === '/v1/client') return reply({ sessions: [{ id: attempt.sessionId, status: 'active', user: { id: attempt.password ? 'password-user' : 'social-user' } }], last_active_session_id: attempt.sessionId })
    throw new Error('Unexpected fixture route')
  } })
  return { make: () => { const a = new mod.AccountService(); a.state = { ...a.state, socialProviders: ['apple', 'google', 'x'] }; return a }, posts, releaseOld, oldStarted, removed, browser: () => browser, factors: () => factors }
}
const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()

test('senha recusada → Google/X/Apple abre o fluxo sem reinício e limpeza antiga não altera a nova conta', async () => {
  for (const provider of ['google', 'x', 'apple'] as const) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'account-switch-social-')); const f = world(dir)
    try {
      const account = f.make(); await restore(account)
      assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), 'error')
      assert.match(account.state.error!, /escolha outro método/)
      assert.equal(await (account as unknown as { passwordStore: PasswordAttemptStore }).passwordStore.load(), null)
      await f.oldStarted.promise
      assert.equal(await account.signIn(provider), null, 'não aguarda a limpeza Clerk da tentativa antiga')
      assert.equal(f.browser(), 1); assert.equal(f.factors(), 1)
      assert.equal(f.posts[1].strategy, `oauth_${provider}`); assert.equal(f.posts[1].authorization, undefined)
      const userId = () => account.state.user?.userId
      assert.equal(userId(), 'social-user'); assert.equal(account.state.error, null)
      const seal = account.captureCloudSession()!; const guard = fs.readFileSync(path.join(dir, 'account.guard'))
      f.releaseOld.resolve(); await f.removed.promise; await settle()
      assert.equal(userId(), 'social-user'); assert.ok(account.isCloudSessionCurrent(seal))
      assert.deepEqual(fs.readFileSync(path.join(dir, 'account.guard')), guard)
    } finally { f.releaseOld.resolve(); await settle(); fs.rmSync(dir, { recursive: true, force: true }) }
  }
})

test('social cancelado ou com timeout → senha inicia tentativa nova sem herdar a credencial social', async () => {
  for (const reason of ['cancelled', 'timeout'] as const) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'account-switch-password-')); const f = world(dir, reason)
    try {
      const account = f.make(); await restore(account)
      assert.equal(await account.signIn('google'), reason)
      assert.equal(account.reconciliationState().kind, 'none'); assert.equal(account.state.busy, false)
      assert.match(account.state.error!, reason === 'cancelled' ? /Entrada cancelada/ : /tempo para entrar terminou/)
      assert.equal(fs.existsSync(path.join(dir, 'account.guard')), false)
      assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), null)
      assert.equal(f.posts[1].strategy, null); assert.equal(f.posts[1].authorization, undefined)
      assert.equal(f.browser(), 1); assert.equal(f.factors(), 1)
      assert.equal(account.state.user?.userId, 'password-user'); assert.equal(account.state.error, null)
      await f.removed.promise
    } finally { await settle(); fs.rmSync(dir, { recursive: true, force: true }) }
  }
})
