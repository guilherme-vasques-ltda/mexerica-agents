const assert = require('node:assert/strict')
const { createHash } = require('node:crypto')
const { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } = require('node:fs')
const { tmpdir } = require('node:os')
const { join } = require('node:path')
const { test } = require('node:test')
const { runInNewContext } = require('node:vm')
const official = require('./fork-official-builder.cjs')
const builder = require('./fork-official-rc-builder.cjs')
const { manifestPath, stageManifest } = require('./fork-official-rc.cjs')

const version = '0.0.1-rc.1'
const manifest = { profile: 'rc', version, appId: 'app.donnabot.mac.rc', name: 'Mexerica RC', scheme: 'mexerica-rc', packageName: 'mexerica-rc' }
const main = 'build/official/out/main'
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')

function fixture(t) {
  const projectDir = mkdtempSync(join(tmpdir(), 'desktop-official-rc-'))
  t.after(() => rmSync(projectDir, { recursive: true, force: true }))
  mkdirSync(join(projectDir, main), { recursive: true })
  return {
    projectDir,
    file: path => join(projectDir, path),
    setVersion: version => writeFileSync(join(projectDir, 'package.json'), JSON.stringify({ version })),
  }
}

test('RC config uses an explicit, stable install identity and no inherited service hook or feed', t => {
  const { projectDir, file, setVersion } = fixture(t)
  setVersion(version)
  stageManifest(projectDir)
  const firstManifest = readFileSync(file(manifestPath))
  assert.deepEqual(JSON.parse(firstManifest), manifest)
  const first = builder({ projectDir })
  assert.deepEqual(first.extraMetadata, { name: 'mexerica-rc', productName: 'Mexerica RC', version, mexericaProfile: 'rc', mexericaAppId: 'app.donnabot.mac.rc' })
  assert.deepEqual(first.protocols, [{ name: 'Mexerica RC', schemes: ['mexerica-rc'] }])
  assert.equal(first.directories.output, 'dist/official-rc')
  assert.equal(first.publish, null)
  assert.equal(first.productName, 'Mexerica RC')
  assert.equal(first.appId, 'app.donnabot.mac.rc')
  assert.equal(first.win.executableName, 'mexerica-rc')
  assert.equal(first.linux.executableName, 'mexerica-rc')
  assert.equal(first.linux.desktop.entry.StartupWMClass, 'Mexerica RC')
  assert.equal(first.deb.packageName, 'mexerica-rc')
  assert.equal(first.nsis.include, null)
  assert.equal(first.nsis.differentialPackage, false)
  assert.deepEqual(first.mac.target, [{ target: 'dmg', arch: 'universal' }])
  assert.deepEqual(first.mac.extraFiles, [])
  assert.deepEqual(first.mac.extraResources, [{ from: 'build/native/codync-speech', to: 'codync-speech' }])
  assert.deepEqual(first.win.extraResources, [{ from: 'resources/icon-tray.png', to: 'icon-tray.png' }])
  assert.deepEqual(first.linux.extraResources, [])
  assert.deepEqual(first.extraResources, [...official.extraResources, { from: manifestPath, to: 'rc-manifest.json' }])
  assert.deepEqual(first.electronFuses, official.electronFuses)
  setVersion('0.0.1-rc.2')
  stageManifest(projectDir)
  const nextManifest = readFileSync(file(manifestPath))
  assert.deepEqual(JSON.parse(nextManifest), { ...manifest, version: '0.0.1-rc.2' })
  assert.notEqual(sha256(nextManifest), sha256(firstManifest))
  const next = builder({ projectDir })
  assert.deepEqual(next.extraMetadata, { ...first.extraMetadata, version: '0.0.1-rc.2' })
  assert.deepEqual(next.protocols, first.protocols)
  assert.deepEqual(next.nsis, first.nsis)
  assert.deepEqual(next.linux, first.linux)
  assert.deepEqual(next.win, first.win)
  assert.deepEqual(next.mac, { ...first.mac, bundleVersion: '2' })
  assert.equal(next.buildNumber, '2')
})

test('RC rejects every version outside the strict candidate form', t => {
  const { projectDir, file, setVersion } = fixture(t)
  for (const bad of [null, '0.0.1', 'v0.0.1-rc.1', '0.0.1-rc.0', '0.0.1-rc.01', '01.0.1-rc.1', '0.0.1-RC.1', '0.0.1-rc.1.2', '0.0.1-rc.1+meta', '0.0.1-rc.1\n']) {
    setVersion(bad)
    assert.throws(() => stageManifest(projectDir), /X\.Y\.Z-rc\.N/)
    assert.throws(() => builder({ projectDir }), /X\.Y\.Z-rc\.N/)
    assert.throws(() => readFileSync(file(manifestPath)), { code: 'ENOENT' })
  }
})

