import assert from 'node:assert/strict'
import test from 'node:test'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { loadMainModule } from './test-support/main-module.ts'

// Uses the actual service and a fake transport; no network or real profile.
test('saída durante operação pendente cerca o selo e não duplica a saída', async () => {
  let clerk = 0
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': { randomBytes },
    electron: { app: { getPath: () => '/fixture', getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s) } },
    './auth': {}, './environment': { identity: { appId: 'fixture', releaseUpdates: false }, buildConfig: {} },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } },
    './fork-account/config': { accountConfig, accountBrowserURL },
    './fork-account/storage': { AccountStorage: class { async retain(_data: Buffer, _state: string, current: () => void) { current() } async block() {} async load() { return { kind: 'empty' } } async clear() {} } },
  }, { URL, URLSearchParams, Buffer, process: { platform: 'darwin' }, fetch: async () => { clerk++; throw new Error('unexpected') } })
  const account = new mod.AccountService()
  const state = account as unknown as { base: string; deviceToken: string; cachedToken: unknown; reconcileBusy: boolean; logoutPending: boolean; pendingExit: unknown; applyClient(client: unknown): void }
  state.base = 'https://identity.example.test'
  state.deviceToken = 'Bearer fixture'
  state.applyClient({ sessions: [{ id: 'session-a', status: 'active', user: { id: 'user-a' } }], last_active_session_id: 'session-a' })
  const seal = account.captureCloudSession()!
  state.reconcileBusy = true
  const before = account.sessionEpoch
  await account.signOut()
  assert.equal(account.sessionEpoch, before + 1)
  assert.equal(account.isCloudSessionCurrent(seal), false)
  assert.equal(account.state.user, null)
  await assert.rejects(account.sessionToken())
  assert.equal(state.deviceToken, null)
  assert.equal(clerk, 1)
})
