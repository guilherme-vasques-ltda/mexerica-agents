import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { once } from 'node:events'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { attestHubProcess, verifyBundledHub } from './fork-hub-signature.ts'

test('macOS validates an ad hoc bundle and refuses a substituted signed resource', { skip: process.platform !== 'darwin' }, (t) => {
  const temp = mkdtempSync(join(realpathSync(tmpdir()), 'mx-sign-'))
  t.after(() => rmSync(temp, { recursive: true, force: true }))
  const bundle = join(temp, 'Fixture.app')
  const contents = join(bundle, 'Contents')
  const resources = join(contents, 'Resources')
  mkdirSync(resources, { recursive: true, mode: 0o755 })
  mkdirSync(join(contents, 'MacOS'), { mode: 0o755 })
  writeFileSync(join(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>fixture</string>
<key>CFBundleIdentifier</key><string>invalid.example.mexerica.fixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>`)
  copyFileSync('/usr/bin/true', join(contents, 'MacOS', 'fixture'))
  const host = join(resources, 'donnabot-host')
  copyFileSync('/usr/bin/true', host)
  const sign = (path: string) => execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', path], { stdio: 'pipe' })
  sign(host)
  sign(bundle)
  const approved = verifyBundledHub(resources, 'donnabot-host')
  assert.equal(approved.binary, host)
  assert.match(approved.sha256, /^[0-9a-f]{64}$/)
  assert.match(approved.cdhash, /^[0-9a-f]{40}$/)

  copyFileSync('/usr/bin/false', host)
  sign(host)
  assert.throws(() => verifyBundledHub(resources, 'donnabot-host'), /hub_bundle_signature_invalid/)
  rmSync(host)
  symlinkSync(join(contents, 'MacOS', 'fixture'), host)
  assert.throws(() => verifyBundledHub(resources, 'donnabot-host'), /hub_bundle_signature_invalid/)
})

test('macOS verifica o CDHash da imagem viva e recusa outro processo ou PID sem imagem', { skip: process.platform !== 'darwin' }, async (t) => {
  const directory = mkdtempSync(join(realpathSync(tmpdir()), 'mx-attest-'))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  const start = async (name: string) => {
    const path = join(directory, name)
    copyFileSync('/bin/sleep', path)
    execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', '--identifier', 'invalid.example.' + name, path], { stdio: 'pipe' })
    const { stderr, status } = spawnSync('/usr/bin/codesign', ['--display', '--verbose=4', path], { encoding: 'utf8' })
    assert.equal(status, 0)
    const hash = /^CDHash=([0-9a-f]{40})$/m.exec(stderr)?.[1]
    assert.ok(hash)
    const child = spawn(path, ['20'], { env: {}, stdio: 'ignore' })
    const done = once(child, 'close')
    const stop = async () => { child.kill(); await done }
    t.after(stop)
    await once(child, 'spawn')
    assert.ok(child.pid)
    return { pid: child.pid, hash, stop }
  }
  const one = await start('first')
  const two = await start('second')
  assert.notEqual(one.hash, two.hash)
  assert.doesNotThrow(() => attestHubProcess(one.pid, one.hash))
  assert.throws(() => attestHubProcess(two.pid, one.hash), /hub_bundle_signature_invalid/)
  await one.stop()
  assert.throws(() => attestHubProcess(one.pid, one.hash), /hub_bundle_signature_invalid/)
})
