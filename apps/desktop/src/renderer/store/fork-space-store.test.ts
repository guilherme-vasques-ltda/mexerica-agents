// E2, Passo 1: loja, cliente e fake-host reais; o transporte só encaminha RPC e query SSE.
import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { setImmediate as tick } from 'node:timers/promises'
import { webcrypto } from 'node:crypto'
import { buildSync } from 'esbuild'
import type { Computer } from '../../shared/models'
import type { HostTransport } from '../client/host-client'

type Bundle = { SpaceBotCreation: typeof import('./fork-space-bot-creation').SpaceBotCreation; SpaceMirrors: typeof import('./fork-space-mirrors').SpaceMirrors; refKey: typeof import('./app-model').refKey; parseRef: typeof import('./app-model').parseRef; BotStore: typeof import('./bot-store').BotStore; FakeHost: typeof import('../demo/fake-host').FakeHost; HostClient: typeof import('../client/host-client').HostClient }
const renderer = new URL('../', import.meta.url).pathname
const code = buildSync({
  stdin: { contents: `export { SpaceBotCreation } from './store/fork-space-bot-creation'; export { SpaceMirrors } from './store/fork-space-mirrors'; export { refKey, parseRef } from './store/app-model'; export { BotStore } from './store/bot-store'; export { HostClient } from './client/host-client'; export { FakeHost } from './demo/fake-host'`, resolveDir: renderer, loader: 'ts' },
  tsconfig: new URL('../../../tsconfig.json', import.meta.url).pathname, bundle: true, platform: 'node', format: 'cjs', write: false,
  loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
}).outputFiles[0].text
const settle = async () => { for (let i = 0; i < 16; i++) await tick() }
function fixture() {
  const timers = new Map<number, { fn: () => void; ms: number }>()
  let timerId = 0
  const clock = (fn: () => void, ms = 0) => { timers.set(++timerId, { fn, ms }); return timerId }
  const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
  const module = { exports: {} }
  const storage = new Map<string, string>()
  const location = { search: '?spaces=e1' }
  runInNewContext(code, {
    module, queueMicrotask, URL, URLSearchParams, Date, crypto: webcrypto, Uint8Array, btoa, atob, structuredClone,
    setTimeout: clock, clearTimeout: (id: number) => timers.delete(id), setInterval: clock, clearInterval: (id: number) => timers.delete(id),
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v), removeItem: (k: string) => storage.delete(k) }, location,
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, location, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }) },
  })
  const { BotStore, FakeHost, HostClient, SpaceMirrors, SpaceBotCreation, refKey, parseRef } = module.exports as Bundle
  const host = new FakeHost('2.20.3')
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const streams: string[] = []
  let muteBots = false
  const transport: HostTransport = {
    async call(method, raw) { const body = (raw ?? {}) as Record<string, unknown>; calls.push({ method, body }); return host.call(method, body) },
    stream(path, onData) { streams.push(path); const query = new URL(path, 'http://fake.invalid').searchParams; return host.subscribe(Number(query.get('since')), line => { if (!muteBots || JSON.parse(line).type !== 'bot') onData(line) }, query.get('spaceId') ?? undefined) },
  }
  const stores: InstanceType<Bundle['BotStore']>[] = []
  const open = (spaceId?: string, computerId = 'demo-mac') => {
    const store = new BotStore({ id: computerId, name: 'Fixture', signKey: '', urls: [] } as Computer, true, () => transport, '2.20.3', 'e2-passo1', spaceId)
    stores.push(store); store.start(); return store
  }
  const flush = async (ms = 0) => {
    await settle()
    const pending = [...timers].filter(([, t]) => t.ms <= ms)
    for (const [id, task] of pending) { if (timers.delete(id)) task.fn() }
    await settle()
  }
  return { host, open, calls, streams, storage, SpaceMirrors, SpaceBotCreation, muteBots: (value: boolean) => { muteBots = value }, refKey, parseRef, client: (space: string) => new HostClient(transport, space), flush, close() { for (const s of stores) s.retire(); timers.clear() } }
}

const PERSONAL = '00000000-0000-4000-8000-000000000001'

