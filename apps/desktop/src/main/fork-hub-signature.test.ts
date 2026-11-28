import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { verifyBundledHub } from './fork-hub-signature.ts'

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

  copyFileSync('/usr/bin/false', host)
  sign(host)
  assert.throws(() => verifyBundledHub(resources, 'donnabot-host'), /hub_bundle_signature_invalid/)
  rmSync(host)
  symlinkSync(join(contents, 'MacOS', 'fixture'), host)
  assert.throws(() => verifyBundledHub(resources, 'donnabot-host'), /hub_bundle_signature_invalid/)
})
