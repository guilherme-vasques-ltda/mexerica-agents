import test from 'node:test'
import assert from 'node:assert/strict'
import { SpaceClient } from './fork-spaces-client.ts'
import { normalizeBot } from './models.ts'
import { buildSync } from 'esbuild'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'

test('wrapper fixa Hub/Space e recusa resposta que terminou após descarte', async () => {
  let answer!: (value: unknown) => void
  const calls: unknown[] = []
  const client = new SpaceClient('fixture-hub', 'space-a', {
    call: async (_method: string, body: unknown) => {
      calls.push(body)
      return new Promise((resolve) => { answer = resolve })
    },
  })
  const request = client.call('history', { botId: 'fixture-bot' })
  assert.deepEqual(calls, [{ botId: 'fixture-bot', hubId: 'fixture-hub', spaceId: 'space-a' }])
  client.dispose()
  answer({ entries: [] })
  await assert.rejects(request, /stale_space_context/)
  await assert.rejects(client.call('history', { spaceId: 'space-b' }), /stale_space_context/)
})

test('normalização conserva Space persistido e não inventa destino de cliente legado', () => {
  assert.equal(normalizeBot({ id: 'bot', spaceId: 'space-b' }).spaceId, 'space-b')
  assert.equal(normalizeBot({ id: 'legacy' }).spaceId, undefined)
})

test('cliente real fixa scope nas chamadas e stream, preservando eventos de Spaces', async () => {
  const source = buildSync({ entryPoints: [new URL('../renderer/client/host-client.ts', import.meta.url).pathname], bundle: true, platform: 'node', format: 'cjs', write: false }).outputFiles[0]!.text
  const module = { exports: {} }
  runInNewContext(source, { module, exports: module.exports, require: createRequire(import.meta.url), URLSearchParams })
  const { HostClient, parseEvent } = module.exports as typeof import('../renderer/client/host-client.ts')
  let body: unknown
  let path = ''
  const client = new HostClient({ async call(_method, value) { body = value; return {} }, stream(value) { path = value; return () => {} } }, 'space-a')
  await client.call('history', { botId: 'fixture' })
  assert.equal((body as { spaceId: string }).spaceId, 'space-a')
  client.events(0, 'fixture', () => {}, () => {})
  assert.equal(new URL(path, 'http://fixture.invalid').searchParams.get('spaceId'), 'space-a')
  const event = parseEvent(JSON.stringify({ type: 'conversation', spaceId: 'space-a', rev: 3, conversation: { id: 'chat-a', spaceId: 'space-a' } }))
  assert.equal(event?.type, 'conversation')
})

test('mirror real guarda caches e rascunhos separados por Hub e Space', () => {
  const source = buildSync({ entryPoints: [new URL('../renderer/store/bot-mirror.ts', import.meta.url).pathname], bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react'] }).outputFiles[0]!.text
  const storage = new Map<string, string>()
  const localStorage = { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value), removeItem: (key: string) => storage.delete(key) }
  const module = { exports: {} }
  runInNewContext(source, { module, exports: module.exports, require: createRequire(import.meta.url), localStorage, setTimeout, clearTimeout })
  const { BotMirror } = module.exports as typeof import('../renderer/store/bot-mirror.ts')
  const computer = { id: 'fixture-hub', name: 'Fixture' } as import('./models.ts').Computer
  const transport = { call: async () => ({}), stream: () => () => {} }
  const a = new BotMirror(computer, false, () => transport, 'fixture', 'local', 'space-a', 'fixture-hub-identity')
  const b = new BotMirror(computer, false, () => transport, 'fixture', 'local', 'space-b', 'fixture-hub-identity')
  a.bots.set('a', normalizeBot({ id: 'a', spaceId: 'space-a' }))
  b.bots.set('b', normalizeBot({ id: 'b', spaceId: 'space-b' }))
  a.saveCache(); b.saveCache()
  assert.equal([...storage.keys()].filter((key) => key.startsWith('codync-mirror')).length, 2)
  const again = new BotMirror(computer, false, () => transport, 'fixture', 'local', 'space-a', 'fixture-hub-identity')
  assert.equal(again.bots.has('a'), true)
  assert.equal(again.bots.has('b'), false)
})


test('conversa fixa destino e rejeita retorno tardio após outra seleção', async () => {
  let complete!: (value: unknown) => void
  const space = new SpaceClient('fixture', 'space-a', { call: async () => new Promise((resolve) => { complete = resolve }) })
  const a = space.conversation('a')
  const pending = a.history()
  a.dispose()
  complete({ hubId: 'fixture', context: { spaceId: 'space-a', conversationId: 'a' }, entries: [] })
  await assert.rejects(pending, /stale_conversation_context/)
  const b = space.conversation('b')
  const other = b.history()
  complete({ hubId: 'fixture', context: { spaceId: 'space-a', conversationId: 'a' }, entries: [] })
  await assert.rejects(other, /cross_conversation_response/)
  space.dispose()
  await assert.rejects(b.history(), /stale_space_context/)
})
