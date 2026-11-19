// MX-217: a queda curta numa loja real (BotStore + FakeHost), com o link trocável de produção.
import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { setImmediate as tick } from 'node:timers/promises'
import { webcrypto } from 'node:crypto'
import { buildSync } from 'esbuild'
import type { Computer } from '../../shared/models'
import type { HostTransport } from '../client/host-client'

type Bundle = {
  BotStore: typeof import('./bot-store').BotStore
  FakeHost: typeof import('../demo/fake-host').FakeHost
  LinkTrocavel: typeof import('./fork-queda-curta').LinkTrocavel
  LojaSegura: typeof import('./fork-queda-curta').LojaSegura
  emQueda: typeof import('./fork-queda-curta').emQueda
  transporteParado: typeof import('../client/fork-transporte-parado').transporteParado
  linkCaido: typeof import('../client/fork-transporte-parado').linkCaido
}
const renderer = new URL('../', import.meta.url).pathname
const code = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'; export { FakeHost } from './demo/fake-host'; export { LinkTrocavel, LojaSegura, emQueda } from './store/fork-queda-curta'; export { transporteParado, linkCaido } from './client/fork-transporte-parado'`, resolveDir: renderer, loader: 'ts' },
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
  const location = { search: '' }
  runInNewContext(code, {
    module, queueMicrotask, URL, URLSearchParams, Date, crypto: webcrypto, Uint8Array, btoa, atob, structuredClone,
    setTimeout: clock, clearTimeout: (id: number) => timers.delete(id), setInterval: clock, clearInterval: (id: number) => timers.delete(id),
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v), removeItem: (k: string) => storage.delete(k) }, location,
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, location, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }) },
  })
  const b = module.exports as Bundle
  const host = new b.FakeHost('2.20.3')
  /** Cada túnel: o mesmo FakeHost, com o registro de quem chamou por qual túnel. */
  const chamadas: string[] = []
  const tunel = (nome: string): HostTransport => ({
    async call(method, raw) { chamadas.push(`${nome}:${method}`); return host.call(method, (raw ?? {}) as Record<string, unknown>) },
    stream(path, onData) { chamadas.push(`${nome}:stream`); const q = new URL(path, 'http://fake.invalid').searchParams; return host.subscribe(Number(q.get('since')), onData, q.get('spaceId') ?? undefined) },
  })
  const link = new b.LinkTrocavel<string>('t1', tunel, b.transporteParado, b.linkCaido)
  const store = new b.BotStore({ id: 'demo-mac', name: 'Hub', signKey: '', urls: [] } as Computer, true, link.transporte, '2.20.3', 'mx217')
  const segura = new b.LojaSegura(store, link, { agora: () => 42, depois: () => 0, cancelar: () => {} })
  store.start()
  const flush = async (ms = 0) => {
    await settle()
    for (const [id, task] of [...timers].filter(([, t]) => t.ms <= ms)) { if (timers.delete(id)) task.fn() }
    await settle()
  }
  return { b, host, store, segura, chamadas, flush, close() { store.retire(); timers.clear() } }
}

test('queda: a autoridade de ação some na hora (sem os 5 s de tolerância) e a conversa fica', async () => {
  const f = fixture()
  try {
    await f.flush()
    assert.equal(f.store.connection.kind, 'online')
    const bots = f.store.bots.size
    assert.ok(bots > 0)
    f.segura.cair(() => assert.fail('não solta dentro do prazo'))
    assert.equal(f.store.connectedClient, null)
    assert.equal(f.store.connection.kind, 'offline')
    assert.equal(f.store.linkCaidoDesde, 42)
    assert.equal(f.b.emQueda(f.store), true)
    await f.flush(20_000)
    assert.equal((f.store.connection as { kind: string }).kind, 'offline')
    assert.equal(f.store.bots.size, bots)
    // Uma ação durante a queda falha fechada, sem tocar túnel nenhum.
    const antes = f.chamadas.length
    await assert.rejects(f.store.ready(Date.now()), (e: Error & { status?: number }) => e.status === 0)
    assert.equal(f.chamadas.slice(antes).filter((c) => c.startsWith('t1:')).length, 0)
  } finally { f.close() }
})

test('volta para outro Hub: sem autoridade até o hello conferir; o espelho do Hub antigo não atravessa', async () => {
  const f = fixture()
  try {
    await f.flush()
    assert.equal(f.store.hello?.hostId, 'demo-host')
    f.segura.cair(() => {})
    await f.flush()
    f.host.hostId = 'outro-hub'
    const bot = [...f.store.bots.keys()][0]!
    f.segura.voltar('t2')
    assert.equal(f.store.connection.kind, 'connecting')
    assert.equal(f.store.connectedClient, null)
    // Ainda em queda até o hello: faixa, Enviar e Permitir continuam desligados.
    assert.equal(f.store.linkCaidoDesde, 42)
    assert.equal(f.b.emQueda(f.store), true)
    // Uma decisão tomada nesse intervalo pertence ao Hub antigo: não atravessa para o novo.
    f.store.respond({ id: 'cartao-antigo', botId: bot, rev: 1 } as never, 'allow_once')
    await f.flush()
    await f.flush(2_000)
    assert.equal(f.store.connection.kind, 'online')
    assert.equal(f.store.linkCaidoDesde, null)
    assert.equal(f.b.emQueda(f.store), false)
    assert.equal(f.store.hello?.hostId, 'outro-hub')
    assert.ok(f.store.connectedClient)
    assert.ok(f.chamadas.some((c) => c === 't2:stream'))
    assert.equal(f.chamadas.some((c) => c.endsWith(':respondPermission')), false)
  } finally { f.close() }
})

test('geração antiga: resposta que chega depois da queda não vale e nada mais sai pelo túnel antigo', async () => {
  const f = fixture()
  try {
    await f.flush()
    const antigo = f.store.connectedClient!
    const pendente = antigo.call('teamSettings', {})
    f.segura.cair(() => {})
    await assert.rejects(pendente, (e: Error & { status?: number; isTransient?: boolean }) => e.status === 0 && e.isTransient === false)
    const antes = f.chamadas.length
    await assert.rejects(antigo.call('teamSettings', {}), (e: Error & { isTransient?: boolean }) => e.isTransient === false)
    f.segura.voltar('t2')
    await assert.rejects(antigo.call('teamSettings', {}))
    assert.equal(f.chamadas.slice(antes).filter((c) => c.startsWith('t1:')).length, 0)
  } finally { f.close() }
})

test('liberação definitiva: o cliente já capturado também para (sem resposta nem chamada pelo túnel antigo)', async () => {
  const f = fixture()
  try {
    await f.flush()
    const antigo = f.store.connectedClient!
    const pendente = antigo.call('teamSettings', {})
    f.segura.revogar()
    await assert.rejects(pendente, (e: Error & { isTransient?: boolean }) => e.isTransient === false)
    const antes = f.chamadas.length
    await assert.rejects(antigo.call('teamSettings', {}))
    assert.equal(f.chamadas.slice(antes).filter((c) => c.startsWith('t1:')).length, 0)
  } finally { f.close() }
})
