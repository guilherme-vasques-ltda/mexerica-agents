// D171 Clerk Opus2 F1/F2/F4: serviço e armazenamento reais; somente Clerk/Cloud e cifra são fakes locais.
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
import type { PasswordAttemptStore } from '../main/fork-account/password-attempt.ts'
import type { AccountStorage } from '../main/fork-account/storage.ts'
import { deferred, settle, loadMainModule } from './test-support/main-module.ts'

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
      const url = new URL(`${specifier}.ts`, context.parentURL)
      if (existsSync(url)) return nextResolve(url.href, context)
    }
    return nextResolve(specifier, context)
  },
})
const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: 'https://issuer.example.test', cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const binding = { version: 1, userId: 'user-a', sessionId: 'session-a', origin: config.cloudURL, issuer: config.clerkIssuer, deploymentId: config.deploymentId, manifestHash: config.manifestHash, deviceId: 'dev_a', deviceKey: 'a'.repeat(43) }
type Clerk = { offline: boolean; sessions: 'active' | 'ended' | 'denied' | 'changed'; calls: string[] }
function world(dir: string, remoteService: Clerk, hooks: { beforeClient?: () => Promise<void>; beforeRead?: () => Promise<void>; available?: () => boolean; decrypt?: (b: Buffer) => string; rejectQuarantineGuard?: boolean; beforeGuardRename?: (value: { state?: string; hash?: string }) => Promise<void>; onBrowser?: () => void; route?: (url: URL, method: string, credential?: string) => unknown } = {}) {
  const fetch = async (url: URL, init: { method: string; headers?: Record<string, string> }) => {
    remoteService.calls.push(`${init.method} ${url.pathname}`)
    const routed = hooks.route?.(url, init.method, init.headers?.Authorization)
    if (routed !== undefined) return routed
    if (remoteService.offline) throw new Error('offline')
    if (url.pathname === '/v1/client') {
      await hooks.beforeClient?.()
      if (remoteService.sessions === 'denied') return { ok: false, status: 401, headers: new Headers(), json: async () => ({ errors: [{ message: 'fixture unauthenticated' }] }) }
      if (remoteService.sessions === 'changed') return { ok: true, status: 200, headers: new Headers(), json: async () => ({ response: { sessions: [{ id: 'session-b', status: 'active', user: { id: 'user-b' } }], last_active_session_id: 'session-b' } }) }
      const sessions = [{ id: 'session-a', status: remoteService.sessions === 'active' ? 'active' : 'expired', user: { id: 'user-a' } }]
      return { ok: true, status: 200, headers: new Headers({ authorization: init.headers?.Authorization ?? 'Bearer client-a' }), json: async () => ({ response: { sessions, last_active_session_id: remoteService.sessions === 'active' ? 'session-a' : null } }) }
    }
    if (init.method === 'POST' && url.pathname === '/v1/client/sessions/session-a/tokens') return { ok: true, status: 200, json: async () => ({ response: { jwt: 'fixture-token' } }) }
    if (init.method === 'POST' && url.pathname === '/v1/client/sessions/session-a/remove') return { ok: true, status: 200, json: async () => ({ response: { id: 'session-a', status: 'removed' } }) }
    throw new Error(`unexpected ${init.method} ${url.pathname}`)
  }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': { ...fs, promises: { ...fs.promises, readFile: async (...args: Parameters<typeof fs.promises.readFile>) => { if (String(args[0]) === path.join(dir, 'account.guard')) await hooks.beforeRead?.(); return fs.promises.readFile(...args) }, rename: async (source: string, target: string) => {
      if (target === path.join(dir, 'account.guard')) {
        const candidate = JSON.parse(await fs.promises.readFile(source, 'utf8')) as { state?: string; hash?: string }
        if (hooks.rejectQuarantineGuard && candidate.state === 'revocation_unknown' && candidate.hash) throw new Error('fixture quarantine publication failure')
        await hooks.beforeGuardRename?.(candidate)
      }
      await fs.promises.rename(source, target)
    } } }, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => hooks.available?.() ?? true, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => hooks.decrypt?.(b) ?? b.toString() } },
    './auth': { authenticate: async () => { hooks.onBrowser?.(); assert.fail('no browser in this proof') }, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, Headers, process: { platform: 'darwin' }, fetch })
  return () => { const account = new mod.AccountService(); account.setBindingRestorer(async () => true); account.setReconciliationKeyVerifier(async () => true); return account }
}
async function seed(make: () => unknown) {
  await (make() as { persist(token: string, epoch: number, unknown: boolean, binding: unknown): Promise<void> }).persist('Bearer client-a', 0, false, binding)
}
const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()
const passwordReady = { id: 'attempt-b', status: 'needs_first_factor', identifier: 'fixture@example.test', created_session_id: null }
const passwordComplete = { id: 'attempt-b', status: 'complete', created_session_id: 'session-b' }
function passwordRoute(complete: boolean, terminal: boolean) {
  return (url: URL, method: string) => {
    let value: unknown
    if (url.pathname === '/v1/client') value = { sessions: [{ id: 'session-b', status: terminal ? 'ended' : 'active', user: { id: 'user-b' } }], last_active_session_id: terminal ? null : 'session-b' }
    else if (url.pathname.endsWith('/attempt_first_factor')) value = passwordComplete
    else if (url.pathname === '/v1/client/sign_ins' || url.pathname === '/v1/client/sign_ins/attempt-b') value = complete ? passwordComplete : passwordReady
    else return undefined
    return { ok: true, status: 200, headers: new Headers(method === 'POST' && url.pathname === '/v1/client/sign_ins' ? { authorization: 'Bearer client-b' } : {}), json: async () => ({ response: value }) }
  }
}

