import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { TowerEvent, TowerLink, TowerSource } from './torre-session.ts'

// Carrega SaudeHubCard e DoctorSession reais sem DOM real nem APIs do Electron.
const bundle = await build({
  stdin: {
    contents: `export { SaudeHubCard, DoctorSession } from './views/fork/torre/SaudeHubCard';
      export { HostClient, parseEvent } from './client/host-client';
      export { idiomaPref } from './lib/fork-i18n/estado';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: fileURLToPath(new URL('../../../', import.meta.url)),
    loader: 'ts',
  },
  bundle: true,
  platform: 'node',
  format: 'cjs',
  write: false,
  jsx: 'automatic',
  alias: { '@shared': fileURLToPath(new URL('../../../../shared', import.meta.url)) },
  plugins: [{
    name: 'marcadores',
    setup(b) {
      b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icon', namespace: 'marker' }))
      b.onLoad({ filter: /.*/, namespace: 'marker' }, () => ({
        contents: 'export function Icon() { return null }',
        loader: 'js',
      }))
    },
  }],
  loader: { '.css': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})

const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const module = { exports: {} as Record<string, any> }
runInNewContext(bundle.outputFiles[0]!.text, {
  module,
  exports: module.exports,
  require: createRequire(import.meta.url),
  window: { addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 },
  matchMedia: media,
  localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
  crypto,
  TextEncoder,
  URLSearchParams,
  console,
  setTimeout,
  clearTimeout,
  queueMicrotask,
})
const m = module.exports
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms))

function renderState(state: unknown, language = 'pt-BR', onReread?: () => void) {
  m.idiomaPref.set(language)
  return m.renderToStaticMarkup(m.createElement(m.SaudeHubCard, { state, onRetry: () => {}, onReread }))
}

test('SaudeHubCard oferece nova leitura só para falha recuperável', () => {
  for (const reason of ['error', 'invalid']) {
    const html = renderState({ kind: 'unavailable', reason }, 'pt-BR', () => {})
    assert.match(html, /Tentar de novo/)
  }
  for (const reason of ['unsupported', 'denied']) {
    const html = renderState({ kind: 'unavailable', reason })
    assert.doesNotMatch(html, /<button/)
  }
})

test('SaudeHubCard não usa API exclusiva do Electron', () => {
  const source = readFileSync(new URL('./SaudeHubCard.tsx', import.meta.url), 'utf8')
    .replace(/\/\/.*$/gm, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
  assert.doesNotMatch(source, /window\.codync|from\s+['"]electron['"]|ipcRenderer/)
})

test('SaudeHubCard distingue hub_offline, unavailable (Hub antigo), waiting_user e error', () => {
  const offlineHtml = renderState({ kind: 'hub_offline', spaceId: 'personal', offlineSince: 1_760_000_000_000 })
  assert.match(offlineHtml, /data-saude-overall="hub_offline"/)
  assert.match(offlineHtml, /Hub offline/)
  assert.doesNotMatch(offlineHtml, /Operacional/)

  const oldHubHtml = renderState({ kind: 'unavailable', reason: 'unsupported' })
  assert.match(oldHubHtml, /data-saude-overall="unavailable"/)
  assert.match(oldHubHtml, /Não informado/)
  assert.doesNotMatch(oldHubHtml, /Operacional/)

  const waitingHtml = renderState({
    kind: 'ready',
    report: {
      version: 1,
      hubId: 'h1',
      spaceId: 'personal',
      collectedAt: 1_760_000_000_000,
      overall: 'waiting_user',
      checks: [
        { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
        { id: 'ai_engine', area: 'ai_engine', status: 'ok', code: 'ai_ok', count: 1, actionKind: 'none' },
        { id: 'approvals', area: 'approvals', status: 'warning', code: 'approvals_waiting', count: 3, actionKind: 'review_waiting' },
      ],
    },
  })
  assert.match(waitingHtml, /data-saude-overall="waiting_user"/)
  assert.match(waitingHtml, /Esperando você/)
  assert.match(waitingHtml, /não está travado/)
  assert.match(waitingHtml, /Esperando sua decisão \(3\)/)

  const aiDownHtml = renderState({
    kind: 'ready',
    report: {
      version: 1,
      hubId: 'h1',
      spaceId: 'personal',
      collectedAt: 1_760_000_000_000,
      overall: 'error',
      checks: [
        { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
        { id: 'ai_engine', area: 'ai_engine', status: 'error', code: 'ai_no_active_account', count: 0, actionKind: 'connect_ai' },
      ],
    },
  })
  assert.match(aiDownHtml, /data-saude-overall="error"/)
  assert.match(aiDownHtml, /IA desconectada/)
})

test('DoctorSession consulta donnaHubDoctor, reconsulta em donnaDoctorChanged e degrada em offline/404', async () => {
  let emit: ((e: TowerEvent) => void) | null = null
  let mode: 'ok' | '404' | 'offline' = 'ok'
  let calls = 0

  class MockSource implements TowerSource {
    hello: { hostId: string; rev: number } | null = { hostId: 'h1', rev: 1 }
    principalEpoch = 1
    private listeners = new Set<() => void>()
    connectedClient: TowerLink | null = {
      call: async (method, body) => {
        assert.equal(method, 'donnaHubDoctor')
        assert.equal(JSON.stringify(body), '{"spaceId":"personal"}')
        calls++
        if (mode === '404') throw Object.assign(new Error('unknown method'), { status: 404 })
        if (mode === 'offline') throw new Error('ECONNREFUSED')
        return {
          version: 1,
          hubId: 'h1',
          spaceId: 'personal',
          collectedAt: 1_760_000_000_000 + calls,
          overall: 'healthy',
          checks: [{ id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' }],
        }
      },
      inSpace: () => ({
        events: (_since, _client, onEvent) => {
          emit = onEvent
          return () => {}
        },
      }),
    }
    subscribe(l: () => void) {
      this.listeners.add(l)
      return () => this.listeners.delete(l)
    }
    notify() {
      for (const l of this.listeners) l()
    }
  }

  const src = new MockSource()
  const session = new m.DoctorSession(() => 999)
  session.open(src, 'personal')
  await tick()
  assert.equal(session.state.kind, 'ready')
  assert.equal(calls, 1)

  // Evento de outro space não dispara leitura; evento do mesmo space dispara.
  emit!({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'outro' })
  emit!({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'personal' })
  await tick(300)
  assert.equal(calls, 2)

  const beforeBurst = calls
  for (let i = 0; i < 4; i++) {
    emit!({ type: 'entry' })
    await tick(100)
  }
  assert.ok(calls > beforeBurst, 'eventos contínuos não adiam a leitura indefinidamente')

  // Hub antigo (404) degrada para unavailable ("Não informado").
  mode = '404'
  session.reread()
  await tick()
  assert.equal(session.state.kind, 'unavailable')
  assert.equal(session.state.reason, 'unsupported')

  mode = 'ok'
  session.reread()
  await tick()
  assert.equal(session.state.kind, 'ready')
  src.hello = { hostId: 'h2', rev: 1 }
  src.connectedClient = { call: () => new Promise(() => {}), inSpace: () => ({ events: () => () => {} }) }
  src.notify()
  assert.equal(session.state.kind, 'loading')

  // Link cai -> hub_offline explícito.
  src.connectedClient = null
  src.notify()
  assert.equal(session.state.kind, 'hub_offline')
  session.close()
})

test('DoctorSession recupera o stream na mesma origem e cancela retries antigos', async () => {
  const streams: { space: string; event: (e: TowerEvent) => void; end: (error: unknown) => void; stopped: boolean }[] = []
  const requests: string[] = []
  const client: TowerLink = {
    async call(_method, body) {
      const { spaceId } = body as { spaceId: string }
      requests.push(spaceId)
      return { version: 1, hubId: 'h1', spaceId, collectedAt: 1, overall: 'healthy', checks: [
        { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
      ] }
    },
    inSpace: (space) => ({ events(_since, _client, event, end) {
      const stream = { space, event, end, stopped: false }
      streams.push(stream)
      return () => { stream.stopped = true }
    } }),
  }
  const source: TowerSource = { hello: { hostId: 'h1', rev: 1 }, principalEpoch: 1, connectedClient: client, subscribe: () => () => {} }
  const session = new m.DoctorSession()
  session.open(source, 'personal')
  try {
    await tick()
    streams[0]!.end(null)
    assert.equal(session.state.kind, 'hub_offline')
    assert.equal(streams[0]!.stopped, true)
    await tick(320)
    assert.equal(streams.length, 2)
    assert.deepEqual(requests, ['personal', 'personal'])
    assert.equal(session.state.kind, 'ready')

    streams[0]!.end(null)
    streams[1]!.event({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'personal' })
    await tick(320)
    assert.equal(requests.length, 3)
    streams[1]!.end(null)
    session.open(source, 'outro')
    await tick(600)
    assert.deepEqual(streams.map((stream) => stream.space), ['personal', 'personal', 'outro'])
    assert.equal(session.state.report.spaceId, 'outro')
    streams[2]!.end(null)
    session.close()
    await tick(320)
    assert.equal(streams.length, 3)
  } finally {
    session.close()
  }
})

test('DoctorSession reread mantém origem, serializa leituras e descarta resposta após troca de origem', async () => {
  const pending: ((answer: unknown) => void)[] = []
  let calls = 0
  const makeClient = (): TowerLink => ({
    call() {
      calls++
      return new Promise((resolve) => { pending.push(resolve) })
    },
    inSpace: () => ({ events: () => () => {} }),
  })
  const client = makeClient()
  const source: TowerSource = { hello: { hostId: 'h1', rev: 1 }, principalEpoch: 1, connectedClient: client, subscribe: () => () => {} }
  const session = new m.DoctorSession()
  session.open(source, 'personal')
  session.reread()
  assert.equal(calls, 1)
  const nextSource: TowerSource = { hello: { hostId: 'h2', rev: 2 }, principalEpoch: 2, connectedClient: makeClient(), subscribe: () => () => {} }
  session.open(nextSource, 'personal')
  assert.equal(calls, 1)
  pending.shift()!({ version: 1, hubId: 'h1', spaceId: 'personal', collectedAt: 1, overall: 'healthy', checks: [
    { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
  ] })
  await tick()
  assert.equal(session.state.kind, 'loading')
  await tick(300)
  assert.equal(calls, 2)
  pending.shift()!({ version: 1, hubId: 'h2', spaceId: 'personal', collectedAt: 2, overall: 'healthy', checks: [
    { id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' },
  ] })
  await tick()
  assert.equal(session.state.kind, 'ready')
  session.close()
})

test('parseEvent aceita donnaDoctorChanged e donnaControlTowerChanged do JSON cru e reconsulta via HostClient', async () => {
  for (const type of ['donnaDoctorChanged', 'donnaControlTowerChanged'] as const) {
    assert.equal(m.parseEvent(JSON.stringify({ type })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, spaceId: 'personal' })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, hubId: '', spaceId: 'personal' })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, hubId: 'h1' })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, hubId: 'h1', spaceId: '' })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, hubId: ' h1', spaceId: 'personal' })), null)
    assert.equal(m.parseEvent(JSON.stringify({ type, hubId: 'h1', spaceId: 'personal ' })), null)
  }
  assert.deepEqual(
    JSON.parse(JSON.stringify(m.parseEvent(JSON.stringify({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'personal' })))),
    { type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'personal' },
  )
  assert.deepEqual(
    JSON.parse(JSON.stringify(m.parseEvent(JSON.stringify({ type: 'donnaControlTowerChanged', hubId: 'h1', spaceId: 'personal' })))),
    { type: 'donnaControlTowerChanged', hubId: 'h1', spaceId: 'personal' },
  )

  let onLine!: (line: string) => void
  let calls = 0
  const client = new m.HostClient(
    {
      async call(method: string, body: unknown) {
        assert.equal(method, 'donnaHubDoctor')
        assert.deepEqual(JSON.parse(JSON.stringify(body)), { spaceId: 'personal' })
        calls++
        return {
          version: 1,
          hubId: 'h1',
          spaceId: 'personal',
          collectedAt: 1_760_000_000_000 + calls,
          overall: 'healthy',
          checks: [{ id: 'hub', area: 'hub', status: 'ok', code: 'hub_ok', count: null, actionKind: 'none' }],
        }
      },
      stream(_path: string, onData: (line: string) => void) {
        onLine = onData
        return () => {}
      },
    },
    undefined,
    'h1',
  )

  const session = new m.DoctorSession(() => 999)
  session.open({ hello: { hostId: 'h1', rev: 1 }, principalEpoch: 1, connectedClient: client, subscribe: () => () => {} }, 'personal')
  await tick()
  assert.equal(calls, 1)

  onLine(JSON.stringify({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'outro' }))
  onLine('{"type":"donnaControlTowerChanged","hubId":"h2","spaceId":"personal"}')
  onLine('{"type":"donnaDoctorChanged","hubId":"h1 ","spaceId":"personal"}')
  await tick(300)
  assert.equal(calls, 1)
  onLine(JSON.stringify({ type: 'donnaDoctorChanged', hubId: 'h1', spaceId: 'personal' }))
  await tick(300)
  assert.equal(calls, 2)

  onLine(JSON.stringify({ type: 'donnaControlTowerChanged', hubId: 'h1', spaceId: 'personal' }))
  await tick(300)
  assert.equal(calls, 3)

  session.close()
})
