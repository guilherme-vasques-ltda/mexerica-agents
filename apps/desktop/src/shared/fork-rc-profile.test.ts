import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join, posix as path } from 'node:path'
import { test } from 'node:test'
import { environmentProfile } from './environment.ts'
import { officialAccount, officialConfigHash } from './fork-official-account.ts'
import { rcDistribution, rcIdentity, rcManifestHash, rcPaths, validateRcBootstrap } from './fork-rc-profile.ts'
import { loadMainModule } from './test-support/main-module.ts'
import { hostUpdateFixture } from './test-support/host-update.ts'
import { infoArguments, knownHostsFiles, tunnelArguments } from '../main/ssh-command.ts'

type RcBuilder = (input: { projectDir: string }) => { productName: string; mac: { executableName?: string }; win: { executableName: string }; linux: { executableName: string } }

function fixture(version = '0.0.1-rc.1') {
  const manifest = Buffer.from(`${JSON.stringify({ profile: rcDistribution.profile, version, appId: rcDistribution.appId,
    name: rcDistribution.name, scheme: rcDistribution.scheme, packageName: rcDistribution.packageName })}\n`)
  const metadata = { name: 'mexerica-rc', productName: 'Mexerica RC', mexericaProfile: 'rc', mexericaAppId: 'app.donnabot.mac.rc', version }
  return {
    manifest, metadata, version, name: metadata.productName,
    compiledProfile: 'rc' as string | undefined, compiledManifestHash: rcManifestHash(manifest) as string | undefined,
    official: true, packaged: true, readMetadata: (): unknown => metadata, readManifest: () => manifest,
  }
}

test('persistent identity and paths distinguish RC from stable, dev and Open, including N → N+1', () => {
  assert.equal(validateRcBootstrap(fixture()), true)
  assert.equal(validateRcBootstrap(fixture('0.0.1-rc.2')), true)
  assert.deepEqual(rcPaths('/home/usuario', '/appdata'), {
    userData: '/appdata/Mexerica RC', sessionData: '/appdata/Mexerica RC/session', logs: '/appdata/Mexerica RC/logs',
    crashDumps: '/appdata/Mexerica RC/crashDumps', temp: '/appdata/Mexerica RC/tmp', sshData: '/home/usuario/.mexerica-rc',
  })
  const main = environmentProfile('main')
  const dev = environmentProfile('dev')
  const open = JSON.parse(readFileSync(new URL('../../config/open.json', import.meta.url), 'utf8'))
  assert.deepEqual([main.appId, main.name, main.scheme, main.dataFolder, main.port], ['app.donnabot.mac', 'Donnabot', 'donnabot', '.mexerica', 19422])
  assert.deepEqual([dev.appId, dev.name, dev.scheme, dev.dataFolder, dev.port], ['app.donnabot.mac.dev', 'Donnabot Dev', 'donnabot-dev', '.mexerica-dev', 19423])
  assert.deepEqual([open.appId, open.name, open.scheme, open.dataFolder, open.port], ['app.mexerica.open', 'Mexerica Open', 'mexerica-open', '.mexerica-open', 19524])
  const variants = [main, dev, { ...open, cryptoName: open.name }, rcIdentity]
  for (const key of ['appId', 'name', 'scheme', 'dataFolder', 'port', 'userDataFolder', 'cryptoName'] as const) {
    assert.equal(new Set(variants.map(profile => profile[key])).size, variants.length, key)
  }
  for (const profile of variants.slice(0, -1)) {
    assert.equal(validateRcBootstrap({
      ...fixture(), compiledProfile: undefined, compiledManifestHash: undefined, name: profile.name, version: '0.0.1',
      readMetadata: () => ({ name: 'other', productName: profile.name }), readManifest: () => assert.fail('non-RC read manifest'),
    }), false)
  }
})

