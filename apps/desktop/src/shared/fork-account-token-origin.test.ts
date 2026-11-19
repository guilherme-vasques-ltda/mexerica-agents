import assert from 'node:assert/strict'
import test from 'node:test'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

// Actual AccountService and request path, fixture transport only: no real session or network.
test('emissão de JWT fixa Origin da conta sem enviar credencial nativa à nuvem nem alterar outros pedidos', async () => {
  for (const environment of ['dev', 'main'] as const) {
    const host = `${environment}.identity.example.test`, origin = `https://${host}`
    const tokenOrigin = environment === 'main' ? 'https://app.example.invalid' : 'https://dev.donnabot.example.test'
    const config = { environment, deploymentEnv: environment, clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: origin, clerkProxyOrigin: environment === 'main' ? 'https://auth.example.invalid' : origin, clerkTokenOrigin: tokenOrigin, clerkIssuer: origin, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
    const requests: { url: URL; method: string; headers: Headers; redirect?: string; credentials?: string }[] = []
    const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
      'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
      electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true } },
      './auth': {}, './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
      '../shared/fork-features': { forkFeatures: { cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
      './fork-account/storage': { AccountStorage: class {} },
    }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async (input: URL, init: RequestInit) => {
      requests.push({ url: new URL(String(input)), method: init.method!, headers: new Headers(init.headers), redirect: init.redirect, credentials: init.credentials })
      return { ok: true, headers: new Headers(), json: async () => ({ response: { jwt: 'fixture-session-jwt' } }) }
    } })
    const account = new mod.AccountService()
    const internal = account as unknown as { deviceToken: string; applyClient(client: unknown): void; request(method: string, path: string): Promise<unknown>; cachedToken: { until: number } | null }
    internal.deviceToken = 'Bearer fixture-native-client'
    internal.applyClient({ sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' })
    assert.equal(await account.sessionToken(), 'fixture-session-jwt', 'the session JWT, never the native client credential')
    assert.equal(await account.sessionToken(), 'fixture-session-jwt')
    assert.equal(requests.length, 1, 'valid same-session cache does not mint another token')
    const request = requests[0]!
    assert.equal(request.url.origin, config.clerkProxyOrigin)
    assert.equal(request.url.pathname, '/v1/client/sessions/session-a/tokens')
    assert.equal(request.url.searchParams.get('_is_native'), 'true')
    assert.equal(request.method, 'POST')
    assert.equal(request.headers.get('Origin'), tokenOrigin, 'missing Origin yields missing azp and the strict Worker rejects it')
    assert.notEqual(request.headers.get('Origin'), config.cloudURL)
    assert.notEqual(request.headers.get('Origin'), origin, 'token origin is independent of the FAPI destination')
    assert.equal(request.headers.get('Authorization'), 'Bearer fixture-native-client')
    assert.equal(request.headers.get('x-bundle-id'), 'app.fixture')
    assert.equal(request.redirect, 'error'); assert.equal(request.credentials, 'omit')
    await internal.request('GET', '/v1/client')
    await internal.request('POST', '/v1/client/sessions/session-a/end')
    await internal.request('GET', '/v1/client/sessions/session-a/tokens')
    for (const other of requests.slice(1)) assert.equal(other.headers.get('Origin'), null, 'only the token POST gains Origin')
    internal.cachedToken = null
    assert.equal(await account.sessionToken(), 'fixture-session-jwt')
    assert.equal(requests.at(-1)!.headers.get('Origin'), tokenOrigin, 'new issuance keeps the same configured pin')
    await assert.rejects(internal.request('POST', '@foreign.example.test/tokens'))
    assert.ok(requests.every(r => r.url.origin === config.clerkProxyOrigin), 'native client credential never leaves the fixed FAPI origin')
  }
})
