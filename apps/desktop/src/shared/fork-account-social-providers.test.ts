import assert from 'node:assert/strict'
import test from 'node:test'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { visibleProviders } from './fork-account-providers.ts'
import { loadMainModule } from './test-support/main-module.ts'

// Real AccountService; Clerk FAPI, browser and system storage are fakes.
const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const social = (on: string[]) => ({ user_settings: { social: Object.fromEntries(['oauth_apple', 'oauth_google', 'oauth_x', 'oauth_github'].map(s => [s, { enabled: on.includes(s), authenticatable: true }])) } })
const production = social(['oauth_google'])
const development = social(['oauth_apple', 'oauth_google', 'oauth_x'])

function world(environment: () => unknown) {
  const calls: string[] = []; const retries: (() => void)[] = []
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value) } },
    './auth': { authenticate: async () => { throw new Error('browser must not open') }, AuthenticationEnded: class extends Error {} },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class { async hasArchives() { return false } async load() { return { kind: 'empty' } } async clear() {} async retain(_b: Buffer, _s: string, current: () => void) { current() } } },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, setTimeout: (fn: () => void) => { retries.push(fn); return {} }, fetch: async (url: URL, init: { method: string; headers: Record<string, string>; credentials: string; redirect: string }) => {
    calls.push(`${init.method} ${url.pathname}`)
    if (url.pathname !== '/v1/environment') throw new Error('sign-in must not start')
    assert.equal(url.searchParams.get('_is_native'), 'true'); assert.equal(init.headers.Authorization, undefined)
    assert.equal(init.credentials, 'omit'); assert.equal(init.redirect, 'error')
    const body = environment()
    if (body instanceof Error) throw body
    return { ok: true, headers: new Headers(), json: async () => body }
  } })
  return { mod, calls, retries }
}

test('environment ilegível → nenhum provedor social, Main recusa; leitura posterior libera', async () => {
  let env: unknown = new Error('offline')
  const w = world(() => env)
  const account = new w.mod.AccountService()
  await (account as unknown as { restore(): Promise<void> }).restore()
  await account.loadSocialProviders()
  assert.deepEqual([...account.state.socialProviders], [])
  assert.equal(await account.signIn('google'), 'error')
  assert.deepEqual(w.calls, ['GET /v1/environment'])
  assert.equal(w.retries.length, 1)
  env = production; w.retries[0](); await new Promise(r => setImmediate(r))
  assert.deepEqual([...account.state.socialProviders], ['google'])
})

test('produção (só Google) → só o botão Google; Main recusa Apple e X', async () => {
  const w = world(() => production)
  const account = new w.mod.AccountService()
  await (account as unknown as { restore(): Promise<void> }).restore()
  await account.loadSocialProviders()
  assert.deepEqual([...account.state.socialProviders], ['google'])
  assert.deepEqual(visibleProviders(account.state.socialProviders), ['google'])
  assert.equal(await account.signIn('apple'), 'error')
  assert.equal(await account.signIn('x'), 'error')
  assert.deepEqual(w.calls, ['GET /v1/environment'])
})

test('Dev (Apple, Google e X) → três botões na ordem da D177', async () => {
  const w = world(() => development)
  const account = new w.mod.AccountService()
  await account.loadSocialProviders()
  assert.deepEqual(visibleProviders(account.state.socialProviders), ['apple', 'x', 'google'])
})
