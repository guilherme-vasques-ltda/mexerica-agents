import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import * as fs from 'node:fs'
import * as wire from '../main/fork-cloud/crypto.ts'
import { deferred, loadMainModule } from './test-support/main-module.ts'

const principal = () => ({ seal: { epoch: 1, userId: 'fixture', sessionId: 'fixture' }, namespace: 'fixture', deployment: { deploymentId: 'fixture', env: 'dev' as const, issuer: 'https://issuer.example.test', origin: 'https://cloud.example.test', manifestHash: 'a'.repeat(64) }, current: () => true, token: async () => 'fixture' })

test('tentativa congela nonce e approved remoto não cria pin', async () => {
  let hostNonce = wire.b64url(new Uint8Array(32).fill(4))
  let status = 'pending'
  let reveals = 0
  const host = new Uint8Array(32).fill(3)
  const computerId = wire.computerIdOf(host)
  const api = class {
    async captureDeviceKey() { return new Uint8Array(32).fill(8) }
    async execute(intent: { kind: string }) {
    if (intent.kind === 'health') return { kind: 'ok', value: {} }
    if (intent.kind === 'requestAccess') return { kind: 'ok', value: { requestId: 'req_fixture', expiresAt: Date.now() + 60_000 } }
    if (intent.kind === 'listComputers') return { kind: 'ok', value: { computers: [{ computerId, signKey: wire.b64url(host) }] } }
    if (intent.kind === 'getAccess') return { kind: 'ok', value: { requestId: 'req_fixture', computerId, hostNonce, status, expiresAt: Date.now() + 60_000 } }
    if (intent.kind === 'revealAccess') { reveals++; return { kind: 'ok', value: {} } }
    throw new Error('unexpected')
  } }
  const mod = loadMainModule<typeof import('../main/fork-cloud/enroll.ts')>('fork-cloud/enroll', {
    'node:crypto': crypto, 'node:fs': fs, 'node:path': path, electron: { safeStorage: {} }, './pins': {}, './crypto': wire, './account-api': { AccountApi: api },
    './device-keys': { deviceSigner: async () => ({ publicKey: new Uint8Array(32).fill(8) }) },
  }, { Buffer })
  const attempt = new mod.Enrollment(principal(), computerId, wire.b64url(host))
  assert.equal((await attempt.start()).kind, 'linking')
  assert.equal((await attempt.check()).kind, 'waitingOwnerApproval')
  status = 'approved'
  assert.equal((await attempt.check()).kind, 'waitingOwnerApproval')
  hostNonce = wire.b64url(new Uint8Array(32).fill(5))
  assert.equal((await attempt.check()).kind, 'hubUnverified')
  assert.equal(reveals, 1)
})

test('pin ausente não conecta e época trocada impede entrega tardia', async () => {
  let current = true, calls = 0
  const pending = deferred<null>()
  const fakePrincipal = { ...principal(), current: () => current }
  const account = { on() {}, off() {}, isCloudSessionCurrent: () => current, verifiedDeviceBinding: () => null }
  const mod = loadMainModule<typeof import('../main/fork-cloud/connection.ts')>('fork-cloud/connection', {
    'node:crypto': crypto, './account-api': { AccountApi: class {} }, './device-keys': { deviceSigner: async () => { calls++; throw new Error('unexpected') } },
    './crypto': wire, './pins': { readPin: () => pending.promise }, './relay-socket': { RelayLink: class {} },
  }, { Buffer })
  const states: unknown[] = []
  const connection = new mod.Connection(account as never, fakePrincipal, wire.computerIdOf(new Uint8Array(32).fill(3)), (state) => states.push(state))
  const start = connection.start()
  current = false
  pending.resolve(null)
  assert.equal(await start, false)
  assert.equal(calls, 0)
  assert.deepEqual(states, [])
})

test('leitura de pin fecha quando a cifra estiver indisponível', async () => {
  let reads = 0
  const mod = loadMainModule<typeof import('../main/fork-cloud/pins.ts')>('fork-cloud/pins', {
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: { readFile: async () => { reads++; throw new Error('unexpected') } } },
    electron: { app: { getPath: () => '/fixture' }, safeStorage: { isEncryptionAvailable: () => false } }, './crypto': wire, './device-keys': {},
  }, { Buffer, process: { platform: 'darwin' } })
  await assert.rejects(mod.readPin(principal(), wire.computerIdOf(new Uint8Array(32).fill(3))))
  assert.equal(reads, 0)
})
