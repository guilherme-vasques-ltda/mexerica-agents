import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as crypto from 'node:crypto'
import { tmpdir } from 'node:os'
import { prepareKeychain, readStorageGeneration } from '../main/fork-keychain/generation.ts'
import { loadMainModule } from './test-support/main-module.ts'

const codec = {
  isEncryptionAvailable: () => true,
  encryptString: (s: string) => Buffer.from(`sealed:${s}`),
  decryptString: (b: Buffer) => { if (!b.toString().startsWith('sealed:')) throw new Error('fixture'); return b.toString().slice(7) },
}
const snapshot = (dir: string) => Object.fromEntries(fs.readdirSync(dir).sort().map(n => [n, fs.readFileSync(path.join(dir, n))]))

test('canário e geração: publicação, idempotência e recuperação entre as duas publicações', () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-'))
  try {
    assert.equal(readStorageGeneration(dir, 'darwin'), 0)
    prepareKeychain(dir, 'darwin', codec)
    assert.equal(readStorageGeneration(dir, 'darwin'), 1)
    const first = snapshot(dir)
    assert.notEqual(first['keychain-canary.bin'].toString(), 'mexerica/keychain-canary/v1')
    prepareKeychain(dir, 'darwin', { ...codec, encryptString: () => assert.fail('não recifrar') })
    assert.deepEqual(snapshot(dir), first)
    fs.unlinkSync(path.join(dir, 'storage-generation')) // crash antes do commit
    prepareKeychain(dir, 'darwin', { ...codec, encryptString: () => assert.fail('reusar canário') })
    assert.deepEqual(snapshot(dir), first)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('falha/recusa de chave, canário ausente e geração maior nunca apagam nem substituem', () => {
  for (const scenario of ['decrypt', 'malformed', 'denied', 'missing', 'future', 'invalid']) {
    const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-block-'))
    try {
      prepareKeychain(dir, 'darwin', codec)
      if (scenario === 'missing') fs.unlinkSync(path.join(dir, 'keychain-canary.bin'))
      if (scenario === 'future' || scenario === 'invalid') fs.writeFileSync(path.join(dir, 'storage-generation'), scenario === 'future' ? JSON.stringify({ generation: 3, phase: 'ready' }) : 'garbage')
      const before = snapshot(dir)
      assert.throws(() => prepareKeychain(dir, 'darwin', {
        ...codec,
        isEncryptionAvailable: () => scenario !== 'denied',
        decryptString: b => { if (scenario === 'decrypt') throw new Error('secret fixture never shown'); return scenario === 'malformed' ? 'lixo com padding válido' : codec.decryptString(b) },
        encryptString: () => assert.fail('não gerar substituta'),
      }), { reason: scenario === 'denied' ? 'unavailable' : scenario === 'future' || scenario === 'invalid' ? 'unsupported' : 'mismatch' })
      assert.deepEqual(snapshot(dir), before)
      if (scenario === 'future') assert.throws(() => readStorageGeneration(dir, 'darwin'))
    } finally { fs.rmSync(dir, { recursive: true, force: true }) }
  }
})

test('Linux e Windows não consultam marcador nem cofre', () => {
  for (const platform of ['linux', 'win32'] as const) {
    assert.equal(readStorageGeneration('/does-not-exist', platform), 0)
    prepareKeychain('/does-not-exist', platform, { ...codec, isEncryptionAvailable: () => assert.fail('inalterado') })
  }
})

test('symlink e permissões inseguras bloqueiam a cerca síncrona', () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-link-'))
  try {
    const outside = path.join(dir, 'outside')
    fs.writeFileSync(outside, JSON.stringify({ generation: 1, phase: 'ready' }), { mode: 0o600 })
    fs.symlinkSync(outside, path.join(dir, 'storage-generation'))
    assert.throws(() => readStorageGeneration(dir, 'darwin'))
    assert.equal(fs.readFileSync(outside, 'utf8'), JSON.stringify({ generation: 1, phase: 'ready' }))
    fs.unlinkSync(path.join(dir, 'storage-generation'))
    fs.writeFileSync(path.join(dir, 'storage-generation'), JSON.stringify({ generation: 1, phase: 'ready' }), { mode: 0o644 })
    fs.chmodSync(path.join(dir, 'storage-generation'), 0o644)
    assert.throws(() => readStorageGeneration(dir, 'darwin'))
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('falhas de write/fsync/publicação deixam o commit ausente e permitem retomada', () => {
  for (const operation of ['writeFileSync', 'fsyncSync', 'linkSync'] as const) {
    for (const occurrence of [1, 2]) {
      const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-io-'))
      let count = 0
      try {
        const module = loadMainModule<typeof import('../main/fork-keychain/generation.ts')>('fork-keychain/generation', {
          'node:crypto': crypto, 'node:path': path, 'node:fs': { ...fs, [operation]: (...args: unknown[]) => {
            if (++count === occurrence) throw new Error('injected')
            return (fs[operation] as (...args: unknown[]) => unknown)(...args)
          } },
        }, { Buffer, process })
        assert.throws(() => module.prepareKeychain(dir, 'darwin', codec), { message: 'unavailable' })
        assert.equal(fs.existsSync(path.join(dir, 'storage-generation')), false)
        prepareKeychain(dir, 'darwin', codec)
        assert.equal(readStorageGeneration(dir, 'darwin'), 1)
      } finally { fs.rmSync(dir, { recursive: true, force: true }) }
    }
  }
})

test('geração 2 usa a mesma chave Donnabot, abre e não rebaixa o marcador', () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-compatible-'))
  try {
    prepareKeychain(dir, 'darwin', codec)
    fs.writeFileSync(path.join(dir, 'storage-generation'), JSON.stringify({ generation: 2, phase: 'ready' }))
    const before = snapshot(dir)
    assert.equal(readStorageGeneration(dir, 'darwin'), 2)
    prepareKeychain(dir, 'darwin', codec)
    assert.deepEqual(snapshot(dir), before)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

for (const marker of [
  { generation: 1, phase: 'pending', targetGeneration: 2, attempts: 1 },
  { generation: 1, phase: 'ready', suspended: true, future: { ignored: true } },
  { generation: 2, phase: 'ready', future: 'preservar byte a byte' },
  { generation: 3, phase: 'ready', future: 'nunca reescrever' },
]) test(`marcador extensível: ${JSON.stringify(marker)}`, () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-marker-'))
  try {
    prepareKeychain(dir, 'darwin', codec)
    fs.writeFileSync(path.join(dir, 'storage-generation'), JSON.stringify(marker))
    const before = snapshot(dir)
    if (marker.generation > 2) assert.throws(() => prepareKeychain(dir, 'darwin', codec))
    else {
      assert.equal(readStorageGeneration(dir, 'darwin'), marker.generation)
      prepareKeychain(dir, 'darwin', codec)
    }
    assert.deepEqual(snapshot(dir), before)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

for (const existing of [false, true]) test(`origem do canário persiste após interrupção: existing=${existing}`, () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-origin-'))
  try {
    if (existing) fs.writeFileSync(path.join(dir, 'legacy.bin'), 'synthetic ciphertext')
    prepareKeychain(dir, 'darwin', codec)
    const marker = path.join(dir, 'storage-generation')
    const expected = existing ? 'existing-data' : 'empty'
    assert.equal(JSON.parse(fs.readFileSync(marker, 'utf8')).canaryOrigin, expected)
    fs.unlinkSync(marker)
    prepareKeychain(dir, 'darwin', codec)
    assert.equal(JSON.parse(fs.readFileSync(marker, 'utf8')).canaryOrigin, expected)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('publicação não substitui destino criado na janela de corrida', () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-race-'))
  try {
    const module = loadMainModule<typeof import('../main/fork-keychain/generation.ts')>('fork-keychain/generation', {
      'node:crypto': crypto, 'node:path': path, 'node:fs': { ...fs, linkSync: (source: string, target: string) => {
        fs.writeFileSync(target, 'synthetic concurrent record', { flag: 'wx', mode: 0o600 })
        fs.linkSync(source, target)
      } },
    }, { Buffer, process })
    assert.throws(() => module.prepareKeychain(dir, 'darwin', codec))
    assert.equal(fs.readFileSync(path.join(dir, 'keychain-canary.bin'), 'utf8'), 'synthetic concurrent record')
    assert.equal(fs.existsSync(path.join(dir, 'storage-generation')), false)
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})

test('temporários anteriores são preservados ao preparar, falhar e tentar novamente', () => {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'mx82-tmp-'))
  try {
    const names = ['account.bin.fixture.tmp', 'keychain-canary.bin.fixture.tmp', 'storage-generation.fixture.tmp']
    for (const name of names) fs.writeFileSync(path.join(dir, name), `synthetic ${name}`)
    prepareKeychain(dir, 'darwin', codec)
    const before = snapshot(dir)
    assert.throws(() => prepareKeychain(dir, 'darwin', { ...codec, isEncryptionAvailable: () => false }))
    assert.deepEqual(snapshot(dir), before)
    prepareKeychain(dir, 'darwin', codec)
    assert.deepEqual(snapshot(dir), before)
    assert.equal(JSON.parse(before['storage-generation'].toString()).canaryOrigin, 'existing-data')
  } finally { fs.rmSync(dir, { recursive: true, force: true }) }
})
