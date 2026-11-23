import assert from 'node:assert/strict'
import fs from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { DesktopSSHProfiles } from './fork-desktop-profile.ts'

test('only a missing leaf below verified parents can be prepared for first launch', { skip: process.platform === 'win32' }, () => {
  const parent = fs.mkdtempSync(join(fs.realpathSync(tmpdir()), 'ssh-first-launch-'))
  try {
    const root = join(parent, 'Donnabot')
    const file = join(root, 'ssh-profiles.json')
    const store = new DesktopSSHProfiles(file)
    assert.equal(fs.existsSync(root), false)
    assert.throws(() => store.load(false))
    assert.equal(fs.existsSync(root), false)
    const parentMode = fs.statSync(parent).mode
    assert.equal(store.load(true), null)
    assert.equal(fs.statSync(root).mode & 0o7777, 0o700)
    assert.equal(fs.statSync(parent).mode, parentMode)
    assert.deepEqual(fs.readdirSync(root), [])
    assert.equal(store.save([]).kind, 'saved')
    assert.equal(fs.statSync(file).mode & 0o7777, 0o600)
    assert.throws(() => new DesktopSSHProfiles(join(parent, 'missing', 'Donnabot', 'ssh-profiles.json')).load(true))
    assert.equal(fs.existsSync(join(parent, 'missing')), false)
  } finally { fs.rmSync(parent, { recursive: true, force: true }) }
})

test('a link, regular object or non-absence error does not permit root preparation', { skip: process.platform === 'win32' }, (t) => {
  const parent = fs.mkdtempSync(join(fs.realpathSync(tmpdir()), 'ssh-first-launch-'))
  try {
    const root = join(parent, 'Donnabot')
    const store = new DesktopSSHProfiles(join(root, 'ssh-profiles.json'))
    fs.symlinkSync(join(parent, 'absent'), root)
    assert.throws(() => store.load(true))
    assert.equal(fs.lstatSync(root).isSymbolicLink(), true)
    assert.equal(fs.existsSync(join(parent, 'absent')), false)
    fs.unlinkSync(root)
    fs.writeFileSync(root, 'fixture')
    assert.throws(() => store.load(true))
    assert.equal(fs.readFileSync(root, 'utf8'), 'fixture')
    fs.unlinkSync(root)
    const lstat = fs.lstatSync
    t.mock.method(fs, 'lstatSync', (path) => {
      if (path === root) throw Object.assign(new Error('fixture'), { code: 'EACCES' })
      return lstat(path)
    })
    assert.throws(() => store.load(true), { code: 'EACCES' })
    assert.equal(store.save([]).kind, 'refused')
    assert.deepEqual(fs.readdirSync(parent), [])
  } finally { fs.rmSync(parent, { recursive: true, force: true }) }
})
