import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import { MIRROR_SCHEMA } from './fork-mirror-cache.ts'
import { DEFAULT_CONFIG, validBotAvatar, avatarDoBot } from './fork-avatar-config.ts'
import { memoryUndoable, normalizeBot, type Bot, type Computer, type Entry } from './models.ts'
import type { HostTransport } from '../renderer/client/host-client.ts'
import type { BotSync } from '../renderer/store/bot-sync.ts'

// Executa mirror, sync e parseEvent reais, resolvendo os aliases como o app, sem iniciar Electron.
const bundle = buildSync({
  entryPoints: [new URL('../renderer/store/bot-sync.ts', import.meta.url).pathname],
  bundle: true, platform: 'node', format: 'cjs', write: false, external: ['react'],
}).outputFiles[0]!.text
const require = createRequire(import.meta.url)
const cacheKey = 'codync-mirror-["local","avatar-test","hub-test",null]'
const computer = { id: 'avatar-test', name: 'Hub' } as Computer
const donna = { shape: 'oval', color: '#9F1239', eye: 'cat', badge: 'sparkle', badgeStyle: 'light', head: 'none', face: 'none', ears: 'hoops', chin: 'none', neck: 'necklace', cheeks: 'none' } as const
// Mesma fixture é conferida pelo Hub real em fork_avatars/mexeriquinhas_tests.rs.
import mexeriquinhaWire from './fork-mexeriquinha-wire-fixture.json' with { type: 'json' }
const hubBots = [
  ...mexeriquinhaWire.map((raw) => normalizeBot({ ...raw, rev: 688 } as unknown as Bot)),
  normalizeBot({ id: 'donna', name: 'Donna', rev: 686, browserPreference: 'computer', computer: false, avatarConfig: donna, workspaceIssue: 'Revise a pasta. Escolha outra pasta nas configurações do bot.' }),
  normalizeBot({ id: 'joio', name: 'Joio', rev: 687, avatarConfig: { ...DEFAULT_CONFIG, shape: 'gota', color: '#FFB020' } }),
  // Diferente do preset por nome: prova que chega a escolha persistida no Hub.
  normalizeBot({ id: 'research', name: 'Research', rev: 689, avatarConfig: { ...DEFAULT_CONFIG, shape: 'ciclope', color: '#38BDF8', badge: 'search' } }),
]

function harness(rawCache?: string, entries: Entry[] = []) {
  const storage = new Map(rawCache ? [[cacheKey, rawCache]] : [])
  storage.set('codync-mirror-hub-["local","avatar-test"]', 'hub-test')
  const localStorage = {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => { storage.set(key, value) },
    removeItem: (key: string) => { storage.delete(key) },
  }
  const module = { exports: {} }
  runInNewContext(bundle, { module, exports: module.exports, require, localStorage, URLSearchParams, queueMicrotask, setTimeout, clearTimeout })
  const Sync = (module.exports as { BotSync: typeof BotSync }).BotSync
  const since: number[] = []
  let connected!: () => void
  const streaming = new Promise<void>((resolve) => { connected = resolve })
  const transport: HostTransport = {
    async call(method) {
      assert.equal(method, 'hello')
      return { hostId: 'hub-test', version: '2.20.3', minApp: '2.20.1', name: 'Hub', avatarEditing: true, avatarConfigVersion: 2 }
    },
    stream(path, onData) {
      const rev = Number(new URL(path, 'http://hub.test').searchParams.get('since'))
      since.push(rev)
      onData(JSON.stringify({ type: 'hello', hostId: 'hub-test', rev: 689 }))
      // Mesmo filtro de Hub::bots_json: um bot só é reenviado se bot.rev > since.
      for (const bot of hubBots) if (bot.rev > rev) onData(JSON.stringify({ type: 'bot', bot }))
      for (const entry of entries) if (entry.rev > rev) onData(JSON.stringify({ type: 'entry', rev: entry.rev, entry }))
      connected()
      return () => {}
    },
  }
  const sync = new Sync(computer, false, () => transport, '2.20.3')
  return { sync, since, streaming, storage }
}