const internal = (s: unknown) => s as { storage: AccountStorage; passwordStore: PasswordAttemptStore }
const journal = (phase: 'starting' | 'ready' | 'sent' | 'rejected' = 'sent') => ({ version: 2 as const, scope: 'normal' as const, owner: null, phase, factorSent: phase === 'sent' || phase === 'rejected', exitRequested: false, credential: phase === 'starting' ? null : 'Bearer client-a', id: phase === 'starting' ? null : 'attempt-a' })
const cleared = async (s: unknown) => {
  assert.equal((await internal(s).storage.load()).kind, 'empty')
  assert.equal(await internal(s).passwordStore.load(), null)
}

test('MX82: só revogação explícita do servidor expira; âncora inválida e saída incerta bloqueiam', async () => {
  for (const kind of ['ended', 'denied', 'changed', 'anchor', 'revocation', 'exit', 'journal'] as const) {
    const dir = mkdtempSync(path.join(tmpdir(), 'account-expire-'))
    const remote = deferred<unknown>(); const started = deferred(); let held = false
    const remoteService: Clerk = { offline: false, sessions: ['ended', 'denied', 'changed'].includes(kind) ? kind as Clerk['sessions'] : 'active', calls: [] }
    const make = world(dir, remoteService, { route: (url, method) => {
      if (method === 'POST' && url.pathname.endsWith('/remove')) { held = true; started.resolve(); return remote.promise }
    } })
    try {
      await seed(make); const store = internal(make()).storage
      const guard = JSON.parse(fs.readFileSync(path.join(dir, 'account.guard'), 'utf8')) as { file: string }
      if (kind === 'anchor') await store.retain(Buffer.from(JSON.stringify({ version: 1, token: 'Bearer client-a', principal: {} })), 'active', () => {})
      if (kind === 'revocation') await store.quarantine(Buffer.from(JSON.stringify({ version: 1, credential: 'Bearer client-a', sessionId: 'session-a', userId: 'user-a' })), () => {})
      if (kind === 'exit') await store.fence(true)
      if (kind === 'journal') { await store.clear(); await internal(make()).passwordStore.save(journal(), () => {}) }
      const account = make(); await restore(account)
      assert.equal(account.state.user, null); assert.equal(account.captureCloudSession(), null)
      if (kind === 'ended') {
        assert.equal(account.state.error, 'Sua sessão expirou. Entre novamente.')
        assert.equal(account.reconciliationState().kind, 'none'); await cleared(account)
        assert.equal(existsSync(path.join(dir, guard.file)), false)
        assert.equal(existsSync(path.join(dir, 'account.guard')), false)
      } else {
        assert.equal(account.reconciliationState().kind, 'session_unverified')
        assert.equal(held, false)
      }
    } finally { remote.resolve({ ok: false, status: 503 }); await settle(); rmSync(dir, { recursive: true, force: true }) }
  }
})

