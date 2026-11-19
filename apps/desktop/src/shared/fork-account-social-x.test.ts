import assert from 'node:assert/strict'
import test from 'node:test'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

// Real AccountService; all transport, browser and system storage boundaries are fake.
test('Apple, X e Google usam a estratégia oficial e só publicam sessão confirmada', async () => {
  for (const provider of ['apple', 'x', 'google'] as const) {
  const host = 'identity.example.test'
  const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
  let callback = ''; const strategies: (string | null)[] = []; let calls = 0
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value) } },
    './auth': { authenticate: async () => callback, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class { async hasArchives() { return false }
    async load() { return { kind: 'empty' } } async clear() {} async retain(_bytes: Buffer, _state: string, current: () => void) { current() } } },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async (_url: string, init: { body?: string }) => {
    calls++
    const body = new URLSearchParams(init.body)
    if (calls <= 2) strategies.push(body.get('strategy'))
    if (calls === 1) callback = body.get('redirect_url')!
    const responses = [
      { id: 'attempt', status: 'needs_first_factor' },
      { id: 'attempt', first_factor_verification: { external_verification_redirect_url: 'https://identity.example.test/oauth' } },
      { id: 'attempt', status: 'complete', created_session_id: 'session' },
      { sessions: [{ id: 'session', status: 'active', user: { id: 'user' } }], last_active_session_id: 'session' },
    ]
    return { ok: true, headers: new Headers(calls === 1 ? { authorization: 'fixture-native' } : {}), json: async () => ({ response: responses[calls - 1] }) }
  } })
  const account = new mod.AccountService()
  account.state = { ...account.state, socialProviders: ['apple', 'google', 'x'] }
  await (account as unknown as { restore(): Promise<void> }).restore()
  assert.equal(await account.signIn('not-allowed' as never), 'error')
  assert.equal(calls, 0)
  assert.equal(await account.signIn(provider), null)
  assert.deepEqual(strategies, [`oauth_${provider}`, `oauth_${provider}`])
  assert.equal(account.state.user?.userId, 'user')
  assert.equal(calls, 4)
  }
})
