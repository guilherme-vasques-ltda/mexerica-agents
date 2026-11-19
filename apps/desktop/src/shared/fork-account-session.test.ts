import assert from 'node:assert/strict'
import test from 'node:test'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { deferred, loadMainModule } from './test-support/main-module.ts'

// All I/O is replaced by local fakes; the fetch fake has no network implementation.
test('login só apresenta a sessão concluída e confirmada pelo cliente e armazenamento', { timeout: 10_000 }, async () => {
  for (const failFinalWrite of [false, true]) {
    let callback = ''
    let count = 0
    const intermediatePrincipals: unknown[] = []
    const storing = deferred(); const release = deferred()
    const host = 'identity.example.test'
    const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
    const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
      'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
      electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s) } },
      './auth': { authenticate: async () => callback, AuthenticationEnded: class extends Error {} },
      './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
      '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } },
      './fork-account/config': { accountConfig, accountBrowserURL },
      './fork-account/storage': { AccountStorage: class {
        async hasArchives() { return false }
    async load() { return { kind: 'empty' } }
        async clear() {}
        async retain(data: Buffer, _state: string, current: () => void) {
          current()
          const principal = JSON.parse(data.toString()).principal
          if (principal?.sessionId === 'session-new') {
            storing.resolve(); await release.promise
            if (failFinalWrite) throw new Error('fixture final secure write failed')
          } else intermediatePrincipals.push(principal)
          current()
        }
      } },
    }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async (_url: string, init: { body?: string }) => {
      count++
      if (count === 1) callback = new URLSearchParams(init.body).get('redirect_url')!
      const payloads = [
        { id: 'signin', status: 'needs_first_factor' },
        { id: 'signin', status: 'needs_first_factor', first_factor_verification: { external_verification_redirect_url: 'https://identity.example.test/oauth' } },
        { id: 'signin', status: 'complete', created_session_id: 'session-new' },
        { sessions: [{ id: 'session-new', status: 'active', user: { id: 'user-new' } }], last_active_session_id: 'session-new' },
      ]
      return { ok: true, headers: new Headers(count === 1 ? { authorization: 'Bearer fixture-native-client' } : {}), json: async () => ({ response: payloads[count - 1] }) }
    } })
    const service = new mod.AccountService()
    service.state = { ...service.state, socialProviders: ['google'] }
    await (service as unknown as { restore(): Promise<void> }).restore()
    ;(service as unknown as { unverifiedPrincipal: unknown }).unverifiedPrincipal = { userId: 'old-user', sessionId: 'old-session' }
    const projectedUser = () => service.state.user
    const pending = service.signIn('google')
    try {
      await storing.promise
      assert.equal(projectedUser(), null)
      assert.equal(service.captureCloudSession(), null)
    } finally { release.resolve() }
    assert.equal(await pending, failFinalWrite ? 'error' : null)
    assert.equal(projectedUser()?.userId ?? null, failFinalWrite ? null : 'user-new')
    assert.equal(!!service.captureCloudSession(), !failFinalWrite)
    assert.equal(service.reconciliationState().kind, failFinalWrite ? 'session_unverified' : 'none')
    assert.equal(count, 4)
    assert.equal(JSON.stringify(intermediatePrincipals), JSON.stringify([null]), 'a new credential must not inherit the old account principal')
  }
})

test('reinício restaura vínculo somente com a sessão e configuração correspondentes', async () => {
  const host = 'identity.example.test'
  const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: 'https://issuer.example.test', cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
  let stored: Buffer | null = null
  class FakeStorage {
    private readonly dir: string
    constructor(dir: string) { this.dir = dir }
    async hasArchives() { return false }
    async load() { if (this.dir.endsWith('/account-password-attempt')) return { kind: 'empty' }; return stored ? { kind: 'active', sealed: stored } : { kind: 'empty' } }
    async save(data: Buffer, current: () => void) { current(); stored = data; current() }
    async retain(data: Buffer, _state: string, current: () => void) { current(); stored = data; current() }
    async clear() { stored = null }
    async block() {}
  }
  const key = randomBytes(32).toString('base64url')
  let restored = 0
  const make = (input: typeof config) => loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value), decryptString: (value: Buffer) => value.toString() } },
    './auth': {}, './environment': { identity: { appId: 'fixture', releaseUpdates: false }, buildConfig: input },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL }, './fork-account/storage': { AccountStorage: FakeStorage },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async () => ({ ok: true, headers: new Headers(), json: async () => ({ response: { sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' } }) }) })
  const first = new (make(config).AccountService)()
  // Boot must finish before capturing a normal session, as it does in the app.
  await (first as unknown as { restore(): Promise<void> }).restore()
  const internal = first as unknown as { deviceToken: string; applyClient(client: unknown): void }
  internal.deviceToken = `Bearer ${randomBytes(16).toString('hex')}`
  internal.applyClient({ sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' })
  const seal = first.captureCloudSession()!
  assert.ok(await first.saveDeviceBinding(seal, 'dev_fixture', key))
  const second = new (make(config).AccountService)()
  second.setBindingRestorer(async (_seal, binding) => { restored++; assert.equal(binding.deviceKey, key); return true })
  await (second as unknown as { restore(): Promise<void> }).restore()
  assert.equal(restored, 1)
  const third = new (make({ ...config, clerkIssuer: 'https://other.example.test' }).AccountService)()
  third.setBindingRestorer(async () => { restored++; return true })
  await (third as unknown as { restore(): Promise<void> }).restore()
  assert.equal(restored, 1)
})
