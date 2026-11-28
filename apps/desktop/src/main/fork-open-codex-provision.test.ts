import assert from 'node:assert/strict'
import childProcess, { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs, { chmod, lstat, mkdir, mkdtemp, readFile, readdir, realpath, rm, symlink, writeFile } from 'node:fs/promises'
import { syncBuiltinESMExports } from 'node:module'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { Readable } from 'node:stream'
import test, { type TestContext } from 'node:test'
import { beforeDeviceLogin, createCodexPreparer, releaseURL } from './fork-open/fork-codex-provision.ts'

const binary = Buffer.from('Not an executable program: the provisioner must never run candidates.\n')
const member = 'codex-x86_64-apple-darwin'
const hash = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')
const signal = new AbortController().signal
const asset = { member, binaryHash: hash(binary), archiveHash: '' }
const missing = { code: 'ENOENT' }

async function fixture(t: TestContext) {
  const home = await mkdtemp(join(await realpath(tmpdir()), 'open-codex-'))
  t.after(() => rm(home, { recursive: true, force: true }))
  const profile = join(home, '.mexerica-open')
  await mkdir(profile, { mode: 0o700 })
  return { home, profile, file: join(profile, 'codex-local.json'), share: join(home, '.local/share') }
}

test('copies a pinned native binary into a private path and reuses its selection without more consent', async (t) => {
  const { home, profile, file, share } = await fixture(t)
  const native = join(home, '.local/bin/codex')
  await mkdir(dirname(native), { recursive: true, mode: 0o700 })
  await writeFile(native, binary, { mode: 0o700 })
  let approvals = 0
  const prepare = createCodexPreparer(profile, 'x64', async () => { approvals++; return true }, {
    asset, candidates: [{ path: native }], download: async () => assert.fail('no download expected'),
  })
  await prepare(signal)
  const config = JSON.parse(await readFile(file, 'utf8'))
  assert.deepEqual(config, { executable: config.executable, version: '0.164.0', sha256: asset.binaryHash })
  assert.equal(dirname(dirname(config.executable)), share)
  assert.deepEqual(await readFile(config.executable), binary)
  assert.equal((await lstat(file)).mode & 0o777, 0o600)
  assert.equal((await lstat(dirname(config.executable))).mode & 0o777, 0o700)
  assert.equal((await lstat(config.executable)).mode & 0o777, 0o700)
  await prepare(signal)
  assert.equal(approvals, 1)
  const modified = JSON.stringify({ ...config, version: '0.163.0' })
  await writeFile(file, modified)
  await assert.rejects(prepare(signal), { code: 'open_codex_selection' })
  assert.equal(await readFile(file, 'utf8'), modified)
})

test('rejects unmanaged symlinks and writable ancestors; accepts a pinned Homebrew target', async (t) => {
  const { home, profile } = await fixture(t)
  const prefix = join(home, 'brew')
  const cellar = join(prefix, 'Cellar/codex')
  const trusted = join(cellar, '0.164.0/bin/codex')
  const linked = join(prefix, 'bin/codex')
  const loose = join(home, 'codex')
  const writable = join(home, 'writable')
  await mkdir(dirname(trusted), { recursive: true, mode: 0o700 })
  await mkdir(dirname(linked), { recursive: true, mode: 0o700 })
  await mkdir(writable)
  await chmod(writable, 0o775)
  for (const path of [trusted, loose, join(writable, 'codex')]) await writeFile(path, binary, { mode: 0o700 })
  await symlink(loose, linked)
  const prepare = createCodexPreparer(profile, 'x64', async () => true, {
    asset, candidates: [{ path: linked, linkRoots: [cellar] }, { path: join(writable, 'codex') }],
    download: async () => { throw new Error('offline') },
  })
  await assert.rejects(prepare(signal), { code: 'open_codex_download' })
  await rm(linked)
  await symlink(trusted, linked)
  await prepare(signal)
})

test('keeps an existing manual selection when its own checksum and path are valid', async (t) => {
  const { home, profile, file } = await fixture(t)
  const executable = join(home, '.local/bin/codex')
  const manual = Buffer.from('Previously selected binary')
  await mkdir(dirname(executable), { recursive: true, mode: 0o700 })
  await writeFile(executable, manual, { mode: 0o700 })
  const selection = JSON.stringify({ executable, version: '0.164.0', sha256: hash(manual) })
  await writeFile(file, selection, { mode: 0o600 })
  const prepare = createCodexPreparer(profile, 'x64', async () => assert.fail('no consent expected'), { asset })
  await prepare(signal)
  assert.equal(await readFile(file, 'utf8'), selection)
  await writeFile(executable, binary)
  await assert.rejects(prepare(signal), { code: 'open_codex_selection' })
})

test('downloads only after consent; verifies both hashes and cleans up failures', async (t) => {
  const { home, profile, file, share } = await fixture(t)
  const archive = join(home, 'asset.tar.gz')
  await writeFile(join(home, member), binary)
  execFileSync('/usr/bin/tar', ['-czf', archive, '-C', home, member])
  const bytes = await readFile(archive)
  let downloads = 0
  let accepted = false
  const download = async () => { downloads++; return Readable.from([bytes]) }
  const prepare = (pins: typeof asset) => createCodexPreparer(profile, 'x64', async () => accepted,
    { asset: pins, candidates: [], download })
  await assert.rejects(prepare(asset)(signal), { code: 'open_codex_declined' })
  assert.equal(downloads, 0)
  await assert.rejects(lstat(share), missing)
  accepted = true
  for (const pins of [asset, { ...asset, archiveHash: hash(bytes), binaryHash: hash(Buffer.from('different')) }]) {
    await assert.rejects(prepare(pins)(signal), { code: 'open_codex_integrity' })
    assert.deepEqual(await readdir(share), [])
    await assert.rejects(lstat(file), missing)
  }
  await prepare({ ...asset, archiveHash: hash(bytes) })(signal)
  const { executable } = JSON.parse(await readFile(file, 'utf8'))
  assert.deepEqual(await readFile(executable), binary)
  assert.deepEqual(await readdir(dirname(executable)), ['codex'])
})

test('keeps TAR output when the extractor exits before the destination opens', { timeout: 5_000 }, async (t) => {
  const { home, profile, file } = await fixture(t)
  const archive = join(home, 'asset.tar.gz')
  await writeFile(join(home, member), binary)
  execFileSync('/usr/bin/tar', ['-czf', archive, '-C', home, member])
  const bytes = await readFile(archive)
  const spawn = childProcess.spawn
  const open = fs.open
  let extractorClosed: Promise<void> | undefined
  let delayed = false
  const spawnMock = t.mock.method(childProcess, 'spawn', (...args: Parameters<typeof spawn>) => {
    const child = spawn(...args)
    if (args[0] === '/usr/bin/tar') extractorClosed = new Promise((resolve) => child.once('close', () => resolve()))
    return child
  })
  const openMock = t.mock.method(fs, 'open', async (...args: Parameters<typeof open>) => {
    if (String(args[0]).endsWith('/codex')) {
      assert.ok(extractorClosed)
      await extractorClosed
      delayed = true
    }
    return open(...args)
  })
  syncBuiltinESMExports()
  try {
    const prepare = createCodexPreparer(profile, 'x64', async () => true, {
      candidates: [], asset: { ...asset, archiveHash: hash(bytes) },
      download: async () => Readable.from([bytes]),
    })
    await prepare(signal)
    assert.equal(delayed, true)
    const { executable } = JSON.parse(await readFile(file, 'utf8'))
    assert.deepEqual(await readFile(executable), binary)
  } finally {
    openMock.mock.restore()
    spawnMock.mock.restore()
    syncBuiltinESMExports()
  }
})

test('rejects an empty TAR member even when both hashes match', async (t) => {
  const { home, profile, file, share } = await fixture(t)
  const archive = join(home, 'asset.tar.gz')
  const empty = Buffer.alloc(0)
  await writeFile(join(home, member), empty)
  execFileSync('/usr/bin/tar', ['-czf', archive, '-C', home, member])
  const bytes = await readFile(archive)
  const prepare = createCodexPreparer(profile, 'x64', async () => true, {
    candidates: [], asset: { ...asset, archiveHash: hash(bytes), binaryHash: hash(empty) },
    download: async () => Readable.from([bytes]),
  })
  await assert.rejects(prepare(signal), { code: 'open_codex_integrity' })
  assert.deepEqual(await readdir(share), [])
  await assert.rejects(lstat(file), missing)
})

test('never overwrites a selection that appears after consent', async (t) => {
  const { home, profile, file, share } = await fixture(t)
  const native = join(home, 'codex')
  await writeFile(native, binary, { mode: 0o700 })
  const prepare = createCodexPreparer(profile, 'x64', async () => {
    await writeFile(file, 'other selection', { mode: 0o600 })
    return true
  }, { asset, candidates: [{ path: native }] })
  await assert.rejects(prepare(signal), { code: 'open_codex_selection' })
  assert.equal(await readFile(file, 'utf8'), 'other selection')
  assert.deepEqual(await readdir(share), [])
})

test('release URLs are limited to HTTPS GitHub asset hosts without credentials', () => {
  for (const url of ['https://github.com/a', 'https://release-assets.githubusercontent.com/a']) assert.ok(releaseURL(new URL(url)))
  for (const url of ['http://github.com', 'https://github.com.invalid', 'https://github.com:4000', 'https://u:p@github.com']) {
    assert.equal(releaseURL(new URL(url)), false)
  }
})

test('only a login request reaches provisioning and an aborted request cannot advance', async () => {
  const request = { action: 'login', engineAccountId: 'codex', clientRequestId: 'login', expectedRevision: 0 }
  await beforeDeviceLogin('donnaEngineSetupStatus', request, signal, async () => assert.fail())
  await beforeDeviceLogin('donnaEngineSetupStart', { ...request, action: 'diagnose' }, signal, async () => assert.fail())
  const controller = new AbortController()
  await assert.rejects(beforeDeviceLogin('donnaEngineSetupStart', request, controller.signal, async () => controller.abort()), { name: 'AbortError' })
})
