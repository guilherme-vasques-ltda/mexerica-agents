import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../../../shared/models'
import type { HostTransport } from '../../../client/host-client'

// D541 integração: BotStore, BotSync, HostClient e ProjectsSession REAIS; só o transporte, a ponte do Electron e o
// relógio são fictícios (padrão de computador/screen-epoch.test.ts). No app, `principalEpoch` sobe a CADA hello,
// inclusive na reconexão do mesmo Hub: queda passageira não pode apagar a fotografia (nem desmontar a folha com o
// rascunho) nem perder o pedido incerto; só uma leitura autorizada nova libera gravar.

const bundle = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'; export { ProjectsSession } from './views/fork/projetos/projetos-session'`, resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
type Bundle = { BotStore: typeof import('../../../store/bot-store').BotStore; ProjectsSession: typeof import('./projetos-session').ProjectsSession }

function clock() {
  let now = 1_000_000
  let seq = 0
  const timers = new Map<number, { at: number; fn: () => void }>()
  class FakeDate extends Date { static override now() { return now } }
  return {
    Date: FakeDate,
    setTimeout: (fn: () => void, ms = 0) => { timers.set(++seq, { at: now + ms, fn }); return seq },
    clearTimeout: (id: number) => { timers.delete(id) },
    async advance(ms: number) {
      const end = now + ms
      for (;;) {
        const next = [...timers.entries()].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
        if (!next) break
        timers.delete(next[0])
        now = next[1].at
        next[1].fn()
        await settle()
      }
      now = end
      await settle()
    },
  }
}
const settle = async () => { for (let i = 0; i < 8; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
const deferred = () => { let resolve!: (v: unknown) => void; let reject!: (e: unknown) => void; const promise = new Promise((r, j) => { resolve = r; reject = j }); return { promise, resolve, reject } }

const P = '11111111-1111-4111-8111-111111111111'
const T = '22222222-2222-4222-8222-222222222222'
const project = (revision: number) => ({ id: P, name: 'Vídeo X', objective: '', coordinatorBotId: null, state: 'active', revision, createdAt: 1, updatedAt: revision })
const task = { id: T, parentTaskId: null, assigneeBotId: null, title: 'Roteiro', description: '', acceptance: '', evidence: '', state: 'backlog' }

function fixture() {
  const time = clock()
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date: time.Date, setTimeout: time.setTimeout, clearTimeout: time.clearTimeout,
    crypto: globalThis.crypto,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const { BotStore, ProjectsSession } = module.exports as Bundle
  let revision = 1
  /** Leituras/gravações seguradas: o teste decide quando (e se) o Hub responde. */
  let hold: Set<string> = new Set()
  const held: Record<string, ReturnType<typeof deferred>[]> = {}
  const calls: string[] = []
  let endMain!: Parameters<HostTransport['stream']>[2]
  const transport: HostTransport = {
    async call(method, body) {
      const b = body as Record<string, unknown>
      if (method === 'hello') return { hostId: 'hub', version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
      if (method === 'teamSettings') return { primaryBotId: null }
      if (!method.startsWith('donnaProject')) return {}
      calls.push(method)
      if (hold.has(method)) { const d = deferred(); (held[method] ??= []).push(d); return d.promise }
      const snap = { version: 1, hubId: 'hub', spaceId: b.spaceId, project: project(revision), tasks: [task], history: [], historyOmitted: 0, bots: [] }
      if (method === 'donnaProjects') return { version: 1, hubId: 'hub', spaceId: b.spaceId, projects: [project(revision)], bots: [], capabilities: ['manualProjects'] }
      if (method === 'donnaProject') return snap
      return {}
    },
    stream(path, onData, onEnd) {
      // O stream do Space (Projetos) não manda hello; o principal manda, como o Hub.
      if (!path.includes('spaceId=')) {
        endMain = onEnd
        queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 9, usage: { providers: [] } })))
      }
      return () => {}
    },
  }
  const store = new BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => transport)
  store.start()
  const session = new ProjectsSession(() => 7, (() => { let n = 0; return () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}` })())
  return {
    store, session, time, calls, held,
    hold: (...methods: string[]) => { hold = new Set(methods) },
    setRevision: (r: number) => { revision = r },
    drop: () => endMain(Object.assign(new Error('stream'), { status: 0 }) as never),
    close: () => { session.close(); store.retire() },
  }
}

async function opened() {
  const f = fixture()
  await settle()
  f.session.open(f.store as never, 'loja')
  await settle()
  f.session.openProject(P)
  await settle()
  assert.equal(f.session.canWrite, true, `pronto: ${JSON.stringify({ phase: f.session.phase, view: f.session.view, link: f.session.link, events: f.session.events, calls: f.calls, hello: !!f.store.hello, client: !!f.store.connectedClient, auth: f.session.authority })}`)
  return f
}

test('queda e reconexão do mesmo Hub (principalEpoch sobe no hello): fotografia fica, só leitura até a leitura nova', async () => {
  const f = await opened()
  try {
    const epoch = f.store.principalEpoch
    f.hold('donnaProjects', 'donnaProject')
    f.drop(); await settle()
    assert.equal(f.session.stale, true)
    assert.equal(f.session.project?.tasks[0]?.title, 'Roteiro', 'na queda a fotografia fica')
    await f.time.advance(6000)
    assert.ok(f.store.principalEpoch > epoch, 'o hello da reconexão subiu a época (caminho real do BotSync)')
    assert.ok(f.store.connectedClient, 'reconectou')
    assert.equal(f.session.project?.tasks[0]?.title, 'Roteiro', 'reconexão não apaga a fotografia (nem desmonta a folha com o rascunho)')
    assert.equal(f.session.list?.projects.length, 1)
    assert.equal(f.session.canWrite, false, 'antes da leitura autorizada da conexão nova, nada grava')
    assert.equal(f.session.stale, true, 'link e eventos vivos, mas a fotografia segue antiga durante a releitura')
    assert.equal(f.session.confirming, true, 'a faixa diz que está conferindo, não “manual”')
    // A leitura da conexão nova chega: só ela libera.
    f.held.donnaProjects!.at(-1)!.resolve({ version: 1, hubId: 'hub', spaceId: 'loja', projects: [project(2)], bots: [], capabilities: ['manualProjects'] })
    await settle()
    f.held.donnaProject!.at(-1)!.resolve({ version: 1, hubId: 'hub', spaceId: 'loja', project: project(2), tasks: [task], history: [], historyOmitted: 0, bots: [] })
    await settle()
    assert.equal(f.session.canWrite, true)
    assert.equal(f.session.project?.project.revision, 2)
    assert.equal(f.session.stale, false, 'só a leitura autorizada tira o “antigo”')
  } finally { f.close() }
})

test('gravação em voo durante a queda: depois da reconexão fica incerta com o MESMO pedido (não some, não reenvia sozinha)', async () => {
  const f = await opened()
  try {
    f.hold('donnaProjectSave')
    const snap = f.session.project!
    const saving = f.session.save({ name: 'Vídeo X', objective: '', coordinatorBotId: null, state: 'active', tasks: [{ ...task, state: 'done' }] }, snap)
    await settle()
    const id = f.session.write?.clientRequestId
    f.drop(); await settle()
    await f.time.advance(6000)
    assert.equal(f.session.project?.tasks[0]?.title, 'Roteiro')
    // A resposta da época anterior chega depois do novo hello: não pinta; o pedido fica para Tentar de novo.
    f.held.donnaProjectSave![0]!.reject(Object.assign(new Error('stream'), { status: 0 }))
    const r = await saving
    assert.equal(r.ok, false)
    assert.equal(f.session.write?.status, 'uncertain')
    assert.equal(f.session.write?.clientRequestId, id, 'mesma chave para o Hub não duplicar')
    assert.equal(f.calls.filter((c) => c === 'donnaProjectSave').length, 1, 'nada reenviado sozinho')
  } finally { f.close() }
})

test('outra fonte (outro BotStore: outra conta/contexto do mesmo computador) apaga tudo', async () => {
  const f = await opened()
  const g = fixture()
  try {
    await settle()
    f.session.open(g.store as never, 'loja')
    assert.equal(f.session.project, null)
    assert.equal(f.session.list, null)
    assert.equal(f.session.write, null)
  } finally { g.store.retire(); f.close() }
})