test('E2 Passo 1: duas lojas reais recebem apenas roster e eventos do próprio Space', async () => {
  const f = fixture()
  try {
    const personal = f.open(PERSONAL)
    const space = f.open('space-estudio')
    await f.flush()
    assert.equal(personal.connection.kind, 'online')
    assert.equal(space.connection.kind, 'online')
    assert.ok(personal.roster.some(b => b.id === 'sprint'))
    assert.ok(personal.roster.every(b => !b.spaceId || b.spaceId === PERSONAL))
    assert.equal(space.roster.length, 1)
    assert.equal(space.roster[0]!.spaceId, 'space-estudio')
    assert.equal(space.bots.has('sprint'), false)
    assert.equal(space.hello?.spaceId, 'space-estudio')
    assert.equal(f.streams.length, 2)
    const bot = space.roster[0]!
    f.host.fixtureBot(bot.id, { unread: 4 })
    await f.flush()
    assert.equal(space.bots.get(bot.id)?.unread, 4)
    assert.equal(personal.bots.has(bot.id), false)
    assert.ok(f.streams.every(path => new URL(path, 'http://fake.invalid').searchParams.has('spaceId')))
  } finally { f.close() }
})

test('E2 Passo 1: envio normal com anexo em chunks chega e pode ser relido só no bot/Space de origem', async () => {
  const f = fixture()
  try {
    const personal = f.open(PERSONAL)
    const space = f.open('space-estudio')
    await f.flush()
    const bot = space.roster[0]!
    const data = Uint8Array.from({ length: 400_000 }, (_, i) => i % 251)
    space.send('Arquivo do Estúdio', bot.id, { nonce: 'passo1-envio', files: [{ id: 'arquivo-local', name: 'fixture.bin', data }] })
    await f.flush()
    const sent = space.allEntries(bot.id).find(e => e.data.clientNonce === 'passo1-envio')
    assert.equal(sent?.data.status, 'sent')
    assert.equal(sent?.id.startsWith('local-'), false)
    assert.equal(sent?.data.attachments?.length, 1)
    const file = sent!.data.attachments![0]!
    assert.equal(file.name, 'fixture.bin')
    assert.equal(file.size, data.length)
    const received = await f.client('space-estudio').readUpload(bot.id, file.id)
    assert.deepEqual(Array.from(received), Array.from(data))
    assert.equal(personal.allEntries(bot.id).length, 0)
    const uploads = f.calls.filter(c => c.method === 'upload')
    assert.equal(uploads.length, 2)
    assert.equal(f.calls.filter(c => c.method === 'send').length, 1)
    assert.ok(uploads.every(c => c.body.spaceId === 'space-estudio' && c.body.botId === bot.id))
    await assert.rejects(f.client(PERSONAL).readUpload(bot.id, file.id), (e: unknown) => (e as { message?: string }).message === 'cross_space_reference')
    await assert.rejects(f.client(PERSONAL).readUpload('sprint', file.id), (e: unknown) => (e as Error).message === 'unknown_upload')
    // Replay do mesmo nonce não duplica a mensagem nem o anexo.
    await f.client('space-estudio').send(bot.id, 'Arquivo do Estúdio', 'passo1-envio', [file.id])
    assert.equal(space.allEntries(bot.id).filter(e => e.data.clientNonce === 'passo1-envio').length, 1)
  } finally { f.close() }
})

test('E2 Passo 1: resposta ao cartão usa a loja real e preserva cartões de outros Spaces', async () => {
  const f = fixture()
  try {
    const personal = f.open(PERSONAL)
    const space = f.open('space-estudio')
    await f.flush()
    const bot = space.roster[0]!
    const personalCard = f.host.fixtureApproval('sprint')
    const card = f.host.fixtureApproval(bot.id)
    await f.flush()
    assert.equal(personal.allEntries(bot.id).length, 0)
    assert.equal(space.allEntries('sprint').length, 0)
    await assert.rejects(f.client(PERSONAL).call('respondPermission', { entryId: card.id, optionId: 'allow-once' }), (e: unknown) => (e as { message?: string }).message === 'cross_space_reference')
    const entryRev = card.rev
    space.respond(space.allEntries(bot.id).find(e => e.id === card.id)!, 'allow-once')
    await f.flush()
    const answer = space.allEntries(bot.id).find(e => e.id === card.id)!
    assert.equal(answer.data.status, 'answered')
    assert.equal(answer.data.selected, 'allow-once')
    assert.equal(personal.allEntries('sprint').find(e => e.id === personalCard.id)?.data.status, 'pending')
    const call = f.calls.findLast(c => c.method === 'respondPermission')!
    assert.equal(call.body.spaceId, 'space-estudio')
    assert.equal(call.body.botId, bot.id)
    assert.equal(call.body.entryRev, entryRev)
  } finally { f.close() }
})

