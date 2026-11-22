import assert from 'node:assert/strict'
import { randomBytes } from 'node:crypto'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'
import { accountBrowserURL, accountConfig } from '../main/fork-account/config.ts'
import { deferred, loadMainModule } from './test-support/main-module.ts'

test('official session requires live provider proof and closes on offline, expiry and logout', async () => {
  let now = 1_800_000_000_000
  let expiry = now + 90_000
  let offline = false
  let stored: Buffer | null = null
  let callback = ''
  let slowWrite = false
  let failWrite = false
  let sessionId = 'session'
  let heldRefresh: ReturnType<typeof deferred<void>> | null = null
  const removed: string[] = []
  let nextTimer = 0
  const timers = new Map<number, () => Promise<void>>()
  const config = { clerkFrontendOrigin: 'https://clerk.example.invalid', clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: 'https://app.example.invalid' }
  const setup = (configured: boolean) => loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (value: string) => Buffer.from(value), decryptString: (value: Buffer) => value.toString() } },
    './environment': { identity: { appId: 'app.fixture' }, buildConfig: {}, officialLoginConfig: configured ? config : null },
    '../shared/fork-features': { forkFeatures: { cloudAccess: false }, officialDesktop: true },
    './auth': { authenticate: async () => callback, AuthenticationEnded: class extends Error {} }, './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class {
      async hasArchives() { return false }
      async load() { return stored ? { kind: 'active', sealed: stored } : { kind: 'empty' } }
      async retain(data: Buffer, _state: string, current: () => void) {
        current()
        if (JSON.parse(data.toString()).principal) {
          if (slowWrite) { slowWrite = false; now += 21_000 }
          if (failWrite) throw new Error('fixture write failed')
        }
        stored = data
      }
      async clear() { stored = null }
    } },
    './fork-account/password-attempt': { PasswordAttemptStore: class { async load() { return null } async loadForExit() { return null } async clear() {} } },
  }, {
    URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, Date: { now: () => now },
    setTimeout: (action: () => Promise<void>) => { const id = ++nextTimer; timers.set(id, action); return { id, unref() {} } },
    clearTimeout: (timer?: { id: number }) => { if (timer) timers.delete(timer.id) },
    fetch: async (url: URL, init: { method: string; body?: string }) => {
      if (offline) throw new Error('offline')
      let response: unknown
      const headers = new Headers()
      if (url.pathname === '/v1/client/sign_ins' && init.method === 'POST') {
        callback = new URLSearchParams(init.body).get('redirect_url')!
        headers.set('authorization', 'fixture-native-client')
        response = { id: 'signin', status: 'needs_first_factor' }
      } else if (url.pathname.endsWith('/prepare_first_factor')) {
        response = { id: 'signin', status: 'needs_first_factor', first_factor_verification: { external_verification_redirect_url: 'https://clerk.example.invalid/oauth' } }
      } else if (url.pathname === '/v1/client/sign_ins/signin') {
        response = { id: 'signin', status: 'complete', created_session_id: sessionId }
      } else {
        response = { last_active_session_id: sessionId, sessions: [{ id: sessionId, status: 'active', expire_at: expiry, user: { id: 'user' } }] }
        if (url.pathname.endsWith('/remove')) removed.push(url.pathname)
        if (url.pathname === '/v1/client' && heldRefresh) {
          const hold = heldRefresh
          heldRefresh = null
          await hold.promise
          headers.set('authorization', 'fixture-native-client')
        }
      }
      return { ok: true, headers, json: async () => ({ response }) }
    },
  })
  const noConfig = new (setup(false).AccountService)()
  assert.equal(noConfig.state.configured, false)
  assert.equal(noConfig.officialSessionCurrent(), false)
  const service = new (setup(true).AccountService)()
  const projectedUser = () => service.state.user
  const restore = (account: typeof service) => (account as unknown as { restore(): Promise<void> }).restore()
  await restore(service)
  assert.equal(service.officialSessionCurrent(), false)
  service.state.socialProviders = ['google']
  slowWrite = true
  assert.equal(await service.signIn('google'), null)
  assert.equal(service.officialSessionCurrent(), false)
  assert.equal(projectedUser(), null)
  assert.equal(timers.size, 1)
  now += 10_000
  await timers.get(nextTimer)!()
  assert.equal(service.officialSessionCurrent(), true)
  assert.equal(projectedUser()?.userId, 'user')
  assert.equal(timers.size, 2)

  offline = true
  now += 10_000
  await timers.get(nextTimer)!()
  assert.equal(service.state.user, null)
  assert.equal(service.officialSessionCurrent(), false)
  assert.equal(service.reconciliationState().kind, 'session_unverified')

  offline = false
  now += 10_000
  await timers.get(nextTimer)!()
  assert.equal(service.officialSessionCurrent(), true)
  const signingOut = service.signOut()
  assert.equal(service.officialSessionCurrent(), false)
  await signingOut
  assert.equal(service.state.user, null)
  assert.equal(timers.size, 0)

  stored = Buffer.from(JSON.stringify({ version: 3, token: 'fixture-native-client', principal: { userId: 'user', sessionId: 'session' } }))
  const restored = new (setup(true).AccountService)()
  expiry = now + 90_000
  failWrite = true
  await restore(restored)
  assert.equal(restored.officialSessionCurrent(), false)
  assert.equal(restored.reconciliationState().kind, 'session_unverified')
  now += 21_000
  failWrite = false
  assert.equal((await restored.reconcile('check')).kind, 'none')
  assert.equal(restored.officialSessionCurrent(), true)
  assert.equal(timers.size, 2)
  now += 10_000
  await timers.get(nextTimer)!()
  assert.equal(restored.officialSessionCurrent(), true)
  now = expiry
  assert.equal(restored.officialSessionCurrent(), false)
  await timers.get(nextTimer)!()
  assert.equal(restored.state.user, null)

  expiry = now + 15_000
  restored.state.socialProviders = ['google']
  assert.equal(await restored.signIn('google'), null)
  assert.equal(restored.officialSessionCurrent(), true)
  const late = deferred()
  heldRefresh = late
  now += 10_000
  const pending = timers.get(nextTimer)!()
  now = expiry
  assert.equal(restored.officialSessionCurrent(), false)
  late.resolve()
  await pending
  assert.equal(stored, null)
  assert.equal(restored.state.user, null)

  expiry = now + 15_000
  assert.equal(await restored.signIn('google'), null)
  const stale = deferred()
  heldRefresh = stale
  now += 10_000
  const oldCheck = timers.get(nextTimer)!()
  now = expiry
  await timers.get(nextTimer - 1)!()
  assert.equal(restored.officialSessionCurrent(), false)
  sessionId = 'replacement'
  expiry = now + 90_000
  assert.equal(await restored.signIn('google'), null)
  const savedReplacement = stored
  stale.resolve()
  await oldCheck
  assert.equal(restored.officialSessionCurrent(), true)
  assert.equal(stored, savedReplacement)
  assert.ok(removed.every(path => !path.includes('/replacement/')))
})