test('D185/F10: transitório oferece retry; verificar A não revoga o journal residual do mesmo login', async () => {
  for (const issue of ['network', 'keychain', 'decrypt'] as const) {
    const dir = mkdtempSync(path.join(tmpdir(), 'account-retry-')); let unavailable = false
    const remoteService: Clerk = { offline: issue === 'network', sessions: 'active', calls: [] }
    const make = world(dir, remoteService, { available: () => issue !== 'keychain' || !unavailable, decrypt: b => { if (issue === 'decrypt' && unavailable) throw new Error('fixture unavailable'); return b.toString() } })
    try {
      await seed(make); await internal(make()).passwordStore.save(journal(), () => {})
      unavailable = true
      const account = make(); await restore(account)
      assert.equal(account.state.error, issue === 'network' ? 'Não foi possível verificar sua sessão agora' : issue === 'keychain' ? 'keychain.unavailable' : 'keychain.mismatch')
      assert.equal(account.reconciliationState().kind, 'session_unverified')
      assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), 'error')
      unavailable = false; remoteService.offline = false
      assert.equal((await account.reconcile('check')).kind, 'none')
      assert.equal(account.state.user?.userId, 'user-a'); assert.ok(account.captureCloudSession())
      assert.equal(remoteService.calls.some(c => c.startsWith('POST') || c.startsWith('DELETE')), false, 'retry não encerra A pelo alias do journal')
      assert.equal(await internal(account).passwordStore.load(), null)
    } finally { rmSync(dir, { recursive: true, force: true }) }
  }
})

test('D185: Entrar novamente limpa sessão transitória e reinício não restaura nada', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-relogin-'))
  const remoteService: Clerk = { offline: true, sessions: 'active', calls: [] }; const make = world(dir, remoteService)
  try {
    await seed(make); const account = make(); await restore(account); await account.signOut()
    await cleared(account); assert.equal(account.reconciliationState().kind, 'none')
    const next = make(); await restore(next); assert.equal(next.state.user, null); assert.equal(next.state.error, null)
  } finally { await settle(); rmSync(dir, { recursive: true, force: true }) }
})

test('D185/F11: Sair durante leitura de restore nunca deixa a continuação publicar nem reaparecer no reinício', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-exit-restore-'))
  const reading = deferred(); const release = deferred(); let paused = false; let hold = false
  const remoteService: Clerk = { offline: true, sessions: 'active', calls: [] }
  const make = world(dir, remoteService, { beforeRead: async () => { if (hold && !paused) { paused = true; reading.resolve(); await release.promise } } })
  try {
    await seed(make); hold = true
    const account = make(); const work = restore(account); await reading.promise
    const epoch = account.sessionEpoch; const exiting = account.signOut()
    assert.equal(account.sessionEpoch, epoch + 1); assert.equal(account.state.user, null)
    await exiting; release.resolve(); await work; await cleared(account)
    const next = make(); await restore(next); assert.equal(next.state.user, null); assert.equal(next.reconciliationState().kind, 'none')
  } finally { release.resolve(); await settle(); rmSync(dir, { recursive: true, force: true }) }
})

test('D185/F11: Sair durante fator limpa journal, invalida resposta e tenta encerrar somente a tentativa capturada', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-exit-factor-')); const factor = deferred(); const response = deferred<unknown>(); const ended = deferred(); let complete = false
  const remoteService: Clerk = { offline: false, sessions: 'active', calls: [] }
  const make = world(dir, remoteService, { route: (url, method) => {
    const reply = (value: unknown, credential = '') => ({ ok: true, status: 200, headers: new Headers(credential ? { authorization: credential } : {}), json: async () => ({ response: value }) })
    if (url.pathname.endsWith('/attempt_first_factor')) { factor.resolve(); return response.promise }
    if (url.pathname === '/v1/client/sign_ins') return reply(passwordReady, 'Bearer client-b')
    if (url.pathname === '/v1/client/sign_ins/attempt-b') return reply(complete ? passwordComplete : passwordReady)
    if (method === 'POST' && url.pathname === '/v1/client/sessions/session-b/remove') { ended.resolve(); return reply({ id: 'session-b', status: 'removed' }) }
  } })
  try {
    const account = make(); await restore(account)
    const work = account.signInWithPassword('fixture@example.test', 'fixture-password'); await factor.promise
    await account.signOut(); await cleared(account); assert.equal(account.state.user, null)
    complete = true; response.resolve({ ok: true, status: 200, headers: new Headers(), json: async () => ({ response: passwordComplete }) })
    assert.equal(await work, 'error'); await ended.promise
    assert.equal(account.state.user, null); assert.equal(account.captureCloudSession(), null); await cleared(account)
    const next = make(); await restore(next); assert.equal(next.state.user, null)
  } finally { response.resolve({ ok: false, headers: new Headers() }); await settle(); rmSync(dir, { recursive: true, force: true }) }
})

