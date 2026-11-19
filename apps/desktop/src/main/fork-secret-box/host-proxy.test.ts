import assert from 'node:assert/strict'
import test from 'node:test'
import type { WebContents } from 'electron'
import { loadMainModule, settle } from '../../shared/test-support/main-module.ts'
import * as contexts from './contexts.ts'

function fixture(fetch: typeof globalThis.fetch) {
  type Handler = (event: { sender: WebContents }, ...args: any[]) => any
  const handlers = new Map<string, Handler>()
  const sent: unknown[][] = []
  const sender = { isDestroyed: () => false, send: (...args: unknown[]) => sent.push(args) } as unknown as WebContents
  const proxy = loadMainModule<typeof import('../host-proxy.ts')>('host-proxy', {
    electron: { ipcMain: {
      handle: (channel: string, handler: Handler) => handlers.set(channel, handler),
      on: (channel: string, handler: Handler) => handlers.set(channel, handler),
    } },
    './analytics': { noteAnalyticsSetting: () => {} },
    './fork-secret-box/contexts': contexts,
  }, { URL, fetch, AbortController, setTimeout, clearTimeout, TextDecoder })
  proxy.registerHostProxy()
  return {
    sender, sent,
    invoke: (channel: string, ...args: unknown[]) => handlers.get(channel)!({ sender }, ...args),
    invokeFrom: (sender: WebContents, channel: string, ...args: unknown[]) => handlers.get(channel)!({ sender }, ...args),
  }
}

test('handlers reais recusam privado antes de leitura, fetch e eco', async () => {
  let fetches = 0
  const f = fixture(async () => { fetches++; throw new Error('fetch inesperado') })
  contexts.markPrivateContext(f.sender)
  const poison = new Proxy({}, { get: () => { throw new Error('argumento lido') } })
  const result = await f.invoke('host:call', poison, poison, poison, poison, poison)
  assert.equal(JSON.stringify(result), JSON.stringify({ ok: false, error: { status: 0, message: 'Contexto indisponível.' } }))
  f.invoke('host:stream:open', poison, poison, poison)
  f.invoke('host:stream:close', poison)
  assert.equal(fetches, 0)
  assert.deepEqual(f.sent, [])
})

test('privado não substitui/fecha stream geral; fluxo geral preserva call e close', async () => {
  let streamSignal: AbortSignal | undefined
  let finish!: (response: Response) => void
  const calls: string[] = []
  const f = fixture(async (url, init) => {
    calls.push(String(url))
    if (String(url).endsWith('/api/ping')) return Response.json({ value: 'fixture' })
    streamSignal = init?.signal as AbortSignal
    return new Promise<Response>(resolve => { finish = resolve })
  })
  const result = await f.invoke('host:call', 'http://localhost:1', 'fixture', 'ping', {}, 1000)
  assert.equal(result.ok, true)
  assert.equal(result.value.value, 'fixture')
  f.invoke('host:stream:open', 'shared-id', 'http://localhost:1/events', 'fixture')
  const privateSender = { isDestroyed: () => false, send: () => assert.fail('eco privado') } as unknown as WebContents
  contexts.markPrivateContext(privateSender)
  f.invokeFrom(privateSender, 'host:stream:open', 'shared-id', 'http://localhost:1/blocked', 'fixture')
  f.invokeFrom(privateSender, 'host:stream:close', 'shared-id')
  assert.equal(streamSignal?.aborted, false)
  assert.deepEqual(calls, ['http://localhost:1/api/ping', 'http://localhost:1/events'])
  f.invoke('host:stream:close', 'shared-id')
  assert.equal(streamSignal?.aborted, true)
  contexts.markPrivateContext(f.sender)
  finish(new Response(null, { status: 500 }))
  await settle()
  assert.deepEqual(f.sent, [])
})

test('stream geral entrega data e end pelo handler real', async () => {
  const f = fixture(async () => new Response('data: fixture\n', { status: 200 }))
  f.invoke('host:stream:open', 'general-id', 'http://localhost:1/events', 'fixture')
  await settle()
  assert.equal(JSON.stringify(f.sent), JSON.stringify([
    ['host:stream:data', 'general-id', ['fixture']],
    ['host:stream:end', 'general-id', null],
  ]))
})

const ateOF = async (f: ReturnType<typeof fixture>) => {
  for (let i = 0; i < 50 && !f.sent.some((m) => m[0] === 'host:stream:end'); i++) await settle()
}
const lotes = (f: ReturnType<typeof fixture>) => f.sent.filter((m) => m[0] === 'host:stream:data').map((m) => m[2])

test('stream em lote: ordem preservada e nenhum evento perdido entre trechos', async () => {
  const encoder = new TextEncoder()
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode('data: a1\ndata: a2\n: ping\nevent: tick\n\n'))
      controller.enqueue(encoder.encode('data: b'))
      controller.enqueue(encoder.encode('1\n'))
      controller.enqueue(encoder.encode('data: c1\r\ndata: c2\n'))
      controller.close()
    },
  })
  const f = fixture(async () => new Response(body, { status: 200 }))
  f.invoke('host:stream:open', 'batch-id', 'http://localhost:1/events', 'fixture')
  await ateOF(f)
  assert.equal(JSON.stringify(lotes(f)), JSON.stringify([['a1', 'a2'], ['b1'], ['c1', 'c2']]))
  assert.deepEqual(f.sent.at(-1), ['host:stream:end', 'batch-id', null])
})

test('stream: lote já lido chega antes do end mesmo com erro no trecho seguinte', async () => {
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('data: x1\ndata: x2\n'))
      setTimeout(() => controller.error(new Error('quebrou')), 0)
    },
  })
  const f = fixture(async () => new Response(body, { status: 200 }))
  f.invoke('host:stream:open', 'err-id', 'http://localhost:1/events', 'fixture')
  await ateOF(f)
  assert.equal(JSON.stringify(lotes(f)), JSON.stringify([['x1', 'x2']]))
  assert.equal(f.sent.at(-1)?.[0], 'host:stream:end')
})

test('erro name_taken do HTTP atravessa o handler real de host:call', async () => {
  const f = fixture(async () => Response.json({ error: 'Nome já usado', code: 'name_taken' }, { status: 409 }))
  const result = await f.invoke('host:call', 'http://localhost:1', 'fixture', 'botGroupsEdit', { op: 'add', name: 'Grupo' }, 1000)
  assert.equal(JSON.stringify(result), JSON.stringify({ ok: false, error: { status: 409, message: 'Nome já usado', code: 'name_taken' } }))
})
