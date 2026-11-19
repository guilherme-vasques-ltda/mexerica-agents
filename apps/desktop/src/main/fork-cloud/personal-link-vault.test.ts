import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as nodeCrypto from 'node:crypto'
import * as crypto from './crypto.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

// D70 PR-A: cofre ausente/fraco/desconhecido FECHA antes de cifrar, decifrar ou assinar. Mock não qualifica SO.
test('backend unknown, basic_text, indisponível ou plataforma não admitida fecham sem uso de cifra', async () => {
  for (const [platform, available, backend] of [['linux', true, 'unknown'], ['linux', true, 'basic_text'], ['linux', true, 'kwallet9'], ['linux', false, 'gnome_libsecret'], ['darwin', false, 'unknown'], ['freebsd', true, 'unknown']] as const) {
    const calls = { encrypt: 0, decrypt: 0, fs: 0 }
    const safeStorage = { isEncryptionAvailable: () => available, getSelectedStorageBackend: () => backend, encryptString: () => { calls.encrypt++; return Buffer.alloc(0) }, decryptString: () => { calls.decrypt++; return '' } }
    const guarded = new Proxy(fs, { get(target, key) { calls.fs++; return Reflect.get(target, key) } })
    const { PersonalLinkVault } = loadMainModule<typeof import('./personal-link-vault.ts')>('fork-cloud/personal-link-vault', {
      'node:crypto': nodeCrypto, 'node:fs': guarded, 'node:path': path, './crypto': crypto, electron: { app: { getPath: () => '/nonexistent-synthetic' }, safeStorage },
    }, { Buffer, process: { ...process, platform, getuid: process.getuid } })
    await assert.rejects(PersonalLinkVault.open('c'.repeat(64), 'fixture'), /personalLinkVaultUnavailable/, `${platform}/${backend}`)
    assert.deepEqual(calls, { encrypt: 0, decrypt: 0, fs: 0 }, `${platform}/${backend}`)
  }
})

test('B reopen ausente não cria diretório; geração existente é readonly e não cai na anterior', async () => {
  const os = await import('node:os')
  const base = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'd433-vault-'))
  let enabled = true
  const safeStorage = { isEncryptionAvailable: () => enabled, getSelectedStorageBackend: () => 'gnome_libsecret', encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() }
  const { PersonalLinkVault } = loadMainModule<typeof import('./personal-link-vault.ts')>('fork-cloud/personal-link-vault', {
    'node:crypto': nodeCrypto, 'node:fs': fs, 'node:path': path, './crypto': crypto, electron: { app: { getPath: () => base }, safeStorage },
  }, { Buffer, process })
  try {
    await assert.rejects(PersonalLinkVault.reopen('c'.repeat(64), 'fixture'))
    assert.deepEqual(await fs.promises.readdir(base), [])
    const initial = await PersonalLinkVault.open('c'.repeat(64), 'fixture')
    await initial.signer(true)
    const snapshot = async () => {
      const parent = path.join(base, (await fs.promises.readdir(base))[0])
      const result: Record<string, string> = {}
      for (const gen of await fs.promises.readdir(parent)) for (const name of await fs.promises.readdir(path.join(parent, gen))) result[`${gen}/${name}`] = (await fs.promises.readFile(path.join(parent, gen, name))).toString('hex')
      return result
    }
    const before = await snapshot(), readOnly = await PersonalLinkVault.reopen('c'.repeat(64), 'fixture'), signer = await readOnly.signer(false)
    await assert.rejects(readOnly.seal('receipt', Buffer.from('x')))
    await assert.rejects(readOnly.next())
    assert.deepEqual(await snapshot(), before)
    enabled = false
    assert.throws(() => signer.sign(Buffer.from('CR')))
    enabled = true
    await initial.next() // geração nova sem device; B fecha em vez de usar g1.
    const incomplete = await PersonalLinkVault.reopen('c'.repeat(64), 'fixture')
    await assert.rejects(incomplete.signer(false))
  } finally { await fs.promises.rm(base, { recursive: true, force: true }) }
})