test('D185/F9: confirmações históricas sem sessão não bloqueiam nova entrada normal por senha', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-history-'))
  const remoteService: Clerk = { offline: false, sessions: 'active', calls: [] }
  const make = world(dir, remoteService, { route: passwordRoute(false, false) })
  try {
    const account = make()
    const userId = () => account.state.user?.userId
    await internal(account).storage.retain(Buffer.from(JSON.stringify({ version: 3, token: null, principal: null, binding: null, orphans: [{ version: 1, credential: 'old-client', sessionId: 'old-session', userId: 'old-owner', restricted: { credential: 'old-confirmation', sessionId: 'old-confirmation-session', verified: true } }] })), 'active', () => {})
    await restore(account)
    assert.equal(account.reconciliationState().kind, 'none'); assert.equal(account.state.user, null)
    assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), null)
    assert.equal(userId(), 'user-b'); assert.ok(account.captureCloudSession())
  } finally { await settle(); rmSync(dir, { recursive: true, force: true }) }
})

test('D185/F12: reset solicita encerrar todas as sessões do cliente, sem aguardar a rede', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-reset-all-')); const sent = deferred(); const response = deferred<unknown>(); let newEntry = false
  const remoteService: Clerk = { offline: false, sessions: 'active', calls: [] }
  const make = world(dir, remoteService, { route: (url, method) => {
    if (newEntry && method !== 'DELETE') return passwordRoute(false, false)(url, method)
    if (url.pathname === '/v1/client' && method === 'GET') return { ok: true, status: 200, headers: new Headers(), json: async () => ({ response: { sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }, { id: 'second-session', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' } }) }
    if (url.pathname === '/v1/client/sessions' && method === 'DELETE') { sent.resolve(); return response.promise }
  } })
  try {
    await seed(make); const account = make(); const userId = () => account.state.user?.userId; await restore(account); await account.signOutAll()
    await cleared(account); assert.equal(account.state.user, null); await sent.promise
    assert.ok(remoteService.calls.includes('DELETE /v1/client/sessions'))
    assert.equal(remoteService.calls.some(c => c.endsWith('/remove')), false)
    newEntry = true
    assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), null)
    const guard = fs.readFileSync(path.join(dir, 'account.guard'))
    response.resolve({ ok: true, status: 200, headers: new Headers({ authorization: 'old-client-rotation' }) }); await settle()
    assert.equal(userId(), 'user-b')
    assert.deepEqual(fs.readFileSync(path.join(dir, 'account.guard')), guard, 'resposta remota antiga não publica nem limpa B')
  } finally { response.resolve({ ok: false, status: 503 }); await settle(); rmSync(dir, { recursive: true, force: true }) }
})

test('MX82: cifra indisponível após Tentar de novo preserva a sessão e permite recuperação', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-keychain-retry-')); let available = true
  const remoteService: Clerk = { offline: true, sessions: 'active', calls: [] }
  const make = world(dir, remoteService, { available: () => available })
  try {
    await seed(make); available = false
    const account = make(); await restore(account)
    assert.equal(account.reconciliationState().kind, 'session_unverified')
    const before = fs.readFileSync(path.join(dir, 'account.guard'))
    assert.equal((await account.reconcile('check')).kind, 'session_unverified')
    assert.equal(account.state.error, 'keychain.unavailable')
    assert.deepEqual(fs.readFileSync(path.join(dir, 'account.guard')), before)
    assert.equal(remoteService.calls.length, 0)
    available = true; remoteService.offline = false
    assert.equal((await account.reconcile('check')).kind, 'none')
    assert.equal(account.state.user?.userId, 'user-a')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

// Cipher failures are neither an expired login nor permission to reset the identity.
test('MX82: conta e tentativa ilegíveis conservam todos os bytes em retries e reinícios', async () => {
  for (const family of ['account', 'password'] as const) for (const signal of ['unavailable', 'exception', 'malformed'] as const) {
    const dir = mkdtempSync(path.join(tmpdir(), 'mx82-account-'))
    let fail = false, resets = 0
    const remoteService: Clerk = { offline: false, sessions: 'active', calls: [] }
    const make = world(dir, remoteService, { available: () => !(fail && signal === 'unavailable'), decrypt: b => {
      if (fail && (family === 'account' ? b.toString().includes('"token"') : b.toString().includes('"scope"'))) {
        if (signal === 'exception') throw new Error('fixture cipher denied')
        if (signal === 'malformed') return '{"version":1,"token":42}'
      }
      return b.toString()
    } })
    const files = (root: string): Record<string, string> => Object.fromEntries(fs.readdirSync(root, { recursive: true, withFileTypes: true }).filter(e => e.isFile()).map(e => {
      const file = path.join(e.parentPath, e.name)
      return [path.relative(root, file), fs.readFileSync(file).toString('hex')]
    }))
    try {
      await seed(make)
      await internal(make()).passwordStore.save(journal(), () => {})
      const before = files(dir); fail = true
      for (let restart = 0; restart < 2; restart++) {
        const account = make()
        account.setDeviceIdentityResetter(async () => { resets++ })
        await restore(account)
        for (let retry = 0; retry < 2; retry++) assert.equal((await account.reconcile('check')).kind, 'session_unverified')
        assert.equal(account.captureCloudSession(), null)
        assert.deepEqual(files(dir), before)
        await account.signOut()
        await account.signOut()
        assert.deepEqual(files(dir), before, 'Sair também preserva ciphertext que não decifra')
      }
      assert.equal(resets, 0); assert.equal(remoteService.calls.length, 0)
      fail = false
      const recovered = make(); await restore(recovered)
      assert.equal(recovered.state.user?.userId, 'user-a')
    } finally { rmSync(dir, { recursive: true, force: true }) }
  }
})

for (const store of ['account', 'password-attempt'] as const) for (const scenario of ['namespace', 'no-hash', 'no-guard', 'corrupt'] as const) test(`Sair preserva registro sem vínculo e permite entrar: ${store}/${scenario}`, async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'account-unbound-'))
  const remote: Clerk = { offline: false, sessions: 'active', calls: [] }
  const make = world(dir, remote, { route: passwordRoute(false, false) })
  try {
    const initial = make()
    if (store === 'account') await seed(make)
    else await internal(initial).passwordStore.save(journal(), () => {})
    const folder = store === 'account' ? dir : path.join(dir, 'account-password-attempt')
    const guardPath = path.join(folder, 'account.guard')
    const guard = JSON.parse(fs.readFileSync(guardPath, 'utf8'))
    if (scenario === 'namespace') guard.namespace = 'different-deployment'
    if (scenario === 'no-hash') { guard.state = 'revocation_unknown'; delete guard.hash }
    if (scenario === 'no-guard') fs.unlinkSync(guardPath)
    else fs.writeFileSync(guardPath, scenario === 'corrupt' ? '{' : JSON.stringify(guard))
    const tmp = `account.bin.${'a'.repeat(32)}.tmp`
    fs.writeFileSync(path.join(folder, tmp), 'synthetic interrupted write')
    const before = Object.fromEntries(fs.readdirSync(folder).filter(n => n.startsWith('account') && fs.statSync(path.join(folder, n)).isFile()).map(n => [n, fs.readFileSync(path.join(folder, n))]))
    const account = make(); await restore(account)
    assert.equal(account.reconciliationState().kind, 'session_unverified')
    assert.equal(remote.calls.length, 0)
    // Automatic cleanup cannot treat an unbound record as consent to archive.
    await assert.rejects(store === 'account' ? internal(account).storage.clear() : internal(account).passwordStore.clear(() => {}))
    for (const [name, bytes] of Object.entries(before)) assert.deepEqual(fs.readFileSync(path.join(folder, name)), bytes)
    // A failed automatic reset followed by Check is still not an explicit exit.
    await (account as unknown as { resetLocal(message: null): Promise<void> }).resetLocal(null)
    await account.reconcile('check')
    for (const [name, bytes] of Object.entries(before)) assert.deepEqual(fs.readFileSync(path.join(folder, name)), bytes)
    await account.signOut()
    assert.equal(account.reconciliationState().kind, 'none')
    const archive = fs.readdirSync(folder).find(n => n.startsWith('account-quarantine-'))!
    assert.ok(archive)
    for (const [name, bytes] of Object.entries(before)) assert.deepEqual(fs.readFileSync(path.join(folder, archive, name)), bytes)
    await cleared(account)
    assert.equal(await account.signInWithPassword('fixture@example.test', 'synthetic password'), null)
    assert.equal(account.state.user?.userId, 'user-b')
    for (const [name, bytes] of Object.entries(before)) assert.deepEqual(fs.readFileSync(path.join(folder, archive, name)), bytes)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
