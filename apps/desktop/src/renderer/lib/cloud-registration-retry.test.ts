import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import ts from 'typescript'
import { registerHooks } from 'node:module'
import test from 'node:test'

registerHooks({ resolve(specifier, context, next) {
  if (specifier === '@shared/fork-features') return next(new URL('../../shared/fork-features.ts', import.meta.url).href, context)
  if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
    const url = new URL(`${specifier}.ts`, context.parentURL)
    if (existsSync(url)) return next(url.href, context)
  }
  return next(specifier, context)
}, load(url, context, next) {
  if (url === new URL('../../shared/fork-features.ts', import.meta.url).href) return { format: 'module', shortCircuit: true, source: 'export const forkFeatures = { cloud: true, cloudAccess: false }' }
  if (url.endsWith('/renderer/store/cloud.ts')) return { format: 'module', shortCircuit: true, source: ts.transpileModule(readFileSync(new URL(url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2023 } }).outputText }
  return next(url, context)
} })

test('falha do registro inicial pode ser retomada por ação explícita sem novo login ou loop', async () => {
  let posts = 0
  let onAccountChange: (state: any) => void = () => {}
  const disk = new Map<string, string>()
  Object.assign(globalThis, {
    localStorage: { getItem: (k: string) => disk.get(k) ?? null, setItem: (k: string, v: string) => disk.set(k, v), removeItem: (k: string) => disk.delete(k) },
    window: { addEventListener: () => {}, codync: { platform: 'darwin', computerName: 'fixture',
      account: { state: async () => ({ ready: true, configured: true, busy: false, error: null, user: { userId: 'fixture-user', email: null, avatarURL: null }, cloudURL: 'https://cloud.example.test' }), onChange: (fn: typeof onAccountChange) => { onAccountChange = fn }, reconciliationState: async () => ({ kind: 'none' }) },
      cloud: { request: async (method: string, url: string) => {
        if (method === 'POST' && url === 'v1/devices') { posts++; return posts === 1 ? { ok: false, error: { status: 0, code: 'unknown', message: null } } : { ok: true, value: {} } }
        return { ok: true, value: { computers: [] } }
      } },
    } },
  })
  const { account } = await import('../store/account.ts')
  const { CloudModel } = await import('../store/cloud.ts')
  await account.start()
  let tick: () => void = () => {}
  const app = { subscribe: (fn: () => void) => { tick = fn }, managedStores: [], local: null, computers: [], setError: () => {} }
  const cloud = new CloudModel(app as never)
  tick(); await new Promise<void>((resolve) => setImmediate(resolve))
  assert.equal(posts, 1); assert.equal(cloud.deviceRegistration, 'failed')
  tick(); await new Promise<void>((resolve) => setImmediate(resolve)); assert.equal(posts, 1)
  assert.equal(await cloud.registerThisDevice(), true)
  assert.equal(posts, 2); assert.equal(cloud.deviceRegistration, 'registered')
  const stale = cloud.registerThisDevice()
  const state = { ready: true, configured: true, busy: false, error: null, cloudURL: 'https://cloud.example.test' }
  onAccountChange({ ...state, user: null })
  onAccountChange({ ...state, user: { userId: 'fixture-user', email: null, avatarURL: null } })
  assert.equal(await stale, false)
  assert.equal(posts, 2)

})
