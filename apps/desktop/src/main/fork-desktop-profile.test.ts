import assert from 'node:assert/strict'
import fs from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { test } from 'node:test'
import { DesktopSSHProfiles } from './fork-desktop-profile.ts'

const profile = { id: 'example', host: 'desktop.example.invalid', port: null, user: null, identityFile: null,
  remotePort: 19422, computerId: null, name: 'Example' }
const updated = { ...profile, name: 'Updated example' }

function fixture() {
  const dir = fs.mkdtempSync(join(fs.realpathSync(tmpdir()), 'desktop-profile-'))
  const root = join(dir, 'profile')
  fs.mkdirSync(root, { mode: 0o755 })
  return { root, file: join(root, 'ssh-profiles.json'), cleanup: () => fs.rmSync(dir, { recursive: true, force: true }) }
}

test('new storage and tightening preserve the profile and ancestor', { skip: process.platform === 'win32' }, () => {
  const { root, file, cleanup } = fixture()
  try {
    const rootMode = fs.statSync(root).mode
    const store = new DesktopSSHProfiles(file)
    assert.equal(store.load(), null)
    assert.equal(store.save([profile]).kind, 'saved')
    assert.equal(fs.statSync(file).mode & 0o777, 0o600)
    const original = '[\n  {"id":"example","host":"desktop.example.invalid","user":null,"port":null,"remotePort":19422,"identityFile":null}\n]\n'
    fs.writeFileSync(file, original)
    fs.chmodSync(file, 0o644)
    assert.deepEqual(store.load(), [{ ...profile, name: '' }])
    assert.equal(fs.statSync(root).mode, rootMode)
    assert.equal(fs.statSync(file).mode & 0o777, 0o600)
    assert.equal(fs.readFileSync(file, 'utf8'), original)
  } finally { cleanup() }
})

test('invalid contents or metadata block mutation without replacing the object', { skip: process.platform === 'win32' }, () => {
  const { root, file, cleanup } = fixture()
  try {
    const store = new DesktopSSHProfiles(file)
    for (const bytes of ['{', '[null]', JSON.stringify([profile, profile])]) {
      fs.writeFileSync(file, bytes, { mode: 0o600 })
      assert.throws(() => store.load())
      assert.equal(store.save([profile]).kind, 'refused')
      assert.equal(fs.readFileSync(file, 'utf8'), bytes)
    }
    fs.unlinkSync(file)
    const other = join(root, 'fixture.json')
    fs.writeFileSync(other, '[]')
    fs.symlinkSync(other, file)
    assert.throws(() => store.load())
    assert.equal(store.save([profile]).kind, 'refused')
    assert.equal(fs.readFileSync(other, 'utf8'), '[]')
    fs.unlinkSync(file)
    fs.linkSync(other, file)
    assert.throws(() => store.load())
    assert.equal(store.save([profile]).kind, 'refused')
    assert.equal(fs.statSync(other).nlink, 2)
    fs.unlinkSync(file)
    fs.mkdirSync(file)
    assert.throws(() => store.load())
    assert.ok(fs.statSync(file).isDirectory())
  } finally { cleanup() }
})

test('a changed or newly appeared profile is not replaced', { skip: process.platform === 'win32' }, () => {
  const { root, file, cleanup } = fixture()
  try {
    const store = new DesktopSSHProfiles(file)
    assert.equal(store.load(), null)
    const original = JSON.stringify([profile])
    fs.writeFileSync(file, original, { mode: 0o600 })
    assert.equal(store.save([updated]).kind, 'refused')
    assert.deepEqual(store.load(), [profile])
    fs.writeFileSync(file, '[]')
    assert.equal(store.save([updated]).kind, 'refused')
    assert.equal(fs.readFileSync(file, 'utf8'), '[]')
    assert.deepEqual(fs.readdirSync(root), ['ssh-profiles.json'])
  } finally { cleanup() }
})