test('E2 Passo 1: referências cruzadas são recusadas antes de gerar efeitos no fake-host', async () => {
  const f = fixture()
  try {
    const space = f.open('space-estudio')
    await f.flush()
    const before = space.allEntries('sprint').length
    await assert.rejects(f.client('space-estudio').send('sprint', 'Nunca no Pessoal', 'cruzado', null), (e: unknown) => (e as { message?: string }).message === 'cross_space_reference')
    await assert.rejects(f.client('space-inexistente').hello(), (e: unknown) => (e as { message?: string }).message === 'space_not_found')
    await assert.rejects(f.client('space-estudio').call('hello', { spaceId: PERSONAL }), /cross_space_reference/)
    assert.equal(space.allEntries('sprint').length, before)
    // História sem dono conhecido não pode reaparecer no Pessoal após uma exclusão.
    const seeded = await f.client('space-estudio').call<{ bot: { id: string } }>('createBot', {
      hubId: f.host.hostId, name: 'Auxiliar fictício', backend: 'codex',
      forkEditor: { engineAccountId: `ac_${'d'.repeat(64)}` },
    })
    f.host.fixtureApproval(seeded.bot.id)
    await f.client('space-estudio').call('deleteBot', { botId: seeded.bot.id })
    await f.flush()
    assert.equal(space.bots.has(seeded.bot.id), false)
    const personal = f.open(PERSONAL)
    await f.flush()
    assert.equal(personal.allEntries(seeded.bot.id).length, 0)
  } finally { f.close() }
})


test('E2: quatro Spaces mantêm quatro lojas/streams, rascunhos e anexos; seleção volta ao contexto certo', async () => {
  const f = fixture()
  try {
    const root = f.open()
    const window = { stores: new Map([[root.computer.id, root]]), selection: null as import('./app-model').BotRef | null, select(ref: import('./app-model').BotRef | null) { this.selection = ref } }
    const manager = new f.SpaceMirrors(window, () => {})
    manager.attach(root)
    await f.flush()
    assert.equal(manager.all.length, 4)
    assert.equal(f.streams.length, 4)
    const a = manager.store(root.computer.id, 'space-estudio')!
    const b = manager.store(root.computer.id, 'space-jardim')!
    const aid = a.principal.id!, bid = b.principal.id!
    assert.ok(aid && bid)
    window.selection = { computerId: root.computer.id, botId: 'sprint' }
    manager.activate(root.computer.id, a.spaceId)
    window.selection = { computerId: root.computer.id, spaceId: a.spaceId, botId: aid }
    a.composerDrafts.set(aid, 'Rascunho A')
    const bytes = new Uint8Array([1, 2, 3])
    a.composerDrafts.setFiles(aid, [{ id: 'a', name: 'a.txt', data: bytes }])
    manager.activate(root.computer.id, b.spaceId)
    b.composerDrafts.set(bid, 'Rascunho B')
    assert.equal(b.composerDrafts.get(aid), '')
    assert.equal(b.composerDrafts.files(aid).length, 0)
    window.selection = { computerId: root.computer.id, spaceId: b.spaceId, botId: bid }
    manager.activate(root.computer.id, a.spaceId)
    assert.equal(window.selection?.botId, aid)
    assert.equal(a.composerDrafts.get(aid), 'Rascunho A')
    assert.deepEqual([...a.composerDrafts.files(aid)[0]!.data], [1, 2, 3])
    manager.activate(root.computer.id)
    assert.equal(window.selection?.botId, 'sprint')
    f.host.fixtureBot(aid, { unread: 7 })
    await f.flush()
    assert.equal(a.bots.get(aid)?.unread, 7)
    assert.equal(root.bots.has(aid), false)
    a.saveCache(); b.saveCache(); root.saveCache()
    const keys = [...f.storage.keys()].filter(k => k.startsWith('codync-mirror-['))
    assert.equal(keys.length, 3)
    assert.equal(new Set(keys).size, 3)
    assert.ok(keys.every(k => k.includes(root.hello!.hostId)))
    manager.detach(root.computer.id)
  } finally { f.close() }
})

