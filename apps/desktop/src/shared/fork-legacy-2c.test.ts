import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createContext, Script } from 'node:vm'
import ts from 'typescript'
import * as crypto from 'node:crypto'
import { loadMainModule } from './test-support/main-module.ts'

test('legado fechado não assina, invoca IPC, altera preferências ou chama o Hub', async () => {
  for (const cloudAccess of [false, true]) {
    let subscriptions = 0, prefs = 0, calls = 0, registrations = 0
    const flags = { cloud: false, cloudAccess }
    const source = readFileSync(new URL('../renderer/store/cloud.ts', import.meta.url), 'utf8')
    const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
    const exports: Record<string, unknown> = {}
    const mock: Record<string, unknown> = {
      '@shared/fork-features': { forkFeatures: flags },
      '../lib/auto-claim': { mayAutoClaim: () => false },
      '../lib/observable': { Observable: class { changed() {} } },
      '../lib/older-copy': { isOlderCopy: () => false },
      '../lib/prefs': { pref: () => { prefs++; return { get: () => [], set: () => { prefs++ } } } },
      './account': { account: { user: { userId: 'fixture' }, subscribe: () => { subscriptions++ } } },
    }
    const ctx = createContext({ exports, Promise, console, window: { codync: { cloud: { request: () => { calls++; throw new Error('unexpected') } } } }, require: (name: string) => { if (!(name in mock)) throw new Error(name); return mock[name] } })
    new Script(output).runInContext(ctx)
    const CloudModel = exports.CloudModel as new (app: unknown) => { claim(store: unknown): Promise<boolean>; refresh(): Promise<boolean>; registerThisDevice(): Promise<boolean>; setCloud(store: unknown, value: boolean): Promise<void>; unclaim(store: unknown): Promise<void>; removeFromAccount(id: string): Promise<boolean> }
    const app = { subscribe: () => { subscriptions++ }, setError: () => { calls++ } }
    const store = { computer: { id: 'fixture' }, client: {}, ready: async () => { calls++; throw new Error('unexpected') } }
    const cloud = new CloudModel(app)
    assert.equal(await cloud.claim(store), false)
    assert.equal(await cloud.refresh(), false)
    assert.equal(await cloud.registerThisDevice(), false)
    await cloud.setCloud(store, true); await cloud.unclaim(store)
    assert.equal(await cloud.removeFromAccount('fixture'), false)
    const main = loadMainModule<typeof import('../main/cloud.ts')>('cloud', {
      '../shared/fork-features': { forkFeatures: flags }, 'node:crypto': crypto,
      './fork-cloud/account-relay': { openAccountRelay: () => { calls++; throw new Error('unexpected') } },
      './fork-account/device-key': {}, electron: { ipcMain: { handle: () => { registrations++ } } },
    }, { Buffer })
    main.registerCloud({} as never)
    assert.equal((await main.openSealedRelay({} as never, {} as never)).kind, 'notEligible')
    assert.equal(subscriptions + prefs + calls + registrations, 0)
  }
})
