// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
const require = createRequire(import.meta.url)
const dev = require('../../tools/dev-builder.cjs')
const { AppInfo } = require('app-builder-lib/out/appInfo')
const { getWindowsInstallationDirName, getWindowsInstallationAppPackageName } = require('app-builder-lib/out/targets/targetUtil')
const { parse } = require('yaml')
const prod = parse(readFileSync(new URL('../../electron-builder.yml', import.meta.url), 'utf8'))
const metadata = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8'))

test('Linux packages and Windows installation resources have separate development names', () => {
  const mainInfo = new AppInfo({ config: prod, metadata })
  const devInfo = new AppInfo({ config: dev, metadata: { ...metadata, ...dev.extraMetadata } })
  assert.equal(mainInfo.linuxPackageName, 'donnabot-desktop')
  assert.equal(devInfo.linuxPackageName, 'donnabot-desktop-dev')
  assert.notEqual(devInfo.updaterCacheDirName, mainInfo.updaterCacheDirName)
  assert.notEqual(getWindowsInstallationDirName(devInfo, true), getWindowsInstallationDirName(mainInfo, true))
  assert.notEqual(getWindowsInstallationAppPackageName(devInfo.name), getWindowsInstallationAppPackageName(mainInfo.name))
  assert.equal(dev.extraMetadata.desktopName, 'donnabot-dev.desktop')
})

test('development packaging registers only development callbacks and background helpers', () => {
  assert.equal(dev.appId, 'app.donnabot.mac.dev')
  assert.equal(dev.productName, 'Mexerica Dev')
  assert.equal(dev.publish, null)
  assert.ok(new RegExp(dev.mac.signIgnore[0]).test('DonnabotDevScreen.app/Contents/Helpers/cua-driver'))
  assert.deepEqual(dev.protocols.flatMap((p: { schemes: string[] }) => p.schemes), ['donnabot-dev', 'app.donnabot.mac.dev'])
  assert.deepEqual(dev.mac.extraFiles.map((f: { to: string }) => f.to), [
    'Library/LoginItems/DonnabotDevScreen.app',
    'Library/LaunchAgents/app.donnabot.dev.screen.plist',
  ])
  assert.ok(dev.win.extraResources.some((f: { to: string }) => f.to === 'donnabot-dev-hostw.exe'))
  assert.ok(!dev.win.extraResources.some((f: { to: string }) => f.to === 'donnabot-hostw.exe'))
  assert.equal(dev.linux.executableName, 'donnabot-dev')
})

test('o app de produção tem identidade própria, sem colidir com o Codync instalado', () => {
  assert.equal(prod.appId, 'app.donnabot.mac')
  assert.equal(prod.productName, 'Mexerica')
  assert.equal(metadata.productName, 'Mexerica')
  assert.deepEqual(prod.protocols.flatMap((p: { schemes: string[] }) => p.schemes), ['donnabot', 'app.donnabot.mac'])
  assert.deepEqual(prod.mac.extraFiles.map((f: { to: string }) => f.to), [
    'Library/LoginItems/DonnabotScreen.app',
    'Library/LaunchAgents/app.donnabot.screen.plist',
  ])
  for (const config of [prod, dev]) {
    assert.ok(!JSON.stringify(config).includes('com.pokai'), 'com.pokai')
    assert.ok(!JSON.stringify(config).includes('codync-host'), 'codync-host')
  }
  assert.deepEqual(prod.mac.extraResources.map((f: { to: string }) => f.to), ['donnabot-host', 'codync-speech'])
})

test('nenhuma configuração de build herda feed ou chave do upstream', () => {
  for (const config of [prod, dev]) {
    assert.equal(config.publish, null)
    assert.equal(config.mac.extendInfo.SUPublicEDKey, undefined)
  }
})

test('o app local do Mac sai sem host, helper de tela, notarização ou identidade do upstream', () => {
  const local = require('../../tools/local-builder.cjs')
  assert.equal(local.appId, 'app.donnabot.mac')
  assert.deepEqual(local.mac.target.map((t: { target: string }) => t.target), ['dir'])
  assert.equal(local.mac.target[0].arch, require('../../tools/mac-arch.cjs')())
  assert.deepEqual(local.mac.extraResources.map((f: { to: string }) => f.to), ['codync-speech'])
  assert.equal(local.mac.extraFiles, undefined)
  assert.equal(local.mac.notarize, false)
  // DESK-R1-B1: o pacote local usa o Electron do lock; nenhum runtime experimental forçado.
  assert.equal(local.electronVersion, undefined)
  assert.equal(local.electronDist, undefined)
  assert.equal(local.mac.hardenedRuntime, false)
  assert.equal(local.mac.identity, process.env.DONNABOT_MAC_IDENTITY || '-')
  assert.equal(prod.mac.notarize, false)
  for (const text of [JSON.stringify(local), JSON.stringify(metadata.scripts)]) {
    for (const upstream of ['6KFK5J2LB4', '7FUM8A8H72', 'Pokai Lee', 'CSC_NAME']) assert.ok(!text.includes(upstream), upstream)
  }
})