test('E2: abrir bot externo preserva Space na referência e infere apenas bot do mesmo computador', async () => {
  const f = fixture()
  try {
    const root = f.open()
    const window = { stores: new Map([[root.computer.id, root]]), selection: null, select() {} }
    const manager = new f.SpaceMirrors(window, () => {})
    manager.attach(root); await f.flush()
    const a = manager.store(root.computer.id, 'space-estudio')!
    const ref = manager.infer({ computerId: root.computer.id, botId: a.principal.id! })
    assert.equal(ref.spaceId, 'space-estudio')
    const parsed = f.parseRef(f.refKey(ref))!
    assert.equal(parsed.spaceId, ref.spaceId)
    assert.equal(parsed.hubId, root.hello!.hostId)
    assert.equal(manager.resolve({ ...parsed, hubId: 'outro-hub' }), null)
    assert.equal(parsed.botId, ref.botId)
    assert.equal(manager.infer({ computerId: 'outro-hub', botId: ref.botId }).spaceId, undefined)
    manager.activate(parsed.computerId, parsed.spaceId)
    assert.equal(manager.current(root.computer.id), a)
    a.connection = { kind: 'offline', message: 'fixture' }
    assert.equal(manager.current(root.computer.id), a)
    manager.detach(root.computer.id)
  } finally { f.close() }
})

test('E2: cache antigo é descartado sem apagar rascunho; reinício restaura cache apenas do Hub/Space confirmado', async () => {
  const f = fixture()
  try {
    const a = f.open('space-estudio'); await f.flush()
    const id = a.principal.id!
    a.composerDrafts.set(id, 'Texto preservado')
    a.saveCache()
    const key = [...f.storage.keys()].find(k => k.startsWith('codync-mirror-['))!
    const cache = JSON.parse(f.storage.get(key)!)
    const b = f.open('space-estudio')
    assert.ok(b.bots.has(id))
    assert.equal(b.composerDrafts.get(id), 'Texto preservado')
    cache.stamp = 'schema:35'
    f.storage.set(key, JSON.stringify(cache))
    const c = f.open('space-estudio')
    assert.equal(c.bots.size, 0)
    assert.equal(c.composerDrafts.get(id), 'Texto preservado')
  } finally { f.close() }
})

test('E2: criar canal usa o Space da loja e os membros do próprio Space', async () => {
  const f = fixture()
  try {
    const a = f.open('space-estudio'); await f.flush()
    await a.createGroup('geral', '', [a.principal.id!])
    await f.flush()
    const create = f.calls.find(c => c.method === 'createBot')!
    assert.equal(create.body.spaceId, 'space-estudio')
    assert.equal(create.body.hubId, a.hello!.hostId)
    assert.ok(a.roster.some(b => b.kind === 'group' && b.name === 'geral'))
    await assert.rejects(a.createGroup('cruzado', '', ['sprint']), (e: unknown) => (e as { message: string }).message === 'cross_space_member')
  } finally { f.close() }
})


test('E2: A→B→A não leva resposta tardia de A ao B; geração antiga não repõe história', async () => {
  const f = fixture()
  try {
    const a = f.open('space-estudio'), b = f.open('space-jardim')
    await f.flush()
    const aid = a.principal.id!, bid = b.principal.id!
    let release: (() => void) | undefined
    const original = f.host.call.bind(f.host)
    f.host.call = async (method, body) => {
      const result = await original(method, body)
      if (method === 'history') await new Promise<void>(resolve => { release = resolve })
      return result
    }
    const pending = a.loadOlder(aid)
    await f.flush()
    assert.ok(release)
    // B continua independente; A reabre uma geração nova antes da página antiga voltar.
    b.composerDrafts.set(bid, 'B permanece')
    a.restartStream(); await f.flush()
    release!(); await pending
    assert.equal(a.historyComplete.has(aid), false)
    assert.equal(b.allEntries(aid).length, 0)
    assert.equal(b.composerDrafts.get(bid), 'B permanece')
  } finally { f.close() }
})

test('E2: menu apagado não inicia leituras de Times/sessões/conectores das lojas nativas', async () => {
  const f = fixture()
  try {
    f.open('space-estudio'); f.open('space-jardim'); await f.flush(500)
    const denied = f.calls.filter(c => ['donnaTimes', 'donnaSessions', 'donnaWhatsapp', 'botGroupsEdit'].includes(c.method))
    assert.equal(denied.length, 0)
  } finally { f.close() }
})

test('E2: Hub antigo mantém destino explícito indisponível em vez de escolher Pessoal', async () => {
  const f = fixture()
  try {
    const root = f.open()
    const window = { stores: new Map([[root.computer.id, root]]), selection: null, select() {} }
    const manager = new f.SpaceMirrors(window, () => {})
    manager.attach(root); await f.flush()
    manager.activate(root.computer.id, 'space-estudio')
    const original = f.host.call.bind(f.host)
    f.host.call = async (method, body) => {
      const result = await original(method, body)
      return method === 'hello' ? { ...(result as object), spacesContract: undefined } : result
    }
    root.restartStream(); await f.flush()
    assert.equal(manager.active?.spaceId, 'space-estudio')
    assert.equal(manager.current(root.computer.id), null)
    manager.detach(root.computer.id)
  } finally { f.close() }
})


