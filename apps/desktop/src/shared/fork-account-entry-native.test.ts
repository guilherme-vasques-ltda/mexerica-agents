import assert from 'node:assert/strict'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

// Real native island consumer; OS/window creation is forbidden in this boundary proof.
test('a ilha retira o resumo ao perder a conta e recusa mensagens tardias sem reabrir', () => {
  let confirmed = false
  let windows = 0
  const handlers = new Map<string, (...args: any[]) => any>()
  const mod = loadMainModule<typeof import('../main/island.ts')>('island', {
    'node:child_process': { execFile: () => assert.fail('no OS probe') },
    electron: {
      BrowserWindow: class { constructor() { windows++; assert.fail('no window before account') } },
      globalShortcut: { unregister() {} },
      ipcMain: { on: (name: string, cb: (...args: any[]) => any) => handlers.set(name, cb), handle: (name: string, cb: (...args: any[]) => any) => handlers.set(name, cb) },
      screen: { on() {} },
    },
    '../shared/island': {},
    './host-controller': { prefs: { get: () => true } },
  }, { process: { platform: 'darwin', env: {} }, clearTimeout, setTimeout })
  const island = new mod.Island({ url: 'file:///fixture', preload: '/fixture', send: () => assert.fail('no command'), openApp() {}, changed() {}, canPresent: () => confirmed, chat: () => null, chatUrl: 'file:///fixture' })
  island.register()
  const empty = { bots: [], permissions: [], usage: [] }
  const stale = { bots: [{ ref: 'old-bot' }], permissions: [], usage: [] }
  const receive = handlers.get('island:summary')!
  const state = () => handlers.get('island:state')!().summary
  receive({}, stale)
  assert.equal(JSON.stringify(state()), JSON.stringify(empty))
  assert.equal(island.enabled, false)
  confirmed = true
  receive({}, stale)
  assert.equal(state(), stale)
  assert.equal(island.enabled, true)
  confirmed = false
  island.accountChanged()
  assert.equal(JSON.stringify(state()), JSON.stringify(empty))
  receive({}, stale)
  assert.equal(JSON.stringify(state()), JSON.stringify(empty))
  assert.equal(windows, 0)
})
