import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

const renderer = new URL('../', import.meta.url).pathname
const bundle = buildSync({
  stdin: { contents: `export { BotGroupsModel } from './store/bot-groups'; export { LoopbackTransport, HostClient } from './client/host-client'`, resolveDir: renderer, loader: 'ts' },
  tsconfig: new URL('../../../tsconfig.json', import.meta.url).pathname, bundle: true, platform: 'node', format: 'cjs', write: false,
}).outputFiles[0]!.text
const preload = buildSync({
  entryPoints: [new URL('../../preload/index.ts', import.meta.url).pathname],
  bundle: true, platform: 'node', format: 'cjs', external: ['electron'], write: false,
}).outputFiles[0]!.text

test('o envelope HTTP name_taken atravessa IPC e cliente até o modelo', async () => {
  const module = { exports: {} }
  let handler!: (event: unknown, ...args: unknown[]) => Promise<{ ok: boolean; error?: unknown; value?: unknown }>
  const main = loadMainModule<typeof import('../../main/host-proxy.ts')>('host-proxy', {
    electron: { ipcMain: { handle: (_name: string, fn: typeof handler) => { handler = fn }, on: () => {} } },
    './analytics': { noteAnalyticsSetting: () => {} },
    './fork-secret-box/contexts': { isGeneralContext: () => true },
  }, { URL, fetch: async () => Response.json({ error: 'duplicado', code: 'name_taken' }, { status: 409 }), AbortController, setTimeout, clearTimeout, TextDecoder })
  main.registerHostProxy()
  const window: { codync?: unknown } = {}
  const electron = {
    contextBridge: { exposeInMainWorld: (_name: string, bridge: unknown) => { window.codync = bridge } },
    ipcRenderer: { on: () => {}, sendSync: () => '', invoke: (_channel: string, ...args: unknown[]) => handler({ sender: {} }, ...args) },
  }
  runInNewContext(preload, { module: { exports: {} }, exports: {}, require: (name: string) => { if (name === 'electron') return electron; throw new Error(name) }, process })
  runInNewContext(bundle, { module, window, queueMicrotask, URL, URLSearchParams, setTimeout, clearTimeout, process })
  const api = module.exports as typeof import('../store/bot-groups') & typeof import('../client/host-client')
  const client = new api.HostClient(new api.LoopbackTransport('http://localhost', 'teste'), 'pessoal')
  const model = new api.BotGroupsModel(async () => client, () => 'pessoal')
  await assert.rejects(model.edit({ op: 'add', name: 'Grupo' }), (error: unknown) => (error as { code?: string }).code === 'name_taken')
  assert.equal(model.error?.code, 'name_taken')
  assert.equal(model.error?.message, 'duplicado')
})