function appearances(bots: Map<string, Bot>) {
  return hubBots.map(({ id }) => {
    const bot = bots.get(id)!
    return JSON.parse(JSON.stringify(avatarDoBot(bot)))
  })
}

test('cache anterior sem avatarConfig e com rev 689 ressincroniza mesmo com versões 2.20.3 iguais', { timeout: 1000 }, async (t) => {
  // normalizeBot de d371328c selecionava campos conhecidos e descartava avatarConfig.
  const oldBots = hubBots.map(({ avatarConfig: _, ...bot }) => bot)
  const oldCache = JSON.stringify({ stamp: '2.20.3/2.20.3', hostId: 'hub-test', rev: 689, bots: oldBots, entries: [] })
  const { sync, since, streaming } = harness(oldCache)
  t.after(() => sync.retire())
  sync.start()
  await streaming
  assert.deepEqual(appearances(sync.bots), hubBots.map((bot) => avatarDoBot(bot)), `catch-up solicitado: since=${since}`)
  assert.deepEqual(since, [0])
})

test('evento e cache atual preservam aparência, preferência e aviso de rotina sem ressincronizar', { timeout: 1000 }, async (t) => {
  // Payload bruto emitido pelo Hub: o mirror conserva campos de aviso que não interpreta.
  const data = { text: 'Rotina de teste criada.', routineId: 'fixture', routineCreation: {
    version: 1, spaceId: 'pessoal', botId: 'donna', routineId: 'fixture', name: 'Teste', botName: 'Donna', createdAt: 1, decision: 'pending',
  } }
  const entry: Entry = {
    id: 'routine-created:fixture', botId: 'donna', spaceId: 'pessoal', conversationId: 'donna', sessionId: 'donna',
    seq: 1, rev: 690, kind: 'notice', turn: 0, createdAt: 1, updatedAt: 1, data,
  }
  const first = harness(undefined, [entry])
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  assert.deepEqual(first.since, [0])
  assert.deepEqual(appearances(first.sync.bots), hubBots.map((bot) => avatarDoBot(bot)))
  assert.equal(first.sync.bots.get('donna')?.browserPreference, 'computer')
  assert.equal(first.sync.bots.get('donna')?.computer, false)
  assert.deepEqual(JSON.parse(JSON.stringify(first.sync.entries.get('donna')?.[0])), entry)
  for (const bot of first.sync.bots.values()) assert.equal(validBotAvatar(avatarDoBot(bot)), true)
  first.sync.saveCache()

  const second = harness(first.storage.get(cacheKey))
  t.after(() => second.sync.retire())
  assert.deepEqual(appearances(second.sync.bots), hubBots.map((bot) => avatarDoBot(bot)))
  assert.equal(second.sync.bots.get('donna')?.browserPreference, 'computer')
  assert.equal(second.sync.bots.get('donna')?.computer, false)
  assert.deepEqual(JSON.parse(JSON.stringify(second.sync.entries.get('donna')?.[0])), entry)
  second.sync.start()
  await second.streaming
  assert.deepEqual(second.since, [690])
  assert.deepEqual(appearances(second.sync.bots), hubBots.map((bot) => avatarDoBot(bot)))
})

test('cache v1 sem workspaceIssue recebe o aviso do Hub mesmo sem mudar a versão ou a revisão do bot', { timeout: 1000 }, async (t) => {
  const oldBots = hubBots.map(({ workspaceIssue: _, ...bot }) => bot)
  const oldCache = JSON.stringify({ stamp: 'donnabot-mirror-v1/2.20.3/2.20.3', hostId: 'hub-test', rev: 689, bots: oldBots, entries: [] })
  const first = harness(oldCache)
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  assert.deepEqual(first.since, [0])
  assert.match(first.sync.bots.get('donna')?.workspaceIssue ?? '', /^Revise a pasta\./)
  first.sync.saveCache()
  const second = harness(first.storage.get(cacheKey))
  t.after(() => second.sync.retire())
  assert.equal(second.sync.bots.get('donna')?.workspaceIssue, hubBots.find((bot) => bot.id === 'donna')?.workspaceIssue)
})


