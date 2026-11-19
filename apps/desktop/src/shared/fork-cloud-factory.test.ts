import assert from 'node:assert/strict'
import test from 'node:test'
import * as wire from '../main/fork-cloud/crypto.ts'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

const deployment = { deploymentId: 'fixture', env: 'dev' as const, issuer: 'https://issuer.example.test', origin: 'https://cloud.example.test', manifestHash: 'a'.repeat(64) }
const seal = { epoch: 1, userId: 'fixture-user', sessionId: 'fixture-session' }
const key = wire.b64url(new Uint8Array(32).fill(2))
const binding = { version: 1 as const, ...seal, origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_fixture', deviceKey: key }

test('registro tardio não publica alvo; saída usa alvo capturado e recibo incompleto fica incerto', async () => {
  const gate = deferred<typeof binding | null>()
  let live = true, deletes = 0, after = false, complete = false
  let epochListener = () => {}
  let capture: ((capturedSeal: typeof seal, capturedBinding: typeof binding | null) => ((jwt: string, current: () => boolean) => Promise<{ kind: string }>) | null) | undefined
  let retire: ((capturedBinding: typeof binding, current: () => boolean) => Promise<boolean>) | undefined
  const account = {
    state: { cloudURL: deployment.origin }, sessionEpoch: 1,
    on: (_event: string, listener: () => void) => { epochListener = listener },
    isCloudSessionCurrent: () => live,
    deviceIdentityScope: () => ({ origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash }),
    tokenForCloud: async () => 'fixture-jwt', saveDeviceBinding: async () => gate.promise,
    setBindingRestorer: () => {}, setDeviceRevoker: (value: typeof capture) => { capture = value }, setDeviceRevocationVerifier: () => {},
    setReconciliationKeyReader: () => {}, setReconciliationKeyVerifier: () => {}, setReconciliationRegistrar: () => {},
    setDeviceIdentityRetirer: (value: typeof retire) => { retire = value },
  }
  const principal = { seal, deployment, namespace: 'fixture', current: () => live, token: async () => 'fixture-jwt' }
  const fakeApi = class { async prepareRegistrationKey() { return new Uint8Array(32).fill(2) } async execute(intent: { kind: string }) { return intent.kind === 'health' ? { kind: 'ok', value: {} } : { kind: 'ok', value: { device: { deviceId: binding.deviceId, deviceKey: key } } } } async captureDeviceKey() { return new Uint8Array(32).fill(2) } }
  const mod = loadMainModule<typeof import('../main/fork-cloud/register.ts')>('fork-cloud/register', {
    '../../shared/fork-features': { forkFeatures: { cloudAccess: true } },
    '../fork-account/config': { accountConfig: () => ({ kind: 'configured', cloudOrigin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, env: deployment.env, manifestHash: deployment.manifestHash }) },
    '../environment': { buildConfig: {} }, '../fork-account/device-key': {}, './account-api': { AccountApi: fakeApi }, './crypto': wire,
    './device-keys': { deviceSigner: async () => ({ publicKey: new Uint8Array(32).fill(2) }), retireCapturedDevice: async () => { after = true } },
    './pins': { matchesDeployment: () => true }, './principal': { capturePrincipal: () => principal, namespaceFor: () => 'fixture' },
  }, { URL, Buffer, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string; headers?: { Authorization: string } }) => {
    if (url.pathname === '/v1/health') return { ok: true, json: async () => ({ ok: true, deployment }) }
    assert.equal(init.headers?.Authorization, 'Bearer fixture-jwt')
    if (init.method === 'DELETE') { deletes++; return { ok: true } }
    return { ok: true, json: async () => ({ devices: [{ deviceId: binding.deviceId, deviceKey: key, revoked: true, revocations: [{ fenceId: 'fnc_grt_fixture', grantId: 'grt_fixture', computerId: wire.b64url(new Uint8Array(16)), deviceId: binding.deviceId, deviceKey: key, deploymentId: deployment.deploymentId, env: deployment.env, cause: 'deviceRevoked', requestedAt: 1, relay: { state: complete ? 'fenced' : 'pending', at: complete ? 2 : null, attempts: 1 }, hub: { state: 'fenced', at: 3 } }] }] }) }
  } })
  const factory = mod.registerForkCloud(account as never)!
  const pending = factory.registerDevice('fixture', 'macos')
  await settle()
  live = false; epochListener(); gate.resolve(binding)
  assert.equal((await pending).kind, 'unknown')
  assert.equal(capture!(seal, binding), null)
  live = true
  const second = await factory.registerDevice('fixture', 'macos')
  assert.equal(second.kind, 'registered')
  const revoke = capture!(seal, binding)!
  live = false; epochListener()
  assert.equal((await revoke('fixture-jwt', () => true)).kind, 'unknown')
  assert.equal((await revoke('fixture-jwt', () => true)).kind, 'unknown')
  assert.equal(deletes, 1)
  assert.equal(await retire!(binding, () => true), false)
  assert.equal(after, false)
  live = true
  assert.equal((await factory.registerDevice('fixture', 'macos')).kind, 'registered')
  const confirmed = capture!(seal, binding)!
  live = false; epochListener(); complete = true
  assert.equal((await confirmed('fixture-jwt', () => true)).kind, 'revoked')
  assert.equal(await retire!(binding, () => true), true)
  assert.equal(after, true)
})