test('profile storage refuses a linked root and unverified file owner', { skip: process.platform === 'win32' }, (t) => {
  const { root, file, cleanup } = fixture()
  try {
    const other = join(root, '..', 'fixture')
    fs.mkdirSync(other)
    const originalMode = fs.statSync(other).mode
    fs.rmdirSync(root)
    fs.symlinkSync(other, root)
    assert.throws(() => new DesktopSSHProfiles(file).load())
    assert.equal(fs.statSync(other).mode, originalMode)
    fs.unlinkSync(root)
    fs.mkdirSync(root, { mode: 0o755 })
    fs.writeFileSync(file, '[]', { mode: 0o600 })
    const stat = fs.lstatSync
    t.mock.method(fs, 'lstatSync', (path: fs.PathLike) => {
      const meta = stat(path)
      if (path === file) meta.uid++
      return meta
    })
    const store = new DesktopSSHProfiles(file)
    assert.throws(() => store.load())
    assert.equal(store.save([profile]).kind, 'refused')
    assert.equal(fs.readFileSync(file, 'utf8'), '[]')
  } finally { cleanup() }
})

test('sync errors before and after publication have different results', { skip: process.platform === 'win32' }, (t) => {
  const { root, file, cleanup } = fixture()
  try {
    const store = new DesktopSSHProfiles(file)
    assert.equal(store.load(), null)
    assert.equal(store.save([profile]).kind, 'saved')
    const realSync = fs.fsyncSync
    let failDirectory = false
    let publications = 0
    const rename = fs.renameSync
    t.mock.method(fs, 'renameSync', (src: fs.PathLike, dst: fs.PathLike) => { publications++; rename(src, dst) })
    t.mock.method(fs, 'fsyncSync', (fd: number) => {
      if (fs.fstatSync(fd).isDirectory() === failDirectory) throw new Error('synthetic sync failure')
      realSync(fd)
    })
    assert.equal(store.save([updated]).kind, 'refused')
    assert.equal(fs.readFileSync(file, 'utf8'), JSON.stringify([profile]))
    assert.equal(publications, 0)
    failDirectory = true
    assert.deepEqual(store.save([updated]), { kind: 'uncertain', profiles: [updated] })
    assert.equal(fs.readFileSync(file, 'utf8'), JSON.stringify([updated]))
    assert.equal(publications, 1)
    assert.deepEqual(fs.readdirSync(root), ['ssh-profiles.json'])
  } finally { cleanup() }
})

test('first publication reconciles a directory sync error without replacing the file again', { skip: process.platform === 'win32' }, (t) => {
  const { root, file, cleanup } = fixture()
  try {
    const store = new DesktopSSHProfiles(file)
    assert.equal(store.load(), null)
    const sync = fs.fsyncSync
    t.mock.method(fs, 'fsyncSync', (fd: number) => {
      if (fs.fstatSync(fd).isDirectory()) throw new Error('synthetic sync failure')
      sync(fd)
    })
    assert.deepEqual(store.save([profile]), { kind: 'uncertain', profiles: [profile] })
    assert.equal(fs.statSync(file).nlink, 1)
    assert.deepEqual(fs.readdirSync(root), ['ssh-profiles.json'])
    assert.equal(fs.readFileSync(file, 'utf8'), JSON.stringify([profile]))
  } finally { cleanup() }
})

test('synthetic macOS ancestors allow sticky root and refuse writable or linked parents', { skip: process.platform === 'win32' }, (t) => {
  const { root, cleanup } = fixture()
  try {
    const shared = join(root, 'private', 'var', 'folders')
    const parent = join(shared, 'user', 'Library', 'Application Support')
    const profileRoot = join(parent, 'Donnabot')
    fs.mkdirSync(profileRoot, { recursive: true, mode: 0o755 })
    fs.chmodSync(shared, 0o1777)
    const lstat = fs.lstatSync
    const fstat = fs.fstatSync
    const sharedInode = lstat(shared).ino
    t.mock.method(fs, 'lstatSync', (path: fs.PathLike) => {
      const meta = lstat(path)
      if (path === shared) meta.uid = 0
      return meta
    })
    t.mock.method(fs, 'fstatSync', (fd: number) => {
      const meta = fstat(fd)
      if (meta.ino === sharedInode) meta.uid = 0
      return meta
    })
    const store = new DesktopSSHProfiles(join(profileRoot, 'ssh-profiles.json'))
    assert.equal(store.load(), null)
    fs.chmodSync(shared, 0o777)
    assert.throws(() => store.load())
    assert.equal(store.save([profile]).kind, 'refused')
    fs.chmodSync(shared, 0o1777)
    const actual = `${parent}.fixture`
    fs.renameSync(parent, actual)
    fs.symlinkSync(actual, parent)
    assert.throws(() => store.load())
    assert.equal(store.save([profile]).kind, 'refused')
    assert.deepEqual(fs.readdirSync(join(actual, 'Donnabot')), [])
    assert.equal(lstat(shared).mode & 0o7777, 0o1777)
  } finally { cleanup() }
})

