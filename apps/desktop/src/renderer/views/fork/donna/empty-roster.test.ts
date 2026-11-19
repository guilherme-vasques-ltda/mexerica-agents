import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../../../shared/models'
import type { HostTransport } from '../../../client/host-client'
import { defaultSelection, initialTrack, trackSelection, type SelectionTrack } from './default-selection.ts'

// Abertura no Berga, integração. BotSync, AppModel, cliente, a projeção real da tela (computerSections → paleta de
// paletteProjections, a mesma da busca) e a montagem do gancho (startupInput) são reais; só o transporte, a ponte do
// Electron e o relógio são fictícios. Lista vazia: painel neutro; "Novo bot" só depois de 5 s online sem bot visível
// (relógio zerado em toda queda), ou antes com o zero explícito do Hub.

const bundle = buildSync({
  stdin: {
    contents: `export { BotSync } from './store/bot-sync'; export { AppModel } from './store/app-model';
      export { computerSections } from './lib/computer-roster'; export { paletteProjections } from './lib/principal';
      export { startupInput, selectionNow } from './views/fork/donna/use-default-selection';
      export { openPrincipal } from './views/fork/donna/open-principal';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"' },
})
type Bundle = {
  BotSync: typeof import('../../../store/bot-sync').BotSync
  AppModel: typeof import('../../../store/app-model').AppModel
  computerSections: typeof import('../../../lib/computer-roster').computerSections
  paletteProjections: typeof import('../../../lib/principal').paletteProjections
  startupInput: typeof import('./use-default-selection').startupInput
  selectionNow: typeof import('./use-default-selection').selectionNow
  openPrincipal: typeof import('./open-principal').openPrincipal
}

/** Relógio controlado: `Date.now` e os timers do bundle andam só com `advance`. */
function clock() {
  let now = 1_000_000
  let seq = 0
  const timers = new Map<number, { at: number; fn: () => void }>()
  class FakeDate extends Date { static override now() { return now } }
  return {
    Date: FakeDate,
    setTimeout: (fn: () => void, ms = 0) => { timers.set(++seq, { at: now + ms, fn }); return seq },
    clearTimeout: (id: number) => { timers.delete(id) },
    now: () => now,
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

// A ponte do Electron (`window.codync`) responde a tudo sem efeito.
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
function load(time = clock()) {
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date: time.Date, setTimeout: time.setTimeout, clearTimeout: time.clearTimeout,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3' }), location: { search: '' } },
  })
  return module.exports as Bundle
}

/** O estado da conexão sem o estreitamento de tipo das asserções anteriores. */
const kind = (f: { store: { connection: { kind: string } } }): string => f.store.connection.kind
const settle = async () => { for (let i = 0; i < 8; i++) await tick() }
const conversation = (botId: string) => ({ id: botId, spaceId: 'pessoal', botId, title: '', revision: 1, stateRevision: 1, primary: true, closed: false,
  readRevision: 0, engineAccountId: null, engineSelection: null, accountRevision: null, continuity: null })
const bot = (id: string, rev: number, hidden = false) => ({ id, rev, spaceId: 'pessoal', kind: 'agent', members: [], name: id, description: '',
  avatarColor: 'blue', avatarShape: 'blob', title: '', backend: 'claude', cwd: '/tmp', managedWorkspace: true, status: 'idle', unread: 0, pinned: false,
  hidden, position: 0, lastAt: 0 })
const donna = { kind: 'select', ref: { computerId: 'mac', botId: 'donna' } }

/**
 * Um Hub cujo `hello` chega antes dos eventos `bot` e cuja `teamSettings` (a principal) responde antes deles.
 * `announced` undefined é o Hub antigo, sem `conversations`. `down` faz o Hub recusar novas conexões.
 */
function fixture(announced: string[] | undefined, cached: ReturnType<typeof bot>[] = []) {
  const time = clock()
  const { BotSync, AppModel, computerSections, paletteProjections, startupInput, selectionNow } = load(time)
  let emit!: Parameters<HostTransport['stream']>[1]
  let end!: Parameters<HostTransport['stream']>[2]
  let down = false
  const transport: HostTransport = {
    async call(method) {
      if (down) throw Object.assign(new Error('offline'), { status: 0 })
      if (method === 'hello') return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
      if (method === 'teamSettings') return { primaryBotId: 'donna' }
      throw new Error(`Operação fora da fixture: ${method}`)
    },
    stream(_path, onData, onEnd) {
      emit = onData; end = onEnd
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 9, usage: { providers: [] },
        ...(announced ? { spacesContract: 1, spaceId: 'pessoal', conversations: announced.map(conversation) } : {}) })))
      return () => {}
    },
  }
  const computer = { id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer
  const store = new BotSync(computer, false, () => transport)
  for (const b of cached) store.bots.set(b.id, b as never) // o mirror em cache
  const app = new AppModel(false)
  app.attach(computer, store as never) // inicia o stream, como a janela
  let track: SelectionTrack | null = null
  /** A decisão como a tela a monta: lista visível de useComputerRoster, paleta de useRosterFilter, startupInput. */
  const decide = () => {
    const sections = computerSections(app.computers, app.roster).map((section) => ({ ...section, store: app.store(section.computer.id)! }))
    const visible = sections.flatMap((section) => section.items)
    const palette = paletteProjections(sections).flatMap((projection) => projection.palette)
    const at = { ...selectionNow(app, visible), autoSeq: -1 }
    track = track ? trackSelection(track, at) : initialTrack(at)
    const decision = defaultSelection(startupInput(app, { items: visible, palette, rows: visible, hub: app.store('mac'), stores: [app.store('mac')!], connecting: false }, track, time.now()))
    // Cópias neste realm: os arrays do bundle vêm de outro contexto do vm.
    return { decision, palette: Array.from(palette, (item) => item.bot.id), visible: Array.from(visible, (item) => item.bot.id) }
  }
  return {
    store, app, time, decide,
    emit: (event: object) => emit(JSON.stringify(event)),
    drop(hubDown: boolean) { down = hubDown; end(Object.assign(new Error('stream'), { status: 0 }) as never) },
    up() { down = false },
    close: () => store.retire(),
  }
}

test('R4-1: cache só com a Donna oculta e teamSettings antes do evento bot: a projeção da tela a mostra, mas a abertura espera', async () => {
  const f = fixture(['donna'], [bot('donna', 1, true)])
  try {
    await settle()
    assert.equal(kind(f), 'online')
    assert.equal(f.store.principal.phase, 'ready', 'a principal foi confirmada antes do evento bot')
    const first = f.decide()
    assert.deepEqual(first.visible, [], 'nenhum bot visível recebido')
    assert.deepEqual(first.palette, ['donna'], 'a paleta da tela reinclui a principal oculta (D121)')
    assert.deepEqual(first.decision, { kind: 'wait' }, 'a projeção não conta como bot visível')
    await f.time.advance(4900)
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(100)
    assert.deepEqual(f.decide().decision, { kind: 'empty' }, 'sem evento novo, "Novo bot" aos 5 s')
    // O evento novo traz o Berga visível: abre.
    f.emit({ type: 'bot', rev: 5, bot: bot('donna', 5) }); await settle()
    assert.deepEqual(f.decide().decision, donna)
  } finally { f.close() }
})

test('R4-1: a Donna oculta escolhida pela busca (paleta) continua aberta: a paleta só protege a escolha da pessoa', async () => {
  const f = fixture(['donna', 'joio'], [bot('donna', 1, true)])
  try {
    await settle()
    f.emit({ type: 'bot', rev: 4, bot: bot('joio', 4) }); await settle()
    f.decide()
    f.app.select({ computerId: 'mac', botId: 'donna' }) // a pessoa escolhe a Donna oculta na busca
    assert.deepEqual(f.decide().decision, { kind: 'keep' })
  } finally { f.close() }
})

test('R4-2: queda confirmada pelo timer zera o relógio; a reconexão recomeça os 5 s', async () => {
  const f = fixture(undefined)
  try {
    await settle()
    await f.time.advance(6000)
    assert.deepEqual(f.decide().decision, { kind: 'empty' }, 'lista vazia há mais de 5 s online')
    f.drop(true); await settle()
    assert.equal(kind(f), 'online', 'a tolerância segura a queda na tela')
    assert.equal(f.store.emptyWatch.onlineAt, null, 'mas o relógio já zerou')
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(5000)
    assert.equal(kind(f), 'offline', 'queda confirmada pelo timer')
    assert.equal(f.store.emptyWatch.onlineAt, null)
    f.up()
    await f.time.advance(4000) // a próxima tentativa (backoff 1 s, 2 s, 4 s) reconecta
    assert.equal(kind(f), 'online')
    const back = f.store.emptyWatch.onlineAt
    assert.ok(back !== null && back > 1_000_000 + 6000, 'relógio novo no link novo, não o do primeiro link')
    assert.ok(f.store.emptyWatch.remaining(f.time.now())! > 0, 'não reaproveita o relógio antigo')
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(back! + 4999 - f.time.now())
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(1)
    assert.deepEqual(f.decide().decision, { kind: 'empty' })
  } finally { f.close() }
})

test('R4-2: queda curta, dentro da tolerância, também recomeça os 5 s', async () => {
  const f = fixture(undefined)
  try {
    await settle()
    await f.time.advance(6000)
    assert.deepEqual(f.decide().decision, { kind: 'empty' })
    f.drop(false); await settle()
    await f.time.advance(1000) // reconecta na primeira tentativa, antes de a queda ser confirmada
    assert.equal(kind(f), 'online')
    assert.equal(f.store.emptyWatch.remaining(f.time.now()), 5000)
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(5000)
    assert.deepEqual(f.decide().decision, { kind: 'empty' })
  } finally { f.close() }
})

test('cenário 2 da R3: Hub sem o contrato de spaces espera os 5 s; o bot que chega abre', async () => {
  const f = fixture(undefined)
  try {
    await settle()
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    await f.time.advance(4999)
    assert.deepEqual(f.decide().decision, { kind: 'wait' })
    f.emit({ type: 'bot', rev: 5, bot: bot('donna', 5) }); await settle()
    assert.deepEqual(f.decide().decision, donna)
  } finally { f.close() }
})

test('zero explícito no hello mostra "Novo bot" sem esperar', async () => {
  const f = fixture([])
  try {
    await settle()
    assert.deepEqual(f.decide().decision, { kind: 'empty' })
  } finally { f.close() }
})

test('R3 P2-1: clicar na Donna já aberta (handler real do DonnaCard) marca a escolha como da pessoa', () => {
  const { AppModel, openPrincipal } = load()
  const app = new AppModel(false)
  const ref = { computerId: 'mac', botId: 'donna' }
  // A abertura automática escolheu o Berga (autoSeq = o select dele).
  const autoSeq = app.selectionSeq + 1
  app.select(ref)
  let track: SelectionTrack = trackSelection({ seq: 0, key: null, manual: false, outside: false }, { seq: app.selectionSeq, key: 'mac/donna', listed: true, autoSeq })
  assert.equal(track.manual, false)
  openPrincipal(app, ref)
  assert.equal(app.selectionSeq, autoSeq + 1, 'o clique passa por select mesmo sem mudar a referência')
  track = trackSelection(track, { seq: app.selectionSeq, key: 'mac/donna', listed: true, autoSeq })
  assert.equal(track.manual, true)
})
