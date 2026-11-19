import assert from 'node:assert/strict'
import test from 'node:test'
import * as wire from '../main/fork-cloud/crypto.ts'
import { deferred, loadMainModule } from './test-support/main-module.ts'

const deployment = { deploymentId: 'fixture', env: 'dev' as const, issuer: 'https://issuer.example.test', origin: 'https://cloud.example.test', manifestHash: 'a'.repeat(64) }
const seal = { epoch: 1, userId: 'fixture-user', sessionId: 'fixture-session' }
const publicKey = new Uint8Array(32).fill(45)
const key = wire.b64url(publicKey)
const binding = { version: 1 as const, ...seal, origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_fixture', deviceKey: key }

function world() {
  let live = true, posts = 0, saves = 0, prepares = 0, missing = false
  let rows: unknown[] = []
  let wait: Promise<void> | null = null
  let revoker: ((seal: unknown, binding: unknown) => unknown) | null = null
  const principal = { seal, deployment, namespace: 'fixture', current: () => live, token: async () => 'fixture-only' }
  const account = { state: { cloudURL: deployment.origin }, sessionEpoch: 1, on() {},
    setBindingRestorer() {}, setDeviceRevocationVerifier() {}, setReconciliationKeyReader() {}, setReconciliationKeyVerifier() {}, setReconciliationRegistrar() {}, setDeviceIdentityRetirer() {},
    setDeviceRevoker: (cb: typeof revoker) => { revoker = cb },
    saveDeviceBinding: async () => { saves++; return binding }, isCloudSessionCurrent: () => live,
  }
  const api = class {
    async prepareRegistrationKey() { prepares++; return publicKey }
    async captureDeviceKey() { return publicKey }
    async execute(intent: { kind: string }) {
      if (intent.kind === 'health') return { kind: 'ok', value: {} }
      if (intent.kind === 'registerDevice') { posts++; return { kind: 'unknown' } }
      if (intent.kind === 'listDevices') { if (wait) await wait; return { kind: 'ok', value: { devices: rows } } }
      throw new Error('Unexpected operation')
    }
  }
  const mod = loadMainModule<typeof import('../main/fork-cloud/register.ts')>('fork-cloud/register', {
    '../../shared/fork-features': { forkFeatures: { cloudAccess: true } },
    '../fork-account/config': { accountConfig: () => ({ kind: 'configured', cloudOrigin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, env: deployment.env, manifestHash: deployment.manifestHash }) },
    '../environment': { buildConfig: {} }, '../fork-account/device-key': {}, './account-api': { AccountApi: api }, './crypto': wire,
    './device-keys': { deviceSigner: async () => { if (missing) throw Object.assign(new Error('missing'), { code: 'ENOENT' }); return { publicKey } } }, './pins': { matchesDeployment: () => true },
    './principal': { capturePrincipal: () => principal, namespaceFor: () => 'fixture' },
  }, { URL, Buffer, process })
  const factory = mod.registerForkCloud(account as never)!
  return { factory, missingKey: () => { missing = true }, setRows: (value: unknown[]) => { rows = value }, block: (value: Promise<void>) => { wait = value }, stop: () => { live = false },
    posts: () => posts, prepares: () => prepares, saves: () => saves, captureRevoker: () => revoker?.(seal, binding) }
}

test('registro incerto consulta a mesma chave e publica binding+revoker sem outro POST', async () => {
  const x = world()
  assert.equal((await x.factory.registerDevice('fixture', 'macos')).kind, 'unknown')
  assert.equal(x.posts(), 1); assert.equal(x.prepares(), 1)
  assert.equal((await x.factory.reconcileRegistration()).kind, 'unknown')
  x.setRows([{ deviceId: 'dev_fixture', deviceKey: key, revoked: false }, { deviceId: 'dev_old', deviceKey: key, revoked: true }])
  assert.equal((await x.factory.reconcileRegistration()).kind, 'unknown')
  assert.equal(x.saves(), 0)
  x.setRows([{ deviceId: 'dev_fixture', deviceKey: key, revoked: false }])
  assert.equal((await x.factory.reconcileRegistration()).kind, 'registered')
  assert.equal(x.saves(), 1); assert.equal(typeof x.captureRevoker(), 'function')
  assert.equal(x.posts(), 1); assert.equal(x.prepares(), 1)
})

test('consulta de registro que termina após saída não grava binding nem cria outra chave', async () => {
  const x = world(), blocked = deferred<void>()
  await x.factory.registerDevice('fixture', 'macos')
  x.setRows([{ deviceId: 'dev_fixture', deviceKey: key, revoked: false }]); x.block(blocked.promise)
  const pending = x.factory.reconcileRegistration()
  x.stop(); blocked.resolve()
  assert.equal((await pending).kind, 'unknown')
  assert.equal(x.saves(), 0); assert.equal(x.posts(), 1); assert.equal(x.prepares(), 1)
})


test('verificar registro sem identidade existente não cria chave nem impede registro explícito', async () => {
  const x = world(); x.missingKey()
  assert.equal((await x.factory.reconcileRegistration()).kind, 'notSent')
  assert.equal(x.posts(), 0); assert.equal(x.prepares(), 0); assert.equal(x.saves(), 0)
  assert.equal((await x.factory.registerDevice('fixture', 'macos')).kind, 'unknown')
  assert.equal(x.posts(), 1); assert.equal(x.prepares(), 1)
  assert.equal((await x.factory.reconcileRegistration()).kind, 'unknown')
  assert.equal(x.posts(), 1); assert.equal(x.prepares(), 1)
})