test('a transient cleanup failure reconciles the first publication without rewriting it', { skip: process.platform === 'win32' }, (t) => {
  const { root, file, cleanup } = fixture()
  try {
    const store = new DesktopSSHProfiles(file)
    assert.equal(store.load(), null)
    const unlink = fs.unlinkSync
    let attempts = 0
    t.mock.method(fs, 'unlinkSync', (path: fs.PathLike) => {
      if (++attempts === 1) throw new Error('synthetic cleanup failure')
      unlink(path)
    })
    assert.deepEqual(store.save([profile]), { kind: 'uncertain', profiles: [profile] })
    assert.equal(attempts, 2)
    assert.deepEqual(fs.readdirSync(root), ['ssh-profiles.json'])
    assert.equal(fs.statSync(file).nlink, 1)
    assert.deepEqual(new DesktopSSHProfiles(file).load(), [profile])
  } finally { cleanup() }
})

test('load recovers a verified first publication then validates the original contents', { skip: process.platform === 'win32' }, () => {
  const { root, file, cleanup } = fixture()
  try {
    const temp = `${file}.00000000-0000-4000-8000-000000000001.tmp`
    const bytes = JSON.stringify([profile], null, 2)
    fs.writeFileSync(temp, bytes, { mode: 0o600 })
    fs.linkSync(temp, file)
    const before = fs.lstatSync(file)
    assert.deepEqual(new DesktopSSHProfiles(file).load(), [profile])
    assert.deepEqual(fs.readdirSync(root), ['ssh-profiles.json'])
    const after = fs.lstatSync(file)
    assert.equal(after.ino, before.ino)
    assert.equal(after.nlink, 1)
    assert.equal(fs.readFileSync(file, 'utf8'), bytes)
  } finally { cleanup() }
})

test('ambiguous first-publication names and identities stay untouched', { skip: process.platform === 'win32' }, () => {
  for (const scenario of ['two names', 'unrelated file', 'three links', 'wrong name', 'symlink', 'wrong mode']) {
    const { root, file, cleanup } = fixture()
    try {
      const candidate = `${file}.00000000-0000-4000-8000-000000000001.tmp`
      const second = `${file}.00000000-0000-4000-8000-000000000002.tmp`
      const alternate = `${file}.fixture.tmp`
      fs.writeFileSync(file, '[]', { mode: scenario === 'wrong mode' ? 0o640 : 0o600 })
      if (scenario === 'wrong mode') fs.chmodSync(file, 0o640)
      if (['unrelated file', 'wrong name', 'symlink'].includes(scenario)) fs.linkSync(file, alternate)
      else fs.linkSync(file, candidate)
      if (scenario === 'two names') fs.writeFileSync(second, '[]', { mode: 0o600 })
      if (scenario === 'unrelated file') fs.writeFileSync(candidate, '[]', { mode: 0o600 })
      if (scenario === 'three links') fs.linkSync(file, alternate)
      if (scenario === 'symlink') fs.symlinkSync(file, candidate)
      const names = fs.readdirSync(root)
      const original = fs.lstatSync(file)
      const store = new DesktopSSHProfiles(file)
      assert.throws(() => store.load(), scenario)
      assert.equal(store.save([profile]).kind, 'refused', scenario)
      assert.deepEqual(fs.readdirSync(root), names, scenario)
      assert.equal(fs.lstatSync(file).nlink, original.nlink, scenario)
      assert.equal(fs.lstatSync(file).mode, original.mode, scenario)
      assert.equal(fs.readFileSync(file, 'utf8'), '[]', scenario)
    } finally { cleanup() }
  }
})
