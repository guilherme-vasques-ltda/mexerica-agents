import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createContext, Script } from 'node:vm'
import test from 'node:test'
import ts from 'typescript'

test('unknown é HostError não transitório e o ramo de replay não reenvia', async () => {
  class HostError extends Error { readonly status: number; constructor(status: number, message: string) { super(message); this.status = status } get isTransient() { return this.status === 0 } }
  let calls = 0
  const exports: Record<string, unknown> = {}
  const source = readFileSync(new URL('../renderer/client/fork-relay-transport.ts', import.meta.url), 'utf8')
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 } }).outputText
  const context = createContext({ exports, require: (id: string) => { if (id === './host-client') return { HostError }; throw new Error(id) },
    window: { codync: { forkCloud: { call: async () => { calls++; return { kind: 'unknown', reason: 'timeout' } } } } }, URL, URLSearchParams, queueMicrotask })
  new Script(code).runInContext(context)
  const Transport = exports.ForkRelayTransport as new (id: string) => { call(method: string, body: unknown, timeout: number): Promise<unknown> }
  const transport = new Transport('fixture')
  for (;;) {
    try { await transport.call('send', {}, 1000); assert.fail('unknown não é sucesso') }
    catch (error) {
      if (error instanceof HostError && error.isTransient) continue // mesma condição de BotSync.withLink(replay=true)
      assert.ok(error instanceof HostError); assert.equal((error as HostError).isTransient, false); break
    }
  }
  assert.equal(calls, 1)
})

test('renderer encaminha /term/id ao canal do terminal sem replay', () => {
  class HostError extends Error {}
  const requests: unknown[] = []
  const exports: Record<string, unknown> = {}
  const source = readFileSync(new URL('../renderer/client/fork-relay-transport.ts', import.meta.url), 'utf8')
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 } }).outputText
  const context = createContext({ exports, require: () => ({ HostError }), URL, URLSearchParams, queueMicrotask,
    window: { codync: { forkCloud: { subscribeEvents: (_id: unknown, body: unknown) => { requests.push(body); return () => {} } } } } })
  new Script(code).runInContext(context)
  const Transport = exports.ForkRelayTransport as new (id: string) => { stream(path: string, onData: (s: string) => void, onEnd: () => void): () => void }
  const term = '11111111-2222-4333-8444-555555555555'
  const stop = new Transport('fixture').stream(`/term/${term}`, () => {}, () => {})
  assert.deepEqual(JSON.parse(JSON.stringify(requests)), [{ since: 0, client: 'login', kind: 'term', term }])
  stop()
})