test('nova factory verifica saída só por GET e cerca aposentadoria à guarda atual', async () => {
  let epoch = 1, retired = 0
  let epochListener = () => {}
  let value: unknown = undefined
  const calls: string[] = []
  let verify: (target: typeof binding, jwt: string, current: () => boolean) => Promise<{ kind: string }> = async () => { throw new Error('verifier missing') }
  let retire: (target: typeof binding, current: () => boolean) => Promise<boolean> = async () => false
  const account = {
    state: { cloudURL: deployment.origin }, get sessionEpoch() { return epoch },
    on: (_event: string, listener: () => void) => { epochListener = listener },
    setBindingRestorer: () => {}, setDeviceRevoker: () => {},
    setDeviceRevocationVerifier: (callback: typeof verify) => { verify = callback },
    setDeviceIdentityRetirer: (callback: typeof retire) => { retire = callback },
    setReconciliationKeyReader: () => {}, setReconciliationKeyVerifier: () => {}, setReconciliationRegistrar: () => {},
  }
  const mod = loadMainModule<typeof import('../main/fork-cloud/register.ts')>('fork-cloud/register', {
    '../../shared/fork-features': { forkFeatures: { cloudAccess: true } },
    '../fork-account/config': { accountConfig: () => ({ kind: 'configured', cloudOrigin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, env: deployment.env, manifestHash: deployment.manifestHash }) },
    '../environment': { buildConfig: {} }, '../fork-account/device-key': {}, './account-api': { AccountApi: class {} }, './crypto': wire,
    './device-keys': { retireCapturedDevice: async (_binding: unknown, _deployment: unknown, current: () => boolean) => { assert.equal(current(), true); retired++ } },
    './pins': { matchesDeployment: () => true }, './principal': { namespaceFor: () => 'fixture', capturePrincipal: () => { throw new Error('normal session must stay fenced') } },
  }, { URL, Buffer, process: { platform: 'darwin' }, fetch: async (url: URL, init: { method: string; headers?: { Authorization: string } }) => {
    calls.push(`${init.method} ${url.pathname}`)
    assert.equal(init.method, 'GET')
    if (url.pathname === '/v1/health') return { ok: true, json: async () => ({ ok: true, deployment }) }
    assert.equal(init.headers?.Authorization, 'Bearer fixture-jwt')
    return { ok: true, json: async () => ({ devices: [{ deviceId: binding.deviceId, deviceKey: key, revoked: true, revocations: value }] }) }
  } })
  mod.registerForkCloud(account as never)
  assert.deepEqual(calls, [] as string[])
  const first = () => epoch === 1
  assert.equal((await verify(binding, 'fixture-jwt', first)).kind, 'unknown')
  assert.equal(await retire(binding, first), false)
  value = []
  assert.equal((await verify(binding, 'fixture-jwt', first)).kind, 'revoked')
  epoch = 2; epochListener()
  const next = () => epoch === 2
  assert.equal(await retire(binding, next), false)
  value = [{ fenceId: 'fnc_grt_fixture', grantId: 'grt_fixture', computerId: wire.b64url(new Uint8Array(16)), deviceId: binding.deviceId, deviceKey: key, deploymentId: deployment.deploymentId, env: deployment.env, cause: 'deviceRevoked', requestedAt: 1, relay: { state: 'pending', at: null, attempts: 1 }, hub: { state: 'fenced', at: 3 } }]
  assert.equal((await verify(binding, 'fixture-jwt', next)).kind, 'unknown')
  assert.equal(await retire(binding, next), false)
  ;(value as { relay: unknown }[])[0]!.relay = { state: 'fenced', at: 2, attempts: 1 }
  assert.equal((await verify(binding, 'fixture-jwt', next)).kind, 'revoked')
  assert.equal(await retire(binding, () => false), false)
  assert.equal(retired, 0)
  assert.equal(await retire(binding, next), true)
  assert.equal(retired, 1)
  assert.equal(await retire(binding, next), false)
  assert.ok(calls.every((call) => call.startsWith('GET ')))
})
