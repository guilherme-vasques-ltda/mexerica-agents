import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as portableCipher from '@noble/ciphers/chacha.js'
import * as nobleEd25519 from '@noble/curves/ed25519.js'
import * as nobleUtils from '@noble/curves/utils.js'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: 'https://issuer.example.test', cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const restore = (account: unknown) => (account as { restore(): Promise<void> }).restore()

// Actual service, registration API, signer and generation store. Only OS cipher,
// Clerk/Cloud HTTP and IPC are fixtures, with no real credentials or network.
test('F14/D185: sair ou expirar → entrar → registrar usa nova chave; resposta antiga não aposenta a nova', async () => {
  for (const mode of ['exit', 'delayed', 'expired', 'keychain'] as const) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'account-device-reset-'))
    let generation = 0; let denyRestore = false; let encryptionAvailable = true
    const devices: { deviceId: string; deviceKey: string; revoked: boolean }[] = []
    const posts: string[] = []; const deleted: string[] = []
    const removing = deferred(); const finishRemote = deferred(); const backgroundDone = deferred()
    const reply = (value: unknown, auth?: string, status = 200) => ({ ok: status >= 200 && status < 300, status, headers: new Headers(auth ? { authorization: auth } : {}), json: async () => value })
    const fetch = async (url: URL, init: { method: string; headers: Record<string, string> }) => {
      if (url.origin === config.clerkProxyOrigin) {
        if (url.pathname === '/v1/client/sign_ins') { generation++; return reply({ response: { id: `attempt-${generation}`, status: 'needs_first_factor' } }, `Bearer client-${generation}`) }
        if (url.pathname.endsWith('/attempt_first_factor')) return reply({ response: { id: `attempt-${generation}`, status: 'complete', created_session_id: `session-${generation}` } })
        if (url.pathname === '/v1/client') return denyRestore ? reply({ response: { sessions: [{ id: `session-${generation}`, status: 'revoked', user: { id: 'user-a' } }], last_active_session_id: null } }) : reply({ response: { sessions: [{ id: `session-${generation}`, status: 'active', user: { id: 'user-a' } }], last_active_session_id: `session-${generation}` } })
        if (url.pathname.endsWith('/tokens')) return reply({ response: { jwt: 'fixture-jwt' } })
        if (url.pathname.endsWith('/remove')) { backgroundDone.resolve(); return reply({}) }
      }
      if (url.origin === config.cloudURL) {
        if (url.pathname === '/v1/health') return reply({ ok: true, deployment: { deploymentId: config.deploymentId, env: 'main', issuer: config.clerkIssuer, origin: config.cloudURL, manifestHash: config.manifestHash } })
        if (url.pathname === '/v1/devices' && init.method === 'POST') {
          const key = /kid=([^,]+)/.exec(init.headers['Codync-Sig'])![1]
          posts.push(key)
          let device = devices.find(d => d.deviceKey === key)
          // The actual Worker refuses registration with an already revoked key.
          if (device?.revoked) return reply({}, undefined, 403)
          if (!device) { device = { deviceId: `dev_fixture${devices.length + 1}`, deviceKey: key, revoked: false }; devices.push(device) }
          return reply({ device })
        }
        if (url.pathname.startsWith('/v1/devices/') && init.method === 'DELETE') {
          const device = devices.find(d => d.deviceId === url.pathname.split('/').at(-1))!
          assert.ok(device); deleted.push(device.deviceId); removing.resolve()
          if (mode === 'delayed') await finishRemote.promise
          device.revoked = true; return reply({})
        }
        if (url.pathname === '/v1/devices' && init.method === 'GET') return reply({ devices: devices.map(d => ({ ...d, revocations: [] })) })
      }
      throw new Error('Unexpected fixture route')
    }
    const common = {
      'node:events': { EventEmitter }, 'node:crypto': crypto, '@noble/ciphers/chacha.js': portableCipher, '@noble/curves/ed25519.js': nobleEd25519, '@noble/curves/utils.js': nobleUtils, 'node:fs': fs, 'node:path': path,
      electron: { app: { getPath: () => dir, getVersion: () => 'fixture' }, ipcMain: {}, safeStorage: { isEncryptionAvailable: () => encryptionAvailable, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() } },
    }
    const globals = { URL, URLSearchParams, Buffer, Headers, TextEncoder, Uint8Array, process: { platform: 'darwin' }, fetch }
    const service = loadMainModule<typeof import('../main/account.ts')>('account', { ...common,
      './auth': {}, './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
      '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
    }, globals)
    const factory = loadMainModule<typeof import('../main/fork-cloud/register.ts')>('fork-cloud/register', { ...common,
      '../environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
      '../../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, '../fork-account/config': { accountConfig, accountBrowserURL },
      './bootstrap-authority': { bootstrapAuthority: () => null },
    }, globals)
    try {
      let account = new service.AccountService(); await restore(account)
      let cloud = factory.registerForkCloud(account)!
      assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), null)
      assert.equal((await cloud.registerDevice('Fixture', 'macos')).kind, 'registered')
      const oldKey = posts[0]
      if (mode === 'expired') {
        // The same on-disk binding must be retired even without a valid restored seal.
        account = new service.AccountService(); cloud = factory.registerForkCloud(account)!
        denyRestore = true; await restore(account); denyRestore = false
        assert.equal(account.state.error, 'Sua sessão expirou. Entre novamente.')
      } else {
        if (mode === 'keychain') encryptionAvailable = false
        await account.signOut()
        if (mode === 'keychain') {
          assert.equal(account.reconciliationState().kind, 'session_unverified')
          assert.ok(fs.readdirSync(dir).some(n => /^device-key-/.test(n)))
          encryptionAvailable = true
          await account.signOut()
        }
        encryptionAvailable = true
        assert.equal(account.state.error, null)
        if (mode === 'exit' || mode === 'keychain') await backgroundDone.promise
        else await removing.promise
      }
      assert.equal(account.reconciliationState().kind, 'none')
      assert.equal(fs.readdirSync(dir).some(n => /^device-key-/.test(n)), false, 'a identidade capturada é descartada antes da entrada nova')
      assert.equal(await account.signInWithPassword('fixture@example.test', 'fixture-password'), null)
      assert.equal((await cloud.registerDevice('Fixture', 'macos')).kind, 'registered')
      assert.equal(posts.length, 2); assert.notEqual(posts[1], oldKey)
      assert.equal((await cloud.reconcileRegistration()).kind, 'registered')
      const currentBytes = fs.readdirSync(dir).filter(n => /^device-key-/.test(n)).map(n => [n, fs.readFileSync(path.join(dir, n)).toString('hex')])
      if (mode === 'delayed') { finishRemote.resolve(); await backgroundDone.promise; await settle() }
      assert.equal(devices[1].revoked, false)
      assert.deepEqual(deleted, mode === 'expired' || mode === 'keychain' ? [] : ['dev_fixture1'])
      assert.deepEqual(currentBytes, currentBytes.map(([n]) => [n, fs.readFileSync(path.join(dir, n)).toString('hex')]))
      assert.equal((await cloud.reconcileRegistration()).kind, 'registered')
    } finally { finishRemote.resolve(); await settle(); fs.rmSync(dir, { recursive: true, force: true }) }
  }
})
