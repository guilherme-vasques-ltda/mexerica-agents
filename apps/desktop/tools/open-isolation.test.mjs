import assert from 'node:assert/strict'
import { chmodSync, linkSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { test } from 'node:test'
import { createRequire } from 'node:module'
import { environmentProfile } from '../src/shared/environment.ts'
import { PORTA_PRINCIPAL, PORTA_DEV } from '../src/main/fork-oauth-loopback.ts'
import { SANDBOX_PORTS } from '../src/shared/dev-sandbox.ts'
import { open, openPaths, isOpenAsset, bringToFront } from '../src/main/fork-open/isolation.ts'
import { traduzir } from '../src/renderer/lib/fork-i18n/index.ts'
import { SOURCE_LANGUAGE } from '../src/shared/fork-idioma.ts'

const require = createRequire(import.meta.url)

test('a Open usa apenas identificadores próprios, no pacote e em execução', () => {
  const config = require('./open-builder.cjs')
  assert.equal(config.appId, open.appId)
  assert.equal(config.productName, open.name)
  assert.equal(config.extraMetadata.productName, open.name)
  assert.equal(config.mac.extendInfo.CFBundleName, open.name)
  assert.deepEqual(config.protocols.flatMap(({ schemes }) => schemes), [open.scheme])
  assert.equal(config.mac.identity, null)
  assert.equal(config.mac.notarize, false)
  assert.equal(config.publish, null)
  assert.deepEqual(config.mac.extraResources, [{ from: 'build/native/codync-speech', to: 'codync-speech' }])
  assert.equal(config.mac.extraFiles, undefined)
  assert.equal(config.extraMetadata.main, 'out-open/main/open.js')
  for (const environment of ['main', 'dev']) {
    const existing = environmentProfile(environment)
    assert.notEqual(open.appId, existing.appId)
    assert.notEqual(open.name, existing.name)
    assert.notEqual(open.name, existing.cryptoName)
    assert.notEqual(open.userDataFolder, existing.userDataFolder)
    assert.notEqual(open.scheme, existing.scheme)
    assert.notEqual(open.dataFolder, existing.dataFolder)
    assert.notEqual(open.port, existing.port)
  }
  assert.ok(![PORTA_PRINCIPAL, PORTA_DEV].includes(open.port))
  assert.ok(open.port < SANDBOX_PORTS.first || open.port > SANDBOX_PORTS.last)
})

test('os caminhos da Open não aceitam atalhos para o perfil de outro app', (t) => {
  const home = mkdtempSync(join(realpathSync(tmpdir()), 'mexerica-open-'))
  t.after(() => rmSync(home, { recursive: true, force: true }))
  const appData = join(home, 'Application Support')
  mkdirSync(appData, { mode: 0o700 })
  const paths = openPaths(home, appData)
  assert.equal(paths.userData, join(appData, open.userDataFolder))
  assert.equal(paths.homeConfig, join(home, open.dataFolder))
  const personal = join(appData, 'Donnabot')
  mkdirSync(personal, { mode: 0o700 })
  const linkedAppData = join(home, 'Linked Application Support')
  symlinkSync(appData, linkedAppData, 'dir')
  assert.throws(() => openPaths(home, linkedAppData))
  symlinkSync(personal, join(paths.userData, 'Network'), 'dir')
  assert.throws(() => openPaths(home, appData))
  rmSync(join(paths.userData, 'Network'))
  const personalFile = join(personal, 'fixture')
  writeFileSync(personalFile, 'dados fictícios', { mode: 0o600 })
  linkSync(personalFile, join(paths.homeConfig, 'fixture'))
  assert.throws(() => openPaths(home, appData), { message: traduzir(SOURCE_LANGUAGE, 'marca.open.perfilArquivo') })
  rmSync(join(paths.homeConfig, 'fixture'))
  const unsafeFile = join(paths.userData, 'unsafe')
  writeFileSync(unsafeFile, '', { mode: 0o600 })
  chmodSync(unsafeFile, 0o622)
  assert.throws(() => openPaths(home, appData), { message: traduzir(SOURCE_LANGUAGE, 'marca.open.perfilMetadados') })
})

test('a segunda execução passa pela guarda com as travas da instância aberta, sem afrouxar o perfil', (t) => {
  const home = mkdtempSync(join(realpathSync(tmpdir()), 'mexerica-open-'))
  t.after(() => rmSync(home, { recursive: true, force: true }))
  const appData = join(home, 'Application Support')
  mkdirSync(appData, { mode: 0o700 })
  const { userData } = openPaths(home, appData)
  const personal = join(appData, 'Donnabot')
  mkdirSync(personal, { mode: 0o700 })
  symlinkSync('host-1234', join(userData, 'SingletonLock'))
  symlinkSync('5678', join(userData, 'SingletonCookie'))
  symlinkSync(join(home, 'org.chromium.Chromium.example', 'SingletonSocket'), join(userData, 'SingletonSocket'))
  assert.doesNotThrow(() => openPaths(home, appData))
  rmSync(join(userData, 'SingletonLock'))
  symlinkSync(personal, join(userData, 'SingletonLock'), 'dir')
  assert.doesNotThrow(() => openPaths(home, appData))
  const nested = join(userData, 'Network')
  mkdirSync(nested, { mode: 0o700 })
  symlinkSync('host-1234', join(nested, 'SingletonLock'))
  assert.throws(() => openPaths(home, appData))
  rmSync(nested, { recursive: true })
  symlinkSync(personal, join(userData, 'Cookies'), 'dir')
  assert.throws(() => openPaths(home, appData))
  rmSync(join(userData, 'Cookies'))
  const homeConfig = join(home, open.dataFolder)
  symlinkSync('host-1234', join(homeConfig, 'SingletonLock'))
  assert.throws(() => openPaths(home, appData))
  rmSync(join(homeConfig, 'SingletonLock'))
  const personalFile = join(personal, 'fixture')
  writeFileSync(personalFile, 'dados fictícios', { mode: 0o600 })
  rmSync(join(userData, 'SingletonCookie'))
  linkSync(personalFile, join(userData, 'SingletonCookie'))
  assert.throws(() => openPaths(home, appData), { message: traduzir(SOURCE_LANGUAGE, 'marca.open.perfilArquivo') })
})

test('a segunda execução traz a janela existente para a frente', () => {
  const calls = []
  const window = { isMinimized: () => true, restore: () => calls.push('restore'), show: () => calls.push('show'), focus: () => calls.push('focus') }
  bringToFront(window, () => calls.push('app'))
  assert.deepEqual(calls, ['restore', 'show', 'focus', 'app'])
  calls.length = 0
  window.isMinimized = () => false
  bringToFront(window, () => calls.push('app'))
  assert.deepEqual(calls, ['show', 'focus', 'app'])
})

test('a janela só pede arquivos empacotados, sem rede nem esquemas externos', () => {
  const root = '/workspace/Teste Open/resources/app.asar/out-open/renderer'
  assert.equal(isOpenAsset(root, pathToFileURL(join(root, 'assets/main.js')).href), true)
  assert.equal(isOpenAsset(root, pathToFileURL(root + '-extra/main.js').href), false)
  assert.equal(isOpenAsset(root, pathToFileURL('/home/example/.mexerica/settings.json').href), false)
  assert.equal(isOpenAsset(root, 'https://example.invalid/'), false)
  assert.equal(isOpenAsset(root, 'http://127.0.0.1:19422/'), false)
  assert.equal(isOpenAsset(root, 'ws://127.0.0.1:19423/'), false)
  assert.equal(isOpenAsset(root, 'donnabot://anything'), false)
})

test('a Open usa papéis nativos de edição sem ponte para a área de transferência', () => {
  const main = readFileSync(new URL('../src/main/fork-open/main.ts', import.meta.url), 'utf8')
  const submenu = /\{ role: 'editMenu', submenu: \[([^\]]+)\]/.exec(main)?.[1] ?? ''
  assert.deepEqual([...submenu.matchAll(/role: '([^']+)'/g)].map(([, role]) => role), ['undo', 'redo', 'cut', 'copy', 'paste', 'selectAll'])
  assert.doesNotMatch(main, /\bclipboard\b|['"]app:copy['"]/)
})