test('RC preflight binds exact manifest bytes to a separate strict pin and still requires O1', async t => {
  const { projectDir, file, setVersion } = fixture(t)
  const { officialConfigHash } = await import('../src/shared/fork-official-account.ts')
  const account = {
    environment: 'main',
    clerkPublishableKey: `pk_live_${Buffer.from('clerk.example.invalid$').toString('base64')}`,
    clerkFrontendOrigin: 'https://clerk.example.invalid',
    clerkProxyOrigin: 'https://auth.example.invalid',
    clerkTokenOrigin: 'https://app.example.invalid',
  }
  setVersion(version)
  const { beforePack } = builder({ projectDir })
  const context = { packager: { projectDir }, electronPlatformName: 'linux' }
  const rejectPin = () => assert.rejects(beforePack(context), /RC manifest does not match/)
  const pin = file(`${main}/rc-manifest.sha256`)
  const config = file('build/official/account-config.json')
  const o1 = file(`${main}/official-config.sha256`)
  await rejectPin()
  stageManifest(projectDir)
  const exact = readFileSync(file(manifestPath))
  const digest = sha256(exact)
  await rejectPin()
  for (const bad of ['', '0'.repeat(64), digest.toUpperCase(), `${digest}\n`, `${digest} `, digest.slice(1), officialConfigHash(account)]) {
    writeFileSync(pin, bad)
    await rejectPin()
  }
  for (const tampered of ['{', JSON.stringify({ ...manifest, scheme: 'donnabot' }), JSON.stringify({ ...manifest, extra: true }), `${exact}\n`]) {
    writeFileSync(file(manifestPath), tampered)
    writeFileSync(pin, sha256(tampered))
    await rejectPin()
  }
  writeFileSync(pin, digest)
  rmSync(file(manifestPath))
  await rejectPin()
  stageManifest(projectDir)
  await assert.rejects(beforePack(context), /account configuration does not match/)
  writeFileSync(config, JSON.stringify(account))
  writeFileSync(o1, digest)
  await assert.rejects(beforePack(context), /account configuration does not match/)
  writeFileSync(o1, officialConfigHash(account))
  await beforePack(context)
  await beforePack({ ...context, electronPlatformName: 'win32' })
  await assert.rejects(beforePack({ ...context, electronPlatformName: 'darwin' }), /macOS desktop symbols/)
  mkdirSync(file('build/official/out/renderer/symbols'), { recursive: true })
  writeFileSync(file('build/official/out/renderer/symbols/manifest.json'), '{}')
  await assert.rejects(beforePack({ ...context, electronPlatformName: 'darwin' }), /codync-speech/)
  mkdirSync(file('build/native'))
  writeFileSync(file('build/native/codync-speech'), 'fixture', { mode: 0o700 })
  await beforePack({ ...context, electronPlatformName: 'darwin' })
})

test('dist selects RC explicitly and stages before Vite; stable keeps the original commands', () => {
  const script = readFileSync(join(__dirname, 'fork-official-dist.cjs'), 'utf8')
  for (const rc of [false, true]) {
    const events = []
    const run = (_node, args) => events.push([...args])
    const beforePack = () => ({ then: runBuilder => { events.push('beforePack'); runBuilder(); return { catch() {} } } })
    const stub = id => ({
      'node:child_process': { execFileSync: run },
      'node:fs': { rmSync: path => events.push(['rm', path]) },
      'node:path': require('node:path'),
      './fork-official-rc.cjs': { stageManifest: () => events.push('stageManifest') },
      './fork-official-builder.cjs': { beforePack },
      './fork-official-rc-builder.cjs': () => ({ beforePack }),
    })[id]
    stub.resolve = id => id
    runInNewContext(script, {
      require: stub,
      process: { argv: ['node', 'script', 'linux', ...(rc ? ['official-rc'] : [])], platform: 'linux', arch: 'x64', cwd: () => '.', env: {} },
    })
    assert.deepEqual(events, [
      ['tools/account-config.mjs', 'main', '--official'],
      ...(rc ? ['stageManifest'] : []),
      ['rm', 'build/official/out'],
      ['electron-vite/bin/electron-vite.js', 'build', '--mode', rc ? 'official-rc' : 'official', '--outDir', 'build/official/out'],
      'beforePack',
      ['electron-builder/out/cli/cli.js', '--linux', '--x64', '--config', `tools/fork-official${rc ? '-rc' : ''}-builder.cjs`, '--publish', 'never'],
    ])
  }
})
