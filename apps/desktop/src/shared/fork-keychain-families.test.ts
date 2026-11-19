import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as crypto from 'node:crypto'
import { tmpdir } from 'node:os'
import { loadMainModule } from './test-support/main-module.ts'
import { CofreVoz } from '../main/fork-voz/cofre.ts'

const snapshot = (dir: string) => Object.fromEntries(fs.readdirSync(dir, { recursive: true, withFileTypes: true }).filter(e => e.isFile()).map(e => [path.relative(dir, path.join(e.parentPath, e.name)), fs.readFileSync(path.join(e.parentPath, e.name)).toString('hex')]))

for (const signal of ['unavailable', 'exception', 'malformed'] as const) test(`voz: ${signal} fica travada, conserva bytes e recupera`, async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-voice-'))
  let denied = false
  const sistema = { isEncryptionAvailable: () => !(denied && signal === 'unavailable'), encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => { if (denied && signal === 'exception') throw new Error('denied'); return denied && signal === 'malformed' ? '{}' : b.toString() } }
  try {
    const vault = new CofreVoz({ dados: dir, sistema, plataforma: 'darwin' })
    await vault.guardar('openai', 'sk-fixture-000000000000000000000000000000')
    const before = snapshot(dir); denied = true
    const restarted = new CofreVoz({ dados: dir, sistema, plataforma: 'darwin' })
    await assert.rejects(restarted.abrir('openai'))
    assert.equal((await restarted.estado()).chaves.openai.situacao, 'travada')
    await assert.rejects(restarted.tentarAbrir('openai'))
    assert.deepEqual(snapshot(dir), before)
    denied = false
    assert.ok(await restarted.tentarAbrir('openai'))
    assert.deepEqual(snapshot(dir), before)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

for (const signal of ['unavailable', 'exception', 'malformed'] as const) test(`device-key e geração: ${signal} não substitui nem perde âncora`, async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-device-'))
  let denied = false
  const codec = { getSelectedStorageBackend: () => 'gnome_libsecret', isEncryptionAvailable: () => !(denied && signal === 'unavailable'), encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => { if (denied && signal === 'exception') throw new Error('denied'); return denied && signal === 'malformed' ? '{}' : b.toString() } }
  const load = () => loadMainModule<typeof import('../main/fork-account/device-key.ts')>('fork-account/device-key', {
    'node:fs': fs, 'node:crypto': crypto, 'node:path': path,
    electron: { app: { getPath: () => dir }, safeStorage: codec },
  }, { Buffer, process })
  const scope = { origin: 'https://example.invalid', issuer: 'https://issuer.invalid', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
  try {
    const device = load(), current = () => true
    const key = await device.deviceIdentity('fixture', current, 'register', scope)
    for (const retired of [false, true]) {
      if (retired) await device.retireDeviceIdentity({ ...scope, version: 1, userId: 'fixture', sessionId: 'fixture', deviceId: 'dev_fixture', deviceKey: key.publicKey }, current)
      const before = snapshot(dir); denied = true
      for (const mode of ['read', 'sign', 'register'] as const) await assert.rejects(load().deviceIdentity('fixture', current, mode, scope))
      assert.deepEqual(snapshot(dir), before)
      denied = false
      assert.equal(await load().verifyDeviceGeneration('fixture', key.publicKey, scope), true)
    }
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('storage.clear verifica decifragem antes de gravar tombstone ou remover payload/guard', async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-clear-'))
  try {
    const { AccountStorage } = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', { 'node:fs': fs, 'node:crypto': crypto, 'node:path': path }, { Buffer })
    const storage = new AccountStorage(dir, 'fixture', () => { throw new Error('denied') })
    await storage.retain(Buffer.from('synthetic sealed record'), 'active', () => {})
    const before = snapshot(dir)
    await assert.rejects(storage.clear())
    assert.deepEqual(snapshot(dir), before)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

import * as wire from '../main/fork-cloud/crypto.ts'
import * as personalWire from '../main/fork-cloud/personal-wire.ts'
import type { Principal } from '../main/fork-cloud/principal.ts'

for (const signal of ['unavailable', 'exception', 'malformed'] as const) test(`pins, journals e personal-link: ${signal} preserva todos os bytes`, async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-families-'))
  let fail = false
  const codec = {
    isEncryptionAvailable: () => !(fail && signal === 'unavailable'), getSelectedStorageBackend: () => 'gnome_libsecret',
    encryptString: (s: string) => Buffer.from(s),
    decryptString: (b: Buffer) => { if (fail && signal === 'exception') throw new Error('denied'); return fail && signal === 'malformed' ? '!' : b.toString() },
  }
  const dependencies = { 'node:fs': fs, 'node:crypto': crypto, 'node:path': path, electron: { app: { getPath: () => dir }, safeStorage: codec }, './crypto': wire, './device-keys': {}, './personal-wire': personalWire }
  const pins = loadMainModule<typeof import('../main/fork-cloud/pins.ts')>('fork-cloud/pins', dependencies, { Buffer, process })
  const boundaries = { ...dependencies, './pins': pins }
  const bootstrap = loadMainModule<typeof import('../main/fork-cloud/bootstrap-journal.ts')>('fork-cloud/bootstrap-journal', boundaries, { Buffer, process })
  const { PersonalJournalStore } = loadMainModule<typeof import('../main/fork-cloud/personal-journal.ts')>('fork-cloud/personal-journal', boundaries, { Buffer, process })
  const { PersonalLinkVault } = loadMainModule<typeof import('../main/fork-cloud/personal-link-vault.ts')>('fork-cloud/personal-link-vault', dependencies, { Buffer, process })
  const deployment = { deploymentId: 'fixture', env: 'main' as const, issuer: 'https://issuer.invalid', origin: 'https://cloud.invalid', manifestHash: 'a'.repeat(64) }
  const binding = { version: 1 as const, userId: 'fixture', sessionId: 'fixture', origin: deployment.origin, issuer: deployment.issuer, deploymentId: deployment.deploymentId, manifestHash: deployment.manifestHash, deviceId: 'dev_fixture', deviceKey: Buffer.alloc(32, 7).toString('base64url') }
  const p: Principal = { namespace: 'fixture', deployment, seal: { userId: 'fixture', sessionId: 'fixture', epoch: 1 }, current: () => true, token: async () => 'fixture' }
  try {
    const journal = await bootstrap.createBootstrapJournal(p, binding)
    const personal = new PersonalJournalStore(p, binding)
    const first = await personal.create('command', 'local-linux')
    const vault = await PersonalLinkVault.open('fixture', 'fixture')
    await vault.seal('device', Buffer.alloc(32, 7))
    const computerId = Buffer.alloc(16, 5).toString('base64url'), pin = pins.pinPath(p, computerId)
    fs.writeFileSync(pin, Buffer.from('{}'), { mode: 0o600 })
    const before = snapshot(dir); fail = true
    await assert.rejects(pins.readPin(p, computerId))
    await assert.rejects(bootstrap.readBootstrapJournal(p, binding))
    await assert.rejects(bootstrap.advanceBootstrapJournal(p, binding, journal, 'createUnknown'))
    await assert.rejects(personal.load())
    await assert.rejects(personal.advance(first, 'unknown'))
    await assert.rejects(vault.signer(true))
    assert.deepEqual(snapshot(dir), before)
    // Preserve both hardlinked pending/final representations on failed reconciliation.
    fs.linkSync(pin, `${pin}.pending`)
    const pending = snapshot(dir)
    await assert.rejects(pins.reconcilePin(p, computerId))
    assert.deepEqual(snapshot(dir), pending)
    fail = false
    assert.equal((await bootstrap.readBootstrapJournal(p, binding))?.bootstrapId, journal.bootstrapId)
    assert.equal((await personal.load())?.attemptId, first.attemptId)
    assert.ok(await vault.signer(false))
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

for (const signal of ['unavailable', 'exception', 'malformed'] as const) test(`elevenlabs legado e temporários preservados: ${signal}`, async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-legacy-voice-'))
  let denied = true
  const key = 'synthetic-voice-key-000000000000000000000'
  const sistema = { isEncryptionAvailable: () => !(denied && signal === 'unavailable'), encryptString: () => assert.fail('não recifrar'), decryptString: (b: Buffer) => { if (denied && signal === 'exception') throw new Error('denied'); return denied && signal === 'malformed' ? '{}' : b.toString() } }
  try {
    fs.mkdirSync(path.join(dir, 'voz-chaves'))
    // Retired provider is not consumed or migrated; startup must leave its ciphertext alone.
    fs.writeFileSync(path.join(dir, 'voz-chaves', 'elevenlabs.bin'), 'synthetic legacy ciphertext')
    fs.writeFileSync(path.join(dir, 'voz-chaves', 'openai.bin'), key)
    fs.writeFileSync(path.join(dir, 'voz-chaves', 'elevenlabs.bin.fixture.tmp'), 'synthetic interrupted write')
    fs.writeFileSync(path.join(dir, 'account.bin.fixture.tmp'), 'synthetic interrupted account')
    const before = snapshot(dir)
    for (let restart = 0; restart < 2; restart++) {
      const vault = new CofreVoz({ dados: dir, sistema, plataforma: 'darwin' })
      await assert.rejects(vault.abrir('openai'))
      await assert.rejects(vault.tentarAbrir('openai'))
      assert.deepEqual(snapshot(dir), before)
    }
    denied = false
    assert.equal(await new CofreVoz({ dados: dir, sistema, plataforma: 'darwin' }).abrir('openai'), key)
    assert.deepEqual(snapshot(dir), before)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('quarentena interrompida conserva guard e bytes e retoma por saída explícita', async () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-quarantine-'))
  let fail = true
  try {
    fs.writeFileSync(path.join(dir, 'account.bin'), 'synthetic encrypted data')
    fs.writeFileSync(path.join(dir, 'account.guard'), JSON.stringify({ version: 1, state: 'revocation_unknown' }))
    const before = snapshot(dir)
    const { AccountStorage } = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
      'node:crypto': crypto, 'node:path': path, 'node:fs': { ...fs, promises: { ...fs.promises, rename: async (source: string, target: string) => {
        if (fail && source.endsWith('account.guard')) throw new Error('synthetic I/O failure')
        await fs.promises.rename(source, target)
      } } },
    }, { Buffer })
    const store = new AccountStorage(dir, 'fixture', () => assert.fail('nenhum payload vinculado'))
    await assert.rejects(store.clear('blocked', true))
    assert.equal((await store.load()).kind, 'revocation_unknown')
    assert.equal(fs.readFileSync(path.join(dir, 'account.guard')).toString('hex'), before['account.guard'])
    fail = false
    await store.clear('blocked', true)
    assert.equal((await store.load()).kind, 'empty')
    const after = snapshot(dir)
    for (const [name, bytes] of Object.entries(before)) {
      const matches = Object.entries(after).filter(([file]) => file.endsWith(`/${name}`))
      assert.equal(matches.length, 1)
      assert.equal(matches[0][1], bytes)
    }
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})
