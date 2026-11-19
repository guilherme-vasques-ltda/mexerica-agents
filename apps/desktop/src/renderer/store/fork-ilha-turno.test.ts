// MX-251: "Terminou" — a observação do turno é por evento aplicado (onEventApplied), não pela
// assinatura agregada. Um lote do stream aplicado no mesmo microtask (working+idle) chegaria ao
// assinante como só idle; o gancho vê cada transição. BotSync e transporte de fala real; só o
// stream e a ponte são fictícios.
import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../shared/models'
import type { HostTransport } from '../client/host-client'
import { observeTurns, type TurnSeen } from '../../shared/island.ts'

const bundle = buildSync({
  stdin: { contents: `export { BotSync } from './store/bot-sync'`, resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"' },
})
const settle = async () => { for (let i = 0; i < 16; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })

const bot = (id: string, rev: number, status: string, startedAt: number | null) => ({
  id, rev, spaceId: 'pessoal', kind: 'agent', members: [], name: id, description: '', avatarColor: 'blue', avatarShape: 'blob',
  title: '', backend: 'claude', cwd: '/tmp', managedWorkspace: true, status, startedAt, unread: 0, pinned: false, hidden: false, position: 0, lastAt: 0,
})

function fixture() {
  const module = { exports: {} }
  let emit!: Parameters<HostTransport['stream']>[1]
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, setTimeout, clearTimeout,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3' }) },
  })
  const { BotSync } = module.exports as { BotSync: typeof import('./bot-sync').BotSync }
  const transport: HostTransport = {
    async call(method) {
      if (method === 'hello') return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
      if (method === 'teamSettings') return { primaryBotId: null }
      throw new Error(`Operação fora da fixture: ${method}`)
    },
    stream(_path, onData) {
      emit = onData
      queueMicrotask(() => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 0, usage: { providers: [] } })))
      return () => {}
    },
  }
  const store = new BotSync({ id: 'mac', name: 'Mac' } as Computer, false, () => transport)
  store.start()
  return {
    store,
    emit: (event: object) => emit(JSON.stringify(event)),
    close() { store.retire() },
  }
}

test('lote working+idle num microtask só: o gancho por evento vê as duas transições; a assinatura agregada, só o fim', async () => {
  const f = fixture()
  try {
    await settle()
    const agora = 1_000_000
    const seen = new Map<string, TurnSeen>()
    const porEvento: (string | undefined)[] = []
    let perf = 0
    f.store.onEventApplied = () => {
      const b = f.store.bots.get('a')
      porEvento.push(b?.status)
      observeTurns(seen, [{ ref: 'a', status: b?.status ?? 'idle', startedAt: b?.startedAt, watching: false }], { date: () => agora, perf: () => perf })
    }
    const pelaAssinatura: (string | undefined)[] = []
    f.store.subscribe(() => pelaAssinatura.push(f.store.bots.get('a')?.status))
    // O preload entrega o lote inteiro em sequência síncrona, antes do microtask do changed().
    f.emit({ type: 'bot', bot: bot('a', 1, 'working', agora - 61_000) })
    perf = 2_000
    f.emit({ type: 'bot', bot: bot('a', 2, 'idle', null) })
    assert.deepEqual(porEvento, ['working', 'idle'], 'o gancho observa cada evento aplicado')
    assert.equal(seen.get('a')!.ended, true)
    assert.equal(seen.get('a')!.duracaoMs, 61_000 + 2_000, 'a duração do turno sobrevive ao lote')
    await settle()
    assert.deepEqual(pelaAssinatura, ['idle'], 'a notificação agregada só vê o estado final do lote')
  } finally { f.close() }
})

// MX-251 (revisão): o observador é opcional. Uma exceção dele no meio de um lote do stream não
// pode subir pelo apply — no preload (host:stream:data) isso interromperia o loop síncrono e as
// linhas seguintes do lote se perderiam, inclusive falha e aprovação. Sem o isolamento no apply
// este teste falha: o primeiro emit lança e os dois eventos seguintes nunca são aplicados.
test('exceção no observador não interrompe o lote: falha e aprovação seguintes chegam', async () => {
  const f = fixture()
  try {
    await settle()
    let chamadas = 0
    f.store.onEventApplied = () => { chamadas++; throw new Error('observador quebrado') }
    const aprovacao = {
      id: 'e1', seq: 1, botId: 'a', rev: 2, kind: 'permission', turn: 1,
      data: { status: 'pending', toolKind: 'execute', title: 'ls', options: [{ optionId: 'o1', name: 'Permitir', kind: 'allow_once' }] },
      createdAt: 0, updatedAt: 0,
    }
    // Como o preload entrega: as linhas do lote chegam em sequência síncrona.
    f.emit({ type: 'bot', bot: bot('a', 1, 'working', 900_000) })
    f.emit({ type: 'entry', entry: aprovacao })
    f.emit({ type: 'bot', bot: bot('a', 3, 'error', null) })
    assert.equal(chamadas, 3, 'o observador foi chamado e lançou nos três eventos')
    assert.equal(f.store.bots.get('a')?.status, 'error', 'a falha do fim do lote foi aplicada')
    assert.ok(
      f.store.allEntries('a').some((e) => e.kind === 'permission' && e.data.status === 'pending'),
      'a aprovação do meio do lote foi entregue',
    )
    await settle()
  } finally { f.close() }
})

test('o gancho é por evento em qualquer tipo e é limpo na aposentadoria da loja', async () => {
  const f = fixture()
  await settle()
  let chamadas = 0
  f.store.onEventApplied = () => chamadas++
  f.emit({ type: 'bot', bot: bot('a', 1, 'working', 900_000) })
  f.emit({ type: 'usage', usage: { providers: [] } })
  f.emit({ type: 'bot', bot: bot('a', 2, 'idle', null) })
  assert.equal(chamadas, 3, 'cada evento aplicado dispara o gancho, não só mudanças de bot')
  f.close()
  assert.equal(f.store.onEventApplied, null)
})
