import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test, type TestContext } from 'node:test'

const req = createRequire(import.meta.url)
// DESK-R1-B1: o produtor experimental de runtime NÃO entra no pacote normal (PL1 desligado); fica standalone e recusa
// insumos não provados. O registrador abaixo só observa se o beforePack normal o invoca; o recibo de PIN roda real.
const realRuntimeReceipt = req('../../tools/personal-runtime-receipt.cjs')
const runtimeCalls: unknown[] = []
req.cache[req.resolve('../../tools/personal-runtime-receipt.cjs')]!.exports = async (context: unknown) => { runtimeCalls.push(context) }
const verifyBuild = req('../../tools/verify-build.cjs')
const project = join(import.meta.dirname, '../..')

function fixture(t: TestContext, platform = 'darwin') {
  const projectDir = mkdtempSync(join(tmpdir(), 'codync-symbol-check-'))
  t.after(() => rmSync(projectDir, { recursive: true, force: true }))
  mkdirSync(join(projectDir, 'resources'))
  writeFileSync(join(projectDir, 'resources/account-config.json'), JSON.stringify({ environment: 'dev' }))
  return { electronPlatformName: platform, packager: { projectDir, appInfo: { id: 'app.donnabot.mac.dev' } } }
}

test('macOS packaging requires the Apple logo manifest entry and every rendered weight', async (t) => {
  const context = fixture(t)
  await assert.rejects(verifyBuild(context), /ENOENT/)
  const symbols = join(context.packager.projectDir, 'out/renderer/symbols')
  mkdirSync(symbols, { recursive: true })
  writeFileSync(join(symbols, 'manifest.json'), '{}')
  await assert.rejects(verifyBuild(context), /missing the Apple logo/)
  writeFileSync(join(symbols, 'manifest.json'), JSON.stringify({ 'apple.logo': { w: 0.9, h: 1.15 } }))
  await assert.rejects(verifyBuild(context), /ENOENT/)
  for (const weight of ['light', 'regular', 'medium', 'semibold', 'bold', 'black']) {
    writeFileSync(join(symbols, `apple.logo.${weight}.png`), 'fixture')
  }
  await assert.doesNotReject(verifyBuild(context))
  // Conta da outra variante continua recusada.
  writeFileSync(join(context.packager.projectDir, 'resources/account-config.json'), JSON.stringify({ environment: 'main' }))
  await assert.rejects(verifyBuild(context), /Packaging dev with main account config/)
  assert.equal(runtimeCalls.length, 0, 'pacote normal não invoca o produtor experimental de runtime')
})

test('Linux and Windows packaging does not require Apple-only SF Symbols', async (t) => {
  for (const platform of ['linux', 'win32']) {
    await assert.doesNotReject(verifyBuild(fixture(t, platform)))
  }
})

test('runtime receipt real recusa empacotar sem contexto de runtime verificável (falha fechada)', async (t) => {
  for (const platform of ['darwin', 'linux']) {
    await assert.rejects(realRuntimeReceipt(fixture(t, platform)), /Não foi possível verificar o runtime deste pacote/)
  }
})

test('produtor de runtime standalone recusa Electron 44.9.1 e o lock atual antes de rede ou cache', async (t) => {
  const fetches: unknown[] = [], realFetch = globalThis.fetch
  globalThis.fetch = (async (...args: unknown[]) => { fetches.push(args); throw new Error('rede proibida no teste') }) as typeof fetch
  t.after(() => { globalThis.fetch = realFetch })
  const { Arch } = req('builder-util') as { Arch: Record<string, number> }
  const cache = join(project, 'build/personal-runtime')
  const before = existsSync(cache)
  for (const version of ['44.9.1', '44.5.1']) for (const platform of ['darwin', 'linux']) {
    const context = { electronPlatformName: platform, arch: Arch.arm64, packager: { projectDir: project, appInfo: { id: 'app.donnabot.mac' },
      info: { framework: { version } }, config: {} } }
    await assert.rejects(realRuntimeReceipt(context), /Não foi possível verificar o runtime deste pacote/, `${version} ${platform}`)
  }
  assert.equal(fetches.length, 0)
  assert.equal(existsSync(cache), before, 'nada criado em build/personal-runtime')
})
