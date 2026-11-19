import assert from 'node:assert/strict'
import test from 'node:test'
import * as fs from 'node:fs'
import * as path from 'node:path'
import * as crypto from 'node:crypto'
import { loadMainModule } from './test-support/main-module.ts'
import { sandboxLabel } from './dev-sandbox.ts'

function boot(environment: 'main' | 'dev', packaged: boolean, sandbox: string | undefined, generation = 0, rename = false) {
  let name = '', userData = '', reads = 0, exited = false
  const appData = '/synthetic/user/Library/Application Support'
  loadMainModule('environment', {
    electron: { app: { isPackaged: packaged, setName: (n: string) => { name = n }, setPath: (_: string, p: string) => { userData = p }, getPath: (key: string) => key === 'appData' ? appData : userData, exit: () => { exited = true } }, dialog: { showErrorBox: () => {} } },
    'node:fs': { ...fs, readFileSync: () => JSON.stringify({ environment }) }, 'node:path': path, 'node:crypto': crypto, 'node:os': { homedir: () => '/synthetic/user' },
    '../shared/fork-private-files': { privateDirectory: () => {} },
    '../shared/fork-data-home': { mexericaHome: () => sandbox ?? `/synthetic/user/.mexerica${environment === 'dev' ? '-dev' : ''}` },
    ...(rename ? { '../shared/environment': { environmentProfile: () => ({ environment, name: 'Future visible name', cryptoName: environment === 'dev' ? 'Donnabot Dev' : 'Donnabot', userDataFolder: environment === 'dev' ? 'Donnabot Dev' : 'Donnabot' }) } } : {}),
    './fork-keychain/generation': { keychainFailure: () => 'unsupported', readStorageGeneration: (dir: string) => { reads++; assert.equal(dir, userData); if (generation > 2) throw new Error('future'); return generation } },
  }, { __dirname: '/fixture/main', Buffer, process: { platform: 'darwin', resourcesPath: '/fixture', env: sandbox ? { MEXERICA_HOME: sandbox } : {} } })
  return { name, userData, reads, exited }
}
test('environment real mantém nome e userData byte a byte em produção, Dev e sandbox', () => {
  for (const environment of ['main', 'dev'] as const) for (const packaged of [true, false]) for (const sandbox of [undefined, '/synthetic/user/.mexerica-dev-café']) {
    const activeSandbox = environment === 'dev' && !packaged ? sandbox : undefined
    const legacy = environment === 'dev' ? 'Donnabot Dev' : 'Donnabot'
    for (const rename of [false, true]) {
      const actual = boot(environment, packaged, sandbox, 0, rename)
      assert.deepEqual(Buffer.from(actual.userData), Buffer.from(activeSandbox ? path.join(activeSandbox, 'desktop') : path.join('/synthetic/user/Library/Application Support', legacy)))
      assert.equal(actual.name, activeSandbox ? sandboxLabel(legacy, activeSandbox) : legacy)
      assert.equal(actual.reads, 1); assert.equal(actual.exited, false)
    }
  }
})
test('geração superior interrompe a própria avaliação síncrona de environment', () => {
  assert.throws(() => boot('main', true, undefined, 3), /future/)
})