const frontend = 'clerk.example.invalid'
const account = officialAccount({ environment: 'main', clerkPublishableKey: `pk_live_${Buffer.from(`${frontend}$`).toString('base64')}`,
  clerkFrontendOrigin: `https://${frontend}`, clerkProxyOrigin: 'https://proxy.fixture.example.invalid', clerkTokenOrigin: 'https://token.fixture.example.invalid' })!

function bootstrap(input = fixture(), options: { account?: unknown; env?: object; argv?: string[]; effects?: string[]; reads?: string[]; legacyHome?: string } = {}) {
  const reads = options.reads ?? [], effects = options.effects ?? []
  const rc = loadMainModule<typeof import('../main/environment.ts')>('environment', {
    './fork-idioma': {}, '../renderer/lib/fork-i18n/index.ts': { traduzir: (_language: string, key: string) => key },
    '../shared/fork-private-files': { privateDirectory: (name: string) => effects.push(`mkdir:${name}`) },
    '../shared/fork-data-home': { mexericaHome: () => options.legacyHome ?? assert.fail('RC touched the fallback data home') },
    '../shared/dev-sandbox': {}, '../shared/environment': { environmentProfile }, '../shared/fork-rc-profile': { rcIdentity, rcPaths, validateRcBootstrap },
    '../shared/fork-features': { officialDesktop: input.official }, '../shared/fork-official-account': { officialAccount, officialConfigHash },
    'node:path': path, 'node:os': { homedir: () => '/home/usuario' },
    'node:fs': { readFileSync: (name: string) => {
      reads.push(name)
      if (name === '/fixture/app.asar/package.json') return JSON.stringify(input.readMetadata())
      if (name === '/fixture/rc-manifest.json') return input.readManifest()
      if (name === '/fixture/account-config.json') return JSON.stringify(options.account ?? account)
      return assert.fail('unexpected data read')
    } },
    electron: { app: {
      isPackaged: input.packaged, getName: () => input.name, getVersion: () => input.version, getAppPath: () => '/fixture/app.asar',
      commandLine: {
        removeSwitch: (name: string) => effects.push(`removeSwitch:${name}`),
        appendSwitch: (name: string, value: string) => effects.push(`appendSwitch:${name}:${value}`),
      },
      getPath: (name: string) => {
        if (name === 'appData') return '/appdata'
        assert.equal(name, 'userData'); return '/appdata/Mexerica RC'
      },
      setName: (name: string) => effects.push(`name:${name}`),
      setPath: (name: string, value: string) => effects.push(`${name}:${value}`),
    } },
    './fork-keychain/generation': { readStorageGeneration: (name: string) => effects.push(`restore:${name}`) },
  }, {
    __MEXERICA_COMPILED_PROFILE__: input.compiledProfile, __MEXERICA_RC_MANIFEST_HASH__: input.compiledManifestHash,
    __MEXERICA_OFFICIAL_CONFIG_HASH__: officialConfigHash(account), __dirname: '/fixture/out/main',
    process: { resourcesPath: '/fixture', platform: 'linux', env: options.env ?? {}, argv: options.argv ?? [] },
  })
  return { rc, effects, reads }
}