test('schema 13 preserva a referência pública de #202 por evento, cache e reabertura sem refazer catch-up', { timeout: 1000 }, async (t) => {
  const entry: Entry = {
    id: 'recibo', botId: 'donna', spaceId: 'pessoal', conversationId: 'donna', sessionId: 'donna',
    seq: 1, rev: 690, kind: 'notice', turn: 1, createdAt: 1, updatedAt: 1,
    data: { text: 'Conferir resultado', conectorOperation: { tentativaId: 'tentativa', cardId: 'cartao', state: 'incerta', decisionProtocol: 1 } },
  }
  const first = harness(undefined, [entry])
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  const projected = (sync: BotSync) => JSON.parse(JSON.stringify(sync.entries.get('donna')?.[0]))
  assert.deepEqual(projected(first.sync), entry, 'parseEvent e sync conservam a entrada pública, sem selecionar seus campos')
  first.sync.saveCache()
  const raw = first.storage.get(cacheKey)!
  assert.equal(JSON.parse(raw).stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/2.20.3/2.20.3`)
  const second = harness(raw, [entry])
  t.after(() => second.sync.retire())
  assert.deepEqual(projected(second.sync), entry, 'loadCache conserva sessão e referência sem inventar autoridade')
  second.sync.start()
  await second.streaming
  assert.deepEqual(second.since, [690], 'schema atual não provoca catch-up desnecessário')
  assert.deepEqual(projected(second.sync), entry)
})

test('schema 26 refaz catch-up do cache 25 (app só com o rascunho WhatsApp) sem a natureza confirmed', { timeout: 1000 }, async (t) => {
  const entry: Entry = {
    id: 'memoria', botId: 'donna', spaceId: 'pessoal', conversationId: 'donna', sessionId: 'donna',
    seq: 1, rev: 700, kind: 'notice', turn: 1, createdAt: 1, updatedAt: 1,
    data: { memory: { action: 'reclassify', status: 'done', nature: 'confirmed', text: 'Prefere café sem açúcar' } },
  }
  const oldCache = JSON.stringify({ stamp: 'donnabot-mirror-v25/2.20.3/2.20.3', hostId: 'hub-test', rev: 700, bots: hubBots, entries: [] })
  const first = harness(oldCache, [entry])
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  assert.deepEqual(first.since, [0], 'cache 25 não conserva o rev que ocultaria a natureza nova')
  first.sync.saveCache()
  assert.equal(JSON.parse(first.storage.get(cacheKey)!).stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/2.20.3/2.20.3`)
})