test('E2: outra identidade de Hub não reaproveita loja nem referência externa antiga', async () => {
  const f = fixture()
  try {
    const root = f.open()
    const window = { stores: new Map([[root.computer.id, root]]), selection: null, select() {} }
    const manager = new f.SpaceMirrors(window, () => {})
    manager.attach(root); await f.flush()
    const old = manager.store(root.computer.id, 'space-estudio')!
    const ref = manager.infer({ computerId: root.computer.id, botId: old.principal.id! })
    manager.activate(root.computer.id, 'space-estudio')
    root.composerDrafts.set('sprint', 'Rascunho do Hub A')
    root.saveCache()
    const cacheA = [...f.storage.keys()].find(k => k.startsWith('codync-mirror-[') && k.includes('demo-host') && k.endsWith('null]'))!
    f.host.hostId = 'hub-substituto'
    root.restartStream(); await f.flush()
    assert.equal(manager.resolve(ref), null)
    assert.equal(manager.current(root.computer.id), null)
    assert.equal(manager.active?.hostId, 'demo-host')
    const next = manager.store(root.computer.id, 'space-estudio')!
    assert.notEqual(next, old)
    assert.equal(next.hello?.hostId, 'hub-substituto')
    assert.equal(JSON.parse(f.storage.get(cacheA)!).hostId, 'demo-host')
    assert.equal(root.composerDrafts.get('sprint'), '')
    root.composerDrafts.set('sprint', 'Rascunho do Hub B')
    f.host.hostId = 'demo-host'
    root.restartStream(); await f.flush()
    assert.equal(root.composerDrafts.get('sprint'), 'Rascunho do Hub A')
    manager.detach(root.computer.id)
  } finally { f.close() }
})


test('E2: recibo de criação abre o novo bot sem esperar SSE; outro Hub/Space não recebe o recibo', async () => {
  const f = fixture()
  try {
    const root = f.open(), space = f.open('space-estudio'), other = f.open('space-jardim')
    await f.flush()
    f.muteBots(true)
    const creation = new f.SpaceBotCreation(root, 'space-estudio', (bot, hubId) => {
      assert.equal(space.bots.has(bot.id), false, 'stream de bot ainda não entregou')
      assert.equal(space.receiveCreatedBot(bot, 'outro-hub'), false)
      assert.equal(other.receiveCreatedBot(bot, hubId), false)
      assert.equal(space.receiveCreatedBot(bot, hubId), true)
    })
    const bot = await creation.create({ spaceId: 'space-estudio', name: 'Novo', kind: 'group', members: [space.principal.id!] })
    assert.equal(space.bots.get(bot.id)?.name, 'Novo')
    assert.equal(root.bots.has(bot.id), false)
    assert.equal(other.bots.has(bot.id), false)
    assert.equal(await creation.create({ spaceId: 'space-estudio' }), bot, 'recibo conhecido não cria de novo')
  } finally { f.close() }
})

test('E2: deleteBot recusado com resposta em andamento mantém o bot e avisa o motivo', async () => {
  const f = fixture()
  try {
    const space = f.open('space-estudio')
    await f.flush()
    const created = await f.client('space-estudio').call<{ bot: { id: string } }>('createBot', {
      hubId: f.host.hostId, name: 'Auxiliar ocupado', backend: 'codex',
    })
    await f.flush()
    const bot = space.bots.get(created.bot.id)!
    assert.ok(bot, 'o bot criado aparece na loja')
    const original = f.host.call.bind(f.host)
    f.host.call = async (method, body) => {
      if (method === 'deleteBot') throw new Error('open_chat_busy')
      return original(method, body)
    }
    space.composerDrafts.edit(bot.id, 'rascunho a manter', 'typed')
    await space.delete(bot)
    await f.flush()
    assert.equal(space.bots.get(bot.id)?.id, bot.id, 'o bot continua na lista')
    assert.equal(space.composerDrafts.get(bot.id), 'rascunho a manter', 'o rascunho só sai quando o Hub confirma')
    assert.equal(space.lastError, 'Stop the reply before deleting the bot.')
    // Erro sem o código conhecido mantém o comportamento de sempre (o Error vem de outro realm).
    f.host.call = async (method, body) => {
      if (method === 'deleteBot') throw new Error('engine gone')
      return original(method, body)
    }
    await space.delete(bot)
    await f.flush()
    assert.ok(space.lastError?.includes('engine gone'))
  } finally { f.close() }
})
