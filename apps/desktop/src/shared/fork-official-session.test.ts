import assert from 'node:assert/strict'
import { randomBytes } from 'node:crypto'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'
import { accountBrowserURL, accountConfig } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

test('official session requires live provider proof and closes on offline, expiry and logout', async () => {
  let now = 1_800_000_000_000
  let expiry = now + 90_000
  let offline = false
  let nextTimer = 0
  const timers = new Map<number, () => Promise<void>>()
  const config = { clerkFrontendOrigin: 'https://clerk.example.invalid', clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: 'https://app.example.invalid' }
  const setup = (configured: boolean) => loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true } },
    './environment': { identity: { appId: 'app.fixture' }, buildConfig: {}, officialLoginConfig: configured ? config : null },
    '../shared/fork-features': { forkFeatures: { cloudAccess: false }, officialDesktop: true },
    './auth': {}, './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class { async load() { return { kind: 'empty' } } async clear() {} } },
    './fork-account/password-attempt': { PasswordAttemptStore: class { async load() { return null } async clear() {} } },
  }, {
    URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, Date: { now: () => now },
    setTimeout: (action: () => Promise<void>) => { const id = ++nextTimer; timers.set(id, action); return { id, unref() {} } },
    clearTimeout: (timer?: { id: number }) => { if (timer) timers.delete(timer.id) },
    fetch: async () => {
      if (offline) throw new Error('offline')
      return { ok: true, headers: new Headers(), json: async () => ({ response: { last_active_session_id: 'session', sessions: [{ id: 'session', status: 'active', expire_at: expiry, user: { id: 'user' } }] } }) }
    },
  })
  const noConfig = new (setup(false).AccountService)()
  assert.equal(noConfig.state.configured, false)
  assert.equal(noConfig.officialSessionCurrent(), false)
  const service = new (setup(true).AccountService)()
  const fixture = service as unknown as { refreshClient(): Promise<void>; deviceToken: string; set(change: { ready: boolean }): void }
  fixture.set({ ready: true })
  assert.equal(service.officialSessionCurrent(), false)
  fixture.deviceToken = 'fixture-native-client'
  await fixture.refreshClient()
  assert.equal(service.officialSessionCurrent(), true)
  assert.equal(service.state.user?.userId, 'user')

  offline = true
  now += 10_000
  await timers.get(nextTimer)!()
  assert.equal(service.state.user, null)
  assert.equal(service.officialSessionCurrent(), false)
  assert.equal(service.reconciliationState().kind, 'session_unverified')

  offline = false
  await timers.get(nextTimer)!()
  assert.equal(service.officialSessionCurrent(), true)
  now = expiry
  assert.equal(service.officialSessionCurrent(), false)

  expiry += 90_000
  await fixture.refreshClient()
  assert.equal(service.officialSessionCurrent(), true)
  const signingOut = service.signOut()
  assert.equal(service.officialSessionCurrent(), false)
  await signingOut
  assert.equal(service.state.user, null)
})