test('schema 25+ conserva rascunho não decisível e refaz catch-up do cache 24 sem o campo', { timeout: 1000 }, async (t) => {
  const entry: Entry = {
    id: 'rascunho', botId: 'donna', spaceId: 'pessoal', conversationId: 'donna', sessionId: 'donna',
    seq: 1, rev: 690, kind: 'notice', turn: 1, createdAt: 1, updatedAt: 1,
    data: { conectorDraft: { draftId: 'rascunho', revision: 1, deadline: 90_000, state: 'draft' } },
  }
  const oldCache = JSON.stringify({ stamp: 'donnabot-mirror-v24/2.20.3/2.20.3', hostId: 'hub-test', rev: 690, bots: hubBots, entries: [] })
  const first = harness(oldCache, [entry])
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  assert.deepEqual(first.since, [0], 'cache 24 não conserva o rev que ocultaria o novo campo')
  first.sync.saveCache()
  const raw = first.storage.get(cacheKey)!
  assert.equal(JSON.parse(raw).stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/2.20.3/2.20.3`)
  const second = harness(raw, [entry])
  t.after(() => second.sync.retire())
  assert.deepEqual(JSON.parse(JSON.stringify(second.sync.entries.get('donna')?.[0])), entry)
  second.sync.start()
  await second.streaming
  assert.deepEqual(second.since, [690], 'cache atual conserva somente a projeção pública')
})


test('main12 em schema13 conserva procedência por versão e disposição pelo evento e cache real', { timeout: 1000 }, async (t) => {
  const memories: NonNullable<Entry['data']['memory']>[] = [
    {
      action: 'revise', status: 'done', rev: 21, jobId: 'job-concluido',
      text: 'Texto anterior', at: 100, nature: 'conflict', natureBot: 'donna', conflictText: 'Alternativa inferida',
      origin: { schema: 1, kind: 'external', external: true, sources: [{ messageId: 'humana', kind: 'external', detail: 'site' }] },
      previous: 'Texto anterior', previousAt: 90, previousRev: 20, previousNature: 'said', previousNatureBot: null,
      previousOrigin: { schema: 1, kind: 'user', external: false, sources: [{ messageId: 'anterior', kind: 'user' }] },
      messageIds: ['humana'], responseIds: ['resposta'],
    },
    { action: 'remember', status: 'waiting', jobId: 'job-adiado', availability: 'transient', messageIds: ['outra'], responseIds: [] },
    { action: 'remember', status: 'ineligible', jobId: 'job-falhou', availability: 'ineligible', reason: 'ineligible' },
    { action: 'remember', status: 'undone', rev: 22, jobId: 'job-desfeito', nature: 'unknown', origin: null },
  ]
  const entries: Entry[] = memories.map((memory, i) => ({
    id: `memoria-${i}`, botId: 'donna', spaceId: 'pessoal', conversationId: 'donna', sessionId: 'sessao-memoria',
    seq: i + 1, rev: 690 + i, kind: 'notice', turn: 1, createdAt: 1, updatedAt: 1, data: { memory },
  }))
  const first = harness(undefined, entries)
  t.after(() => first.sync.retire())
  first.sync.start()
  await first.streaming
  const projected = (sync: BotSync) => JSON.parse(JSON.stringify(sync.entries.get('donna')))
  assert.deepEqual(projected(first.sync), entries, 'parseEvent e mirror preservam cada versão e sua origem independente')
  first.sync.saveCache()
  const raw = first.storage.get(cacheKey)!
  assert.equal(JSON.parse(raw).stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/2.20.3/2.20.3`)
  const second = harness(raw, entries)
  t.after(() => second.sync.retire())
  assert.deepEqual(projected(second.sync), entries, 'reabertura não mistura origem atual/anterior nem reativa disposição')
  assert.deepEqual(Array.from(second.sync.entries.get('donna')!, (e) => memoryUndoable(e.data.memory)), [true, false, false, false])
  second.sync.start()
  await second.streaming
  assert.deepEqual(second.since, [693])
  assert.deepEqual(projected(second.sync), entries)
})


test('schema atual refaz catch-up de cache27 que descartava configurações v2', {timeout:1000}, async (t) => {
  const oldCache=JSON.stringify({stamp:'donnabot-mirror-v27/2.20.3/2.20.3',hostId:'hub-test',rev:689,bots:[],entries:[]})
  const {sync,since,streaming,storage}=harness(oldCache)
  t.after(()=>sync.retire())
  sync.start(); await streaming
  assert.deepEqual(since,[0])
  for (const bot of mexeriquinhaWire) assert.deepEqual(JSON.parse(JSON.stringify(sync.bots.get(bot.id)?.mexeriquinha)),bot.mexeriquinha)
  sync.saveCache()
  const reopened=harness(storage.get(cacheKey))
  t.after(()=>reopened.sync.retire())
  for (const bot of mexeriquinhaWire) assert.deepEqual(JSON.parse(JSON.stringify(reopened.sync.bots.get(bot.id)?.mexeriquinha)),bot.mexeriquinha)
})