test('RC chooses its incorporated profile, never a flag or env; N and N+1 restore identical paths after validation', () => {
  const one = bootstrap(fixture(), { env: { MEXERICA_PROFILE: 'main', MEXERICA_HOME: '/home/usuario/.mexerica', MEXERICA_PORT: '19422' }, argv: ['--profile=dev', '--user-data-dir=/appdata/Donnabot'] })
  const two = bootstrap(fixture('0.0.1-rc.2'))
  assert.equal(one.rc.isRc, true)
  assert.equal(two.rc.identity, one.rc.identity)
  assert.deepEqual(one.effects, two.effects)
  assert.equal(one.effects.at(-1), 'restore:/appdata/Mexerica RC')
  assert.ok(one.effects.includes('sessionData:/appdata/Mexerica RC/session'))
  assert.ok(one.effects.includes('removeSwitch:user-data-dir'))
  assert.ok(one.effects.includes('appendSwitch:user-data-dir:/appdata/Mexerica RC'))
  assert.deepEqual(one.reads, ['/fixture/app.asar/package.json', '/fixture/rc-manifest.json', '/fixture/account-config.json'])
  const stable = bootstrap({ ...fixture(), name: 'Mexerica', version: '0.0.1', compiledProfile: undefined, compiledManifestHash: undefined,
    readMetadata: () => ({ name: 'donnabot-desktop', productName: 'Mexerica', version: '0.0.1' }),
  }, { legacyHome: '/home/usuario/.mexerica', env: { MEXERICA_PROFILE: 'rc' }, argv: ['--profile=rc'] })
  assert.equal(stable.rc.isRc, false)
  assert.deepEqual(stable.rc.identity, environmentProfile('main'))
  assert.ok(!stable.reads.includes('/fixture/rc-manifest.json'))
})

test('incoherent RC profile, packaging, version or O1 configuration prevents first storage effect', () => {
  const clean = fixture()
  const invalid = [
    { ...clean, compiledProfile: undefined }, { ...clean, compiledManifestHash: undefined }, { ...clean, official: false },
    { ...clean, name: 'Mexerica' }, { ...clean, packaged: false },
    { ...clean, readMetadata: () => ({ ...clean.metadata, mexericaProfile: undefined }) },
    { ...clean, readMetadata: () => ({ ...clean.metadata, mexericaAppId: 'app.donnabot.mac' }) },
    { ...clean, readMetadata: () => ({ ...clean.metadata, productName: 'Mexerica' }) },
    { ...clean, readManifest: () => { throw new Error('missing') } },
    { ...clean, readManifest: () => Buffer.concat([clean.manifest, Buffer.from('\n')]) },
    { ...clean, compiledManifestHash: fixture('0.0.1-rc.2').compiledManifestHash, readManifest: fixture('0.0.1-rc.2').readManifest },
    ...['0.0.1', '0.0.1-rc.0', '0.0.1-rc.01', '0.0.1-rc.1+test', '0.0.1-rc.1\n'].map(fixture),
  ]
  for (const item of invalid) {
    const effects: string[] = [], reads: string[] = []
    assert.throws(() => bootstrap(item, { effects, reads, env: { MEXERICA_PROFILE: 'rc' }, argv: ['--profile=rc'] }), /invalid_rc_identity|missing/i)
    assert.deepEqual(effects, [])
    assert.ok(reads.every(name => name === '/fixture/app.asar/package.json' || name === '/fixture/rc-manifest.json'))
  }
  for (const badAccount of [{ ...account, clerkTokenOrigin: 'https://other.example.invalid' }, { ...account, clerkPublishableKey: 'pk_test_example' }]) {
    const effects: string[] = []
    assert.throws(() => bootstrap(clean, { effects, account: badAccount }), /marca\.rc\.contaInvalida/)
    assert.deepEqual(effects, [])
  }
})

test('only the RC callback is registered or delivered; a valid pending attempt still completes', async () => {
  const schemes: string[] = []
  let deliver: ((event: { preventDefault: () => void }, url: string) => void) | undefined
  let opened = 0
  const auth = loadMainModule<typeof import('../main/auth.ts')>('auth', {
    electron: { app: { setAsDefaultProtocolClient: (scheme: string) => schemes.push(scheme), on: (_name: string, handler: typeof deliver) => { deliver = handler } } },
    './environment': { identity: rcIdentity, isRc: true }, './navigation': { openExternalURL: async () => { opened++; return true } },
    '../shared/navigation-policy': { parseNavigationURL: (url: string) => new URL(url) }, './fork-account/recovery-diagnostic': { recoveryDiagnostic: () => {} },
  }, { process: { argv: [] }, URL, setTimeout, clearTimeout })
  auth.registerSchemes()
  assert.deepEqual(schemes, ['mexerica-rc'])
  const good = 'mexerica-rc://callback?attempt=fixture'
  const browser = 'https://login.example.invalid'
  const wrong = ['donnabot', 'donnabot-dev', 'app.donnabot.mac', 'app.donnabot.mac.rc', 'com.pokai.Codync'].map(scheme => `${scheme}://callback?attempt=fixture`)
  for (const url of wrong) {
    assert.equal(auth.urlFromArgv(['mexerica-rc', url]), undefined)
    await assert.rejects(auth.authenticate(browser, 'mexerica-rc', url), /invalid_rc_callback/)
  }
  assert.equal(opened, 0)
  const pending = auth.authenticate(browser, 'mexerica-rc', good)
  for (const url of wrong) deliver!({ preventDefault() {} }, url)
  deliver!({ preventDefault() {} }, good)
  assert.equal(await pending, good)
})

