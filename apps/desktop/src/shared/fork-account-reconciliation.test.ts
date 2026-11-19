import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

function fixture() {
  const host = 'identity.example.test'
  const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: 'https://issuer.example.test', cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
  let saved: { kind: 'active' | 'revocation_unknown'; sealed: Buffer; recoverable?: boolean; exitRequested?: true } | null = null
  let revoked = false
  let receiptProved = true
  let devicePresent = true
  const calls: string[] = []
  let failClear = false
  let failSave = false
  let failRetain = false
  let failRetainAt = 0
  let retainCalls = 0
  let trackRemoved = false
  let restrictedRemoved = false
  let rotated = false
  let offline = false
  let user = 'user-a'
  let restrictedUser = 'user-a'
  let expired = false
  let callback = ''
  let callbackOverride: string | null = null
  let cleanupFails = false
  let ambiguousRemove = false
  let cancelledBrowser = false
  let pendingFirstFactor = false
  let failPrepare = false
  let attemptGone = false
  let attemptAbandoned = false
  let rejectStart = false
  const diagnostics: unknown[][] = []
  class Storage {
    private readonly dir: string
    constructor(dir: string) { this.dir = dir }
    async hasArchives() { return false }
    async load() { if (this.dir.endsWith('/account-password-attempt')) return { kind: 'empty' }; return saved ?? { kind: 'empty' } }
    async save(data: Buffer, current: () => void) { current(); if (failSave) throw new Error('storage'); saved = { kind: 'active', sealed: data } }
    async quarantine(data: Buffer, current: () => void) { current(); saved = { kind: 'revocation_unknown', sealed: data } }
    async confirmEmpty(current: () => void) { current(); if (failClear || saved) throw new Error('not empty') }
    async fence(exitRequested = false) { if (saved) saved = { ...saved, kind: 'revocation_unknown', recoverable: saved.kind === 'active' || saved.recoverable, ...(exitRequested ? { exitRequested: true as const } : {}) } }
    async resumeFence() { if (!saved?.recoverable || saved.exitRequested) throw new Error('not recoverable'); saved = { kind: 'active', sealed: saved.sealed } }
    // failSave models active-session publication; failRetain covers either state.
    async retain(data: Buffer, state: 'active' | 'revocation_unknown', current: () => void) { current(); retainCalls++; if ((state === 'active' && failSave) || failRetain || (failRetainAt && retainCalls === failRetainAt)) throw new Error('retain failure'); saved = { kind: state, sealed: data } }
    async clear(state: string) { if (failClear && state !== 'revocation_unknown') throw new Error('storage'); saved = state === 'revocation_unknown' ? { kind: 'revocation_unknown', sealed: Buffer.from('{}') } : null }
    async block(state?: string) { saved = state === 'revocation_unknown' ? { kind: 'revocation_unknown', sealed: Buffer.from('{}') } : null }
  }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
    './auth': { authenticate: async () => { if (cancelledBrowser) throw new Error('cancelled'); pendingFirstFactor = false; return callbackOverride ?? callback }, AuthenticationEnded: class extends Error {} }, './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL }, './fork-account/storage': { AccountStorage: Storage },
  }, { URL, URLSearchParams, Buffer, console: { warn: (...args: unknown[]) => diagnostics.push(args) }, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string; body?: string; headers?: Record<string, string> }) => {
    calls.push(`${init.method} ${url.pathname}`)
    if (offline) throw new Error('offline')
    if (url.pathname === '/v1/client/sign_ins' && init.method === 'POST') {
      if (rejectStart) return { ok: false, status: 400, headers: new Headers(), json: async () => ({ errors: [{ long_message: 'private fixture URL https://secret.example.test/?credential=never-print' }] }) }
      callback = new URLSearchParams(init.body).get('redirect_url')!
      return { ok: true, headers: new Headers({ authorization: 'Bearer fixture-restricted' }), json: async () => ({ response: { id: 'signin-a', status: 'needs_first_factor' } }) }
    }
    if (url.pathname.endsWith('/prepare_first_factor')) { if (failPrepare) throw new Error('prepare failed'); callback = new URLSearchParams(init.body).get('redirect_url')!; return { ok: true, headers: new Headers(), json: async () => ({ response: { id: 'signin-a', status: 'needs_first_factor', first_factor_verification: { external_verification_redirect_url: 'https://login.example.test/' } } }) } }
    if (url.pathname === '/v1/client/sign_ins/signin-a' && attemptAbandoned) return { ok: true, headers: new Headers(), json: async () => ({ response: { id: 'signin-a', status: 'abandoned' } }) }
    if (url.pathname === '/v1/client/sign_ins/signin-a' && attemptGone) return { ok: false, status: 404, headers: new Headers(), json: async () => ({}) }
    if (url.pathname === '/v1/client/sign_ins/signin-a') return { ok: true, headers: new Headers(), json: async () => ({ response: pendingFirstFactor ? { id: 'signin-a', status: 'needs_first_factor' } : { id: 'signin-a', status: 'complete', created_session_id: 'restricted-session' } }) }
    if (url.pathname.endsWith('/tokens') && expired && init.headers?.Authorization !== 'Bearer fixture-restricted') return { ok: false, headers: new Headers(), json: async () => ({}) }
    if (url.pathname.endsWith('/tokens')) return { ok: true, headers: new Headers(rotated ? { authorization: `Bearer ${randomBytes(16).toString('hex')}` } : {}), json: async () => ({ response: { jwt: 'fixture-jwt' } }) }
    if (url.pathname === '/v1/me') return { ok: true, json: async () => ({ userId: user }) }
    if (url.pathname === '/v1/devices' && init.method === 'GET') return { ok: true, json: async () => ({ devices: devicePresent ? [{ deviceId: 'dev_a', deviceKey: 'a'.repeat(43), revoked }] : [] }) }
    if (url.pathname === '/v1/devices/dev_a' && init.method === 'DELETE') { revoked = true; return { ok: true } }
    if (url.pathname.endsWith('/remove')) { if (trackRemoved && url.pathname.includes('restricted-session')) restrictedRemoved = true; return { ok: !cleanupFails || !url.pathname.includes('restricted-session'), json: async () => ambiguousRemove ? {} : ({ response: { id: url.pathname.split('/').at(-2), status: 'removed' } }) } }
    if (url.pathname === '/v1/client') { const restricted = init.headers?.Authorization === 'Bearer fixture-restricted'; const id = restricted ? 'restricted-session' : 'session-a'; return { ok: true, headers: new Headers(), json: async () => ({ response: { sessions: [{ id, status: restricted && restrictedRemoved ? 'removed' : 'active', user: { id: restricted ? restrictedUser : 'user-a' } }], last_active_session_id: id } }) } }
    throw new Error('unexpected')
  } })
  const binding = { version: 1, userId: 'user-a', sessionId: 'session-a', origin: config.cloudURL, issuer: config.clerkIssuer, deploymentId: config.deploymentId, manifestHash: config.manifestHash, deviceId: 'dev_a', deviceKey: 'a'.repeat(43) }
  const make = () => {
    const service = new mod.AccountService()
    service.state = { ...service.state, socialProviders: ['google'] }
    service.setDeviceIdentityRetirer(async (_binding, current) => current())
    // This suite isolates AccountService; payload validation is covered in the factory suite.
    service.setDeviceRevocationVerifier(async (_binding, _jwt, current) => ({ kind: current() && revoked && receiptProved ? 'revoked' : 'unknown' }))
    return service
  }
  const install = (service: InstanceType<typeof mod.AccountService>) => {
    const internal = service as unknown as { deviceToken: string; deviceBinding: unknown; applyClient(client: unknown): void; state: { ready: boolean } }
    internal.state.ready = true
    internal.deviceToken = `Bearer ${randomBytes(16).toString('hex')}`
    internal.deviceBinding = binding
    internal.applyClient({ sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' })
  }
  return { make, install, calls, binding, diagnostics, setRejectStart: () => { rejectStart = true }, setReceiptProved: (v: boolean) => { receiptProved = v }, setCallbackOverride: (v: string | null) => { callbackOverride = v }, failRetainOn: (n: number) => { retainCalls = 0; failRetainAt = n }, trackRemoved: () => { trackRemoved = true }, setFailRetain: (v: boolean) => { failRetain = v }, setAttemptGone: (v: boolean) => { attemptGone = v }, setAttemptAbandoned: (v: boolean) => { attemptAbandoned = v }, setPendingFirstFactor: (v: boolean) => { pendingFirstFactor = v }, setFailPrepare: (v: boolean) => { failPrepare = v }, setOffline: (v: boolean) => { offline = v }, setFailSave: (v: boolean) => { failSave = v }, setFailClear: (v: boolean) => { failClear = v }, setRotated: (v: boolean) => { rotated = v }, setUser: (v: string) => { user = v }, setRestrictedUser: (v: string) => { restrictedUser = v }, setCleanupFails: (v: boolean) => { cleanupFails = v }, setAmbiguousRemove: (v: boolean) => { ambiguousRemove = v }, setCancelledBrowser: (v: boolean) => { cancelledBrowser = v }, setExpired: (v: boolean) => { expired = v }, setDevicePresent: (v: boolean) => { devicePresent = v }, setSaved: (v: typeof saved) => { saved = v }, getSaved: () => saved }
}

test('restore offline preserva credencial e recusa login; saída vazia não cria quarentena', async () => {
  const f = fixture(); const first = f.make(); f.install(first)
  const internal = first as unknown as { deviceToken: string }
  f.setSaved({ kind: 'active', sealed: Buffer.from(JSON.stringify({ version: 1, token: internal.deviceToken, binding: f.binding })) })
  f.setOffline(true)
  const next = f.make(); await (next as unknown as { restore(): Promise<void> }).restore()
  assert.equal(await next.signIn('google'), 'error')
  assert.equal(f.getSaved()?.kind, 'active')
  const empty = f.make(); await empty.signOut()
  assert.equal(empty.reconciliationState().kind, 'none')
})

test('falha de armazenamento mantém quarentena em memória e não deixa busy preso', async () => {
  const f = fixture(); const account = f.make(); f.install(account); f.setFailSave(true); f.setRotated(true)
  account.state.busy = true
  await assert.rejects(account.sessionToken())
  assert.equal(account.state.busy, false)
  assert.ok(f.getSaved() === null || f.getSaved()?.kind === 'active')
  assert.equal(await account.signIn('google'), 'error')
})


test('falha de gravação no início do login encerra busy e conserva bloqueio', async () => {
  const f = fixture(); const account = f.make(); account.state.ready = true; f.setFailClear(true)
  assert.equal(await account.signIn('google'), 'error')
  assert.equal(account.state.busy, false)
  assert.equal(f.getSaved(), null)
  assert.equal(account.reconciliationState().kind, 'session_unverified')
  assert.equal((await account.reconcile('check')).kind, 'none', 'store comprovadamente vazio não precisa manter a antiga quarentena')
  f.setFailClear(false)
  assert.equal((await account.reconcile('check')).kind, 'none')
})

test('falha ao salvar vínculo fecha sessão e conserva contexto para reconciliação', async () => {
  const f = fixture(); const account = f.make(); f.install(account); f.setFailSave(true)
  const seal = account.captureCloudSession()!
  await assert.rejects(account.saveDeviceBinding(seal, 'dev_a', 'a'.repeat(43)))
  assert.equal(account.isCloudSessionCurrent(seal), false)
  assert.equal(account.state.user, null)
  assert.ok(f.getSaved() === null || f.getSaved()?.kind === 'active')
  assert.equal(await account.signIn('google'), 'error')
})

test('callback de outra tentativa não consome nonce nem confirma sessão', async () => {
  const f = fixture(); const account = f.make(); account.state.ready = true
  f.setCallbackOverride('app.fixture://callback?attempt=outra-tentativa&rotating_token_nonce=fixture')
  assert.equal(await account.signIn('google'), 'error')
  assert.equal(account.state.user, null)
  assert.equal(account.reconciliationState().kind, 'session_unverified')
  assert.equal(f.calls.includes('GET /v1/client/sign_ins/signin-a'), false)
  assert.equal(f.calls.includes('GET /v1/client'), false)
})

test('falha de entrada informa só etapa e HTTP, não resposta privada, e não libera nem repete', async () => {
  const f = fixture(); f.setRejectStart()
  const account = f.make()
  account.state.ready = true
  assert.equal(await account.signIn('google'), 'error')
  assert.equal(account.state.user, null)
  assert.equal(account.captureCloudSession(), null)
  assert.deepEqual(f.calls, ['POST /v1/client/sign_ins'])
  assert.equal(JSON.stringify(f.diagnostics), JSON.stringify([['account:sign-in-failed', { stage: 'create', status: 400, reason: 'clerk_rejected' }]]))
  assert.equal(account.state.error, 'Não foi possível confirmar a entrada na conta. Tente de novo: a Mexerica só abre com a conta confirmada.')
  assert.ok(!JSON.stringify([f.diagnostics, account.state]).includes('never-print'))
})
