import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { forkFeatures } from './fork-features.ts'
import { updaterFixture } from './test-support/updater.ts'

test('Donnabot mantém os cortes da Fase 0 desligados', () => {
  assert.deepEqual(forkFeatures, { updates: false, analytics: false, cloud: false, cloudAccess: false, localHost: false, macLocalHub: false })
})

test('os configs de conta não herdam a nuvem nem o login do upstream', () => {
  for (const env of ['main', 'dev']) {
    const plist = readFileSync(new URL(`../../config/${env}.plist`, import.meta.url), 'utf8')
    assert.ok(!plist.includes('codync.dev'), `${env}: codync.dev`)
    assert.ok(!/pk_(test|live)_/.test(plist), `${env}: chave do Clerk`)
  }
})

test('o updater desligado ignora preferências antigas, IPC, eventos e ShipIt', async () => {
  const f = updaterFixture({
    updates: forkFeatures.updates,
    prefs: { updatesAutoCheck: true, updatesAutoDownload: true, updateRelaunchInFront: true,
      updateInstalling: { version: '2.11.3', at: Date.UTC(2026, 9, 9) } },
    shipIt: { request: false, job: 'idle' },
  })
  f.ipc.emit('updates:autoCheck', {}, true)
  f.ipc.emit('updates:autoDownload', {}, true)
  f.ipc.emit('updates:check')
  f.ipc.emit('updates:install')
  await f.emit('update-available', { version: '2.11.3' })
  await f.emit('update-downloaded', { version: '2.11.3' })
  assert.equal(f.yieldsToInstall(), false)
  assert.equal(f.updates.state.supported, false)
  assert.deepEqual(f.calls, { check: 0, download: 0, install: 0, prepare: 0, resume: 0, quit: 0 })
  assert.equal(f.updaterLoads, 0)
  assert.equal(f.timers.size, 0)
  assert.equal(f.shipIt.removed, 0)
  assert.deepEqual(f.opened, [])
  assert.deepEqual(f.spawned, [])
})

test('rejeição sem tratamento no main só registra no console e mantém o processo vivo', () => {
  const module = new URL('../main/error-reports.ts', import.meta.url).href
  const child = spawnSync(process.execPath, ['--experimental-strip-types', '--input-type=module', '-e', `
    const { registerErrorReports } = await import(${JSON.stringify(module)})
    registerErrorReports({ call() { throw new Error('envio proibido') } })
    Promise.reject(new Error('rejeição de teste'))
    setImmediate(() => console.log('main continua vivo'))
  `], { encoding: 'utf8', timeout: 10_000 })
  assert.equal(child.error, undefined)
  assert.equal(child.status, 0, child.stderr)
  assert.match(child.stdout, /main continua vivo/)
  assert.match(child.stderr, /Unhandled rejection in the main process: Error: rejeição de teste/)
})