test('RC SSH uses only its own known-hosts cache and explicit lab home/port without installer commands', () => {
  const profile = { id: 'profile', name: '', host: 'lab.example.invalid', user: null, port: null, remotePort: rcIdentity.port, identityFile: null, computerId: null }
  const home = '/home/usuario', data = '/home/usuario/.mexerica-rc'
  assert.deepEqual(knownHostsFiles(home, data, true), [`${data}/ssh_known_hosts`])
  const info = infoArguments(profile, home, rcIdentity.dataFolder, data)
  const tunnel = tunnelArguments(profile, 32000, home, data, false, true)
  for (const args of [info, tunnel]) {
    assert.ok(args.includes(`UserKnownHostsFile="${data}/ssh_known_hosts"`))
    assert.ok(args.includes('StrictHostKeyChecking=yes'))
    assert.ok(args.includes('GlobalKnownHostsFile=none'))
    assert.ok(!args.join(' ').includes('/.ssh/known_hosts'))
  }
  assert.match(info.at(-1)!, /MEXERICA_HOME="\$HOME\/\.mexerica-rc".* info --json --port 19525/)
  assert.ok(tunnel.includes('127.0.0.1:32000:127.0.0.1:19525'))
})

test('the macOS configuration gives the bundle and executable the RC product name', (t) => {
  const require = createRequire(import.meta.url)
  const builder = require('../../tools/fork-official-rc-builder.cjs') as RcBuilder
  const projectDir = mkdtempSync(join(tmpdir(), 'desktop-rc-mac-config-'))
  t.after(() => rmSync(projectDir, { recursive: true, force: true }))
  writeFileSync(join(projectDir, 'package.json'), JSON.stringify({ version: '0.0.1-rc.1' }))
  const config = builder({ projectDir })
  assert.equal(config.productName, 'Mexerica RC')
  assert.equal(config.mac.executableName, undefined)
  assert.equal(config.win.executableName, 'mexerica-rc')
  assert.equal(config.linux.executableName, 'mexerica-rc')
  assert.equal(`${config.productName}.app/Contents/MacOS/${config.productName}`, 'Mexerica RC.app/Contents/MacOS/Mexerica RC')
})

test('RC does not touch local Hub state even with old local settings and a valid session', async () => {
  for (const platform of ['darwin', 'linux', 'win32']) {
    const { host, calls, probes, removed, reads, devPort } = hostUpdateFixture({ rc: true, platform, installed: true,
      localHost: true, macLocalHub: true, hubLocation: 'mac', restartMarker: true, devPort: '19422' })
    assert.equal(devPort, null)
    assert.equal(host.binary, null)
    host.start()
    await host.refresh()
    await host.install()
    host.restart()
    assert.equal(await host.uninstall(), true)
    await host.prepareForUpdate()
    host.resumeAfterCancelledUpdate()
    assert.equal(await host.isIdleForUpdate(), false)
    assert.equal(host.state.kind, 'disabled')
    assert.equal(host.local, null)
    assert.deepEqual([calls, probes, removed, reads], [[], [], [], []])
  }
})
