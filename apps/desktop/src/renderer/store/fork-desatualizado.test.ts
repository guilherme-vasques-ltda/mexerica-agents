// Decisão do dono (16/11, incidente "Atualize a Mexerica"): mismatch de versão de UM computador fica
// isolado nele — estado `desatualizado` limpo (alvo/versão/mínimo, sem texto), leitura do cache
// visível, ações no Hub bloqueadas com motivo — e destrava sozinho num hello bom, com recheck em
// backoff limitado (sem laço apertado e sem reabrir o app). Versão ausente/ilegível segue bloqueada.
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
  Desatualizado: typeof import('./fork-version-gate').Desatualizado
  VersionGate: typeof import('./fork-version-gate').VersionGate
  AppModel: typeof import('./app-model').AppModel
  ComputerAccess: typeof import('./computer-access').ComputerAccess
  FakeHost: typeof import('../demo/fake-host').FakeHost
}
const renderer = new URL('../', import.meta.url).pathname
const code = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'; export { Desatualizado, VersionGate } from './store/fork-version-gate'; export { AppModel } from './store/app-model'; export { ComputerAccess } from './store/computer-access'; export { FakeHost } from './demo/fake-host'`, resolveDir: renderer, loader: 'ts' },
  tsconfig: new URL('../../../tsconfig.json', import.meta.url).pathname, bundle: true, platform: 'node', format: 'cjs', write: false,
  loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
}).outputFiles[0].text
const settle = async () => { for (let i = 0; i < 16; i++) await tick() }

/** Sem prova da geração não há culpa da versão: falha de link (status 0), nunca `Desatualizado`. */
const semCulpa = (b: Bundle) => (e: unknown) => !(e instanceof b.Desatualizado) && (e as { status?: number }).status === 0

type Resposta = { version: string | null; minApp?: string | null }

function fixture() {
  const timers = new Map<number, { fn: () => void; ms: number }>()
  const agendados: number[] = []
  let timerId = 0
  const clock = (fn: () => void, ms = 0) => { agendados.push(ms); timers.set(++timerId, { fn, ms }); return timerId }
  const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })
  const module = { exports: {} }
  const codync = Object.assign(Object.create(bridge as object), { appVersion: '2.20.3' as string | undefined, platform: 'darwin' })
  const storage = new Map<string, string>()
  const writes: string[] = []
  const location = { search: '' }
  runInNewContext(code, {
    module, queueMicrotask, URL, URLSearchParams, Date, crypto: webcrypto, Uint8Array, btoa, atob, structuredClone,
    setTimeout: clock, clearTimeout: (id: number) => timers.delete(id), setInterval: clock, clearInterval: (id: number) => timers.delete(id),
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => { writes.push(k); storage.set(k, v) }, removeItem: (k: string) => storage.delete(k) }, location,
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, location, codync },
  })
  const b = module.exports as Bundle
  const host = new b.FakeHost('2.20.3')
  const chamadas: string[] = []
  /** A resposta de hello de cada túnel, controlada pelo teste (versão baixa, sem versão, boa). */
  const respostas = new Map<string, Resposta>()
  const holds = new Map<string, Promise<void>>()
  const streams: { nome: string; since: number; data: (line: string) => void; end: (e: null) => void }[] = []
  const hold = (nome: string, method: string) => {
    let release!: () => void
    const key = `${nome}:${method}`
    holds.set(key, new Promise<void>(resolve => { release = resolve }))
    return () => { holds.delete(key); release() }
  }
  const tunel = (nome: string): HostTransport => ({
    async call(method, raw) {
      chamadas.push(`${nome}:${method}`)
      if (method !== 'hello') await holds.get(`${nome}:${method}`)
      const resposta = await host.call(method, (raw ?? {}) as Record<string, unknown>)
      const sobre = respostas.get(nome)
      if (method === 'hello' && sobre) {
        const h = resposta as Record<string, unknown>
        if (sobre.version === null) { delete h.version; delete h.minApp }
        else { h.version = sobre.version; if (Object.hasOwn(sobre, 'minApp')) { if (sobre.minApp === undefined) delete h.minApp; else h.minApp = sobre.minApp } }
      }
      if (method === 'hello') await holds.get(`${nome}:${method}`)
      return resposta
    },
    stream(path, onData, onEnd) {
      chamadas.push(`${nome}:stream`)
      const q = new URL(path, 'http://fake.invalid').searchParams
      streams.push({ nome, since: Number(q.get('since')), data: onData, end: onEnd })
      return host.subscribe(Number(q.get('since')), onData, q.get('spaceId') ?? undefined)
    },
  })
  const loja = (id: string, nome: string, resposta?: Resposta, versaoApp?: string) => {
    if (resposta) respostas.set(nome, resposta)
    return new b.BotStore({ id, name: 'Hub', signKey: '', urls: [] } as Computer, false, () => tunel(nome), versaoApp, 'desat')
  }
  const flush = async (ms = 0) => {
    await settle()
    for (const [id, task] of [...timers].filter(([, t]) => t.ms <= ms)) { if (timers.delete(id)) task.fn() }
    await settle()
  }
  return { b, host, chamadas, loja, hold, streams, writes, codync, flush, storage, agendados, respostas, timers, close() { timers.clear() } }
}

test('mismatch de um computador: desatualizado com motivo, cache visível, outro computador intacto', async () => {
  const f = fixture()
  try {
    const ruim = f.loja('mac-velho', 'tRuim', { version: '2.0.0', minApp: '2.0.0' })
    const bom = f.loja('mac-bom', 'tBom')
    ruim.start(); bom.start()
    await f.flush()
    // Estado limpo exposto, sem texto.
    assert.deepEqual({ ...ruim.desatualizado }, { alvo: 'hub', encontrada: '2.0.0', minimo: '2.20.1' })
    assert.equal(ruim.connectedClient, null)
    assert.equal(ruim.connection.kind, 'online')
    // O stream não abre e a ação que exige o Hub falha na hora, com motivo.
    assert.ok(!f.chamadas.includes('tRuim:stream'))
    await assert.rejects(ruim.ready(0), (e: unknown) => (e as { frase?: { chave?: string } }).frase?.chave === 'spaces.recusa.atualize')
    // O outro computador segue normal: stream aberto e dados chegando.
    assert.ok(f.chamadas.includes('tBom:stream'))
    assert.ok(bom.connectedClient)
    assert.ok(bom.roster.length > 0)
    // Recheck: o próximo hello sai depois de 30 s e o seguinte só depois de 60 s — backoff não aperta.
    const hellos = () => f.chamadas.filter((c) => c === 'tRuim:hello').length
    assert.equal(hellos(), 1)
    const antes = f.agendados.length
    await f.flush(30_000)
    assert.equal(hellos(), 2)
    assert.ok(f.agendados.slice(antes).includes(60_000), `esperava recheck de 60 s, agendados: ${f.agendados.slice(antes)}`)
    assert.ok(ruim.desatualizado)
    // Hello bom: destrava sozinho, abre o stream e volta a gravar o cache — sem reabrir o app.
    f.respostas.set('tRuim', { version: '2.20.3' })
    await f.flush(60_000)
    assert.equal(ruim.desatualizado, null)
    assert.ok(ruim.connectedClient)
    assert.ok(f.chamadas.includes('tRuim:stream'))
    await f.flush(2000) // o catch-up do FakeHost entrega num timer de 0 ms; o save sai 2 s depois
    assert.ok(ruim.roster.length > 0)
    await f.flush(2000) // o save agendado pelos eventos do catch-up
    const cache = f.storage.get([...f.storage.keys()].find((k) => k.startsWith('codync-mirror-[') && k.includes('"mac-velho"'))!)
    assert.ok(cache, 'cache do computador destravado foi gravado')
    assert.match(JSON.parse(cache).stamp as string, /2\.20\.3$/)
    // O Hub piora de novo: os dados já lidos ficam visíveis (nada é zerado), o estado volta.
    f.respostas.set('tRuim', { version: '2.0.0', minApp: '2.0.0' })
    ruim.restartStream()
    await f.flush()
    assert.ok(ruim.desatualizado)
    assert.equal(ruim.connectedClient, null)
    assert.ok(ruim.roster.length > 0, 'cache/roster do computador continuam visíveis sob mismatch')
    // E o outro computador continua intacto durante todo o ciclo.
    assert.ok(bom.connectedClient)
    assert.ok(bom.roster.length > 0)
    ruim.retire(); bom.retire()
  } finally { f.close() }
})

test('hello sem versão legível: fail-closed, continua bloqueado', async () => {
  const f = fixture()
  try {
    const sem = f.loja('mac-sem-versao', 'tSem', { version: null })
    sem.start()
    await f.flush()
    assert.deepEqual({ ...sem.desatualizado }, { alvo: 'hub', encontrada: null, minimo: '2.20.1' })
    assert.equal(sem.connectedClient, null)
    assert.ok(!f.chamadas.includes('tSem:stream'))
    await assert.rejects(sem.ready(0))
    sem.retire()
  } finally { f.close() }
})

test('P2-1: nenhum RPC sai por caminho nenhum — client público some sob mismatch', async () => {
  const f = fixture()
  try {
    const ruim = f.loja('mac-rpc', 'tRpc', { version: '2.0.0', minApp: '2.0.0' })
    ruim.start()
    await f.flush()
    assert.equal(ruim.connection.kind, 'online')
    // O guarda único: `store.client` (lido por computer-access, drafts/marketplace, views) é null.
    assert.equal(ruim.client, null)
    assert.equal(ruim.connectedClient, null)
    // O único tráfego deste túnel é o hello de rechecagem — nada de screenStatus, connectors, stream.
    assert.deepEqual([...new Set(f.chamadas.filter((c) => c.startsWith('tRpc:')))], ['tRpc:hello'])
    // Nem no intervalo entre trocar o transporte e o novo hello (restartStream) algo vaza.
    ruim.restartStream()
    assert.equal(ruim.client, null)
    assert.equal(ruim.connectedClient, null)
    await f.flush()
    assert.equal(ruim.client, null)
    assert.deepEqual([...new Set(f.chamadas.filter((c) => c.startsWith('tRpc:')))], ['tRpc:hello'])
    ruim.retire()
  } finally { f.close() }
})

test('P2-2: mismatch não sobrescreve o cache; a releitura pendente ocorre ao destravar', async () => {
  const f = fixture()
  try {
    const loja = f.loja('mac-cache', 'tCache')
    loja.start()
    await f.flush()
    await f.flush(2000) // save agendado pelos eventos do catch-up
    const chave = [...f.storage.keys()].find((k) => k.startsWith('codync-mirror-[') && k.includes('"mac-cache"'))!
    const antes = f.storage.get(chave)!
    assert.ok(JSON.parse(antes).bots.length > 0, 'cache bom gravado antes do mismatch')
    // O Hub piora: nem o hello incompatível nem dados parciais tocam o cache.
    f.respostas.set('tCache', { version: '2.20.2', minApp: '9.9.9' })
    loja.restartStream()
    await f.flush()
    assert.ok(loja.desatualizado)
    await f.flush(10_000) // qualquer saveTimer disparado sob mismatch não grava
    loja.saveCache() // nem chamado direto
    assert.equal(f.storage.get(chave), antes, 'cache preservado byte a byte sob mismatch')
    // Destravou com o MESMO version (só minApp mudou): a releitura pendente não se perde —
    // o cache novo é regravado com o stamp do Hub compatível.
    f.respostas.set('tCache', { version: '2.20.2', minApp: '9.9.9' })
    await f.flush(30_000)
    assert.ok(loja.desatualizado)
    f.respostas.set('tCache', { version: '2.20.2', minApp: '2.20.1' })
    await f.flush(60_000)
    assert.equal(loja.desatualizado, null)
    await f.flush(4000)
    await f.flush(2000)
    assert.match(JSON.parse(f.storage.get(chave)!).stamp as string, /2\.20\.2$/)
    assert.equal(f.streams.at(-1)?.since, 0, 'releitura pendente volta ao rev zero')
    loja.retire()
  } finally { f.close() }
})

test('P2-3: o timer de rechecagem é cancelado no retire e no restart — sem vazamento', async () => {
  const f = fixture()
  try {
    const hellos = () => f.chamadas.filter((c) => c === 'tTimer:hello').length
    const loja = f.loja('mac-timer', 'tTimer', { version: '2.0.0', minApp: '2.0.0' })
    loja.start()
    await f.flush()
    assert.equal(hellos(), 1)
    // Restart no meio do backoff: o timer antigo é cancelado e nasce um ciclo só — sem dois timers.
    loja.restartStream()
    await f.flush()
    assert.equal(hellos(), 2)
    const rechecks = () => [...f.timers.values()].filter((t) => t.ms >= 30_000).length
    assert.equal(rechecks(), 1, 'um único timer de rechecagem pendente')
    await f.flush(30_000)
    assert.equal(hellos(), 3, 'só o timer novo disparou — o antigo foi cancelado')
    // Retire durante o backoff: nenhum hello depois e nenhum timer pendente.
    loja.retire()
    assert.equal([...f.timers.values()].filter((t) => t.ms >= 30_000).length, 0)
    await f.flush(4 * 60_000)
    assert.equal(hellos(), 3, 'nenhum hello após o retire')
  } finally { f.close() }
})

test('P2-4: versão do app ilegível é fail-closed; dev segue livre', async () => {
  const f = fixture()
  try {
    const ilegivel = f.loja('mac-ilegivel', 'tIlegivel', { version: '2.20.3', minApp: '2.20.1' }, 'nao-e-versao')
    ilegivel.start()
    await f.flush()
    assert.deepEqual({ ...ilegivel.desatualizado }, { alvo: 'app', encontrada: 'nao-e-versao', minimo: '2.20.1' })
    assert.equal(ilegivel.client, null)
    assert.equal(ilegivel.connectedClient, null)
    await assert.rejects(ilegivel.ready(0))
    ilegivel.retire()
    // 'dev' é a exceção prevista pelo compat.ts: sem número por definição, não tranca.
    const dev = f.loja('mac-dev', 'tDev', { version: '2.20.3', minApp: '2.20.1' }, 'dev')
    dev.start()
    await f.flush()
    assert.equal(dev.desatualizado, null)
    assert.ok(dev.connectedClient)
    assert.ok(f.chamadas.includes('tDev:stream'))
    dev.retire()
  } finally { f.close() }
})

test('P2-5: ação bloqueada por mismatch devolve motivo observável — nunca falha calada', async () => {
  const f = fixture()
  try {
    const loja = f.loja('mac-acao', 'tAcao', { version: '2.0.0', minApp: '2.0.0' })
    loja.start()
    await f.flush()
    assert.ok(loja.desatualizado)
    // Fire-and-forget (parar o bot): nenhum RPC sai e a recusa tipada fica no estado.
    const before = loja.getVersion()
    const result = await loja.stop('qualquer-bot')
    assert.equal(result.ok, false)
    if (!result.ok) assert.ok(result.error instanceof f.b.Desatualizado)
    await f.flush()
    assert.ok(loja.getVersion() > before, 'store notificou sua própria recusa')
    assert.ok(!f.chamadas.includes('tAcao:stop'), 'nenhum RPC de ação para o Hub incompatível')
    // A frase é criada dentro do bundle (outro protótipo): compara a chave tipada.
    assert.equal((loja.acaoRecusada as { chave?: string } | null)?.chave, 'spaces.recusa.atualize')
    assert.equal(loja.lastError, null, 'sem erro genérico — a recusa é tipada')
    // Destravou: a marcação limpa e a próxima ação segue ao Hub.
    f.respostas.set('tAcao', { version: '2.20.3' })
    await f.flush(30_000)
    assert.equal(loja.desatualizado, null)
    assert.equal(loja.acaoRecusada, null)
    loja.stop('qualquer-bot')
    await f.flush()
    assert.ok(f.chamadas.includes('tAcao:stop'))
    loja.retire()
  } finally { f.close() }
})

test('hello pedindo app novo: desatualizado marcado para o app', async () => {
  const f = fixture()
  try {
    const app = f.loja('mac-app-novo', 'tApp', { version: '2.20.3', minApp: '9.9.9' })
    app.start()
    await f.flush()
    assert.deepEqual({ ...app.desatualizado }, { alvo: 'app', encontrada: '2.20.3', minimo: '9.9.9' })
    assert.equal(app.connectedClient, null)
    app.retire()
  } finally { f.close() }
})


test('compatível → restart com hello pendente → incompatível: revoga RPC, ready, stream, saves e cache', async () => {
  const f = fixture()
  try {
    const store = f.loja('pending', 'p')
    store.start(); await f.flush(); await f.flush(2000)
    const client = store.client!
    const scoped = client.inSpace('pessoal')
    assert.equal(scoped.boundHubId, store.hello?.hostId)
    const stream = f.streams.at(-1)!
    const bot = store.roster[0]!
    // Uma escrita já saiu no transporte anterior, mas sua resposta ainda não voltou.
    const releaseSave = f.hold('p', 'updateBot')
    const save = store.save({ ...bot, name: 'resposta atrasada' } as never)
    const rejectedSave = assert.rejects(save, semCulpa(f.b))
    await settle()
    const oldEntries = JSON.stringify([...store.entries])
    stream.data(JSON.stringify({ type: 'usage', usage: { providers: [] } }))
    assert.ok([...f.timers.values()].some(t => t.ms === 2000), 'há save do transporte antigo agendado')
    const cache = new Map(f.storage)
    const writes = f.writes.length
    const releaseHello = f.hold('p', 'hello')
    f.respostas.set('p', { version: '2.0.0', minApp: '2.20.1' })
    const calls = f.chamadas.length
    store.restartStream()
    assert.equal(store.client, null)
    assert.equal(store.connectedClient, null)
    assert.equal(store.mismatch, null, 'hello pendente não é mismatch')
    await assert.rejects(store.ready(Date.now()), semCulpa(f.b))
    await assert.rejects(client.call('screenStatus'), semCulpa(f.b))
    await assert.rejects(scoped.call('connectors'), semCulpa(f.b))
    assert.throws(() => client.events(0, 'desktop', () => assert.fail(), () => {}), semCulpa(f.b))
    assert.equal(store.acaoRecusada, null)
    assert.equal(store.erroAcao, null)
    stream.data(JSON.stringify({ type: 'botDeleted', id: bot.id, rev: 99999 }))
    releaseSave(); await rejectedSave
    assert.equal(store.bots.get(bot.id)?.name, bot.name, 'save antigo não projeta resposta')
    assert.equal(JSON.stringify([...store.entries]), oldEntries, 'evento tardio não altera o espelho')
    assert.deepEqual(f.chamadas.slice(calls), ['p:hello'], 'só o hello interno passa')
    releaseHello(); await f.flush(2000)
    assert.ok(store.mismatch)
    store.saveCache()
    assert.deepEqual(f.storage, cache, 'saveTimer pendente e save direto não gravam sob mismatch conhecido')
    assert.equal(f.writes.length, writes, 'nem regrava cache antigo sob mismatch')
    const refused = await store.stop(bot.id)
    assert.equal(refused.ok, false)
    if (!refused.ok) {
      assert.ok(refused.error instanceof f.b.Desatualizado)
      assert.equal(refused.error.versao, '2.0.0')
      assert.equal(refused.error.minimo, '2.20.1')
    }
    const cachedBot = store.roster.find(b => b.id !== store.principal.protectedId)!
    const edit = await store.setComputer(cachedBot, !cachedBot.computer)
    assert.equal(edit.ok, false)
    assert.equal(store.bots.get(cachedBot.id)?.computer, cachedBot.computer)
    const deleted = await store.delete(cachedBot)
    assert.equal(deleted?.ok, false)
    assert.ok(store.bots.has(cachedBot.id), 'recusa não remove o bot em cache')
    await assert.rejects(client.call('skills'), e => e instanceof f.b.Desatualizado)
    assert.deepEqual(f.storage, cache)
    store.retire()
    // Nova store restaura exatamente o cache bom, antes e depois de descobrir mismatch.
    const reopened = f.loja('pending', 'p')
    assert.ok(reopened.bots.has(bot.id))
    assert.equal(reopened.bots.get(bot.id)?.name, bot.name)
    reopened.start(); await f.flush(); reopened.saveCache()
    assert.deepEqual(f.storage, cache)
    reopened.retire()
  } finally { f.close() }
})

test('reconexão natural invalida referências antes de iniciar outro hello', async () => {
  const f = fixture()
  try {
    const s = f.loja('natural', 'n'); s.start(); await f.flush()
    const client = s.client!
    f.streams.at(-1)!.end(null)
    assert.equal(s.client, null, 'o fim do stream invalida na hora, antes de qualquer espera')
    await assert.rejects(client.call('skills'), semCulpa(f.b))
    assert.equal(s.client, null)
    const release = f.hold('n', 'hello')
    await f.flush(1000)
    assert.equal(s.client, null)
    release(); await f.flush()
    assert.ok(s.connectedClient)
    await assert.rejects(client.call('skills'), semCulpa(f.b))
    s.retire()
  } finally { f.close() }
})

test('ComputerAccess devolve recusa tipada e notifica os dois observáveis', async () => {
  const f = fixture()
  try {
    const local = f.loja('permission', 'perm', { version: '2.0.0' })
    local.start(); await f.flush()
    const access = new f.b.ComputerAccess({ local } as never)
    const sv = local.getVersion(), av = access.getVersion()
    const result = await access.request('capture')
    assert.equal(result?.ok, false)
    if (result && !result.ok) assert.ok(result.error instanceof f.b.Desatualizado)
    await f.flush()
    assert.ok(local.getVersion() > sv)
    assert.ok(access.getVersion() > av)
    assert.ok(access.error)
    assert.ok(!f.chamadas.includes('perm:requestScreenPermission'))
    local.retire()
  } finally { f.close() }
})

test('timer: teto de quatro minutos, ativo também fora de foco, destrave sozinho e antecipado', async () => {
  const f = fixture()
  try {
    const app = new f.b.AppModel(false)
    const a = f.loja('a', 'a', { version: '2.0.0' }), b = f.loja('b', 'b')
    app.attach(a.computer, a); app.attach(b.computer, b)
    await f.flush()
    app.select({ computerId: 'a', botId: 'x' })
    const delays = () => [...f.timers.values()].filter(t => t.ms >= 30000).map(t => t.ms)
    for (const ms of [30000, 60000, 120000, 240000, 240000]) {
      assert.deepEqual(delays(), [ms])
      await f.flush(ms)
    }
    assert.deepEqual(delays(), [240000])
    app.select({ computerId: 'b', botId: 'x' })
    assert.deepEqual(delays(), [240000], 'computador fora de foco continua rechecando (destrava sozinho)')
    const count = f.chamadas.filter(c => c === 'a:hello').length
    await f.flush(240000)
    assert.equal(f.chamadas.filter(c => c === 'a:hello').length, count + 1)
    f.respostas.set('a', { version: '2.20.3', minApp: '2.20.1' })
    await f.flush(240000)
    assert.equal(a.desatualizado, null, 'computador nunca em foco destrava no hello bom')
    f.respostas.set('a', { version: '2.0.0' })
    a.restartStream(); await f.flush()
    assert.ok(a.desatualizado)
    f.respostas.set('a', { version: '2.20.3', minApp: '2.20.1' })
    a.restartStream(); await f.flush()
    assert.ok(a.connectedClient)
    assert.deepEqual(delays(), [], 'destrave antecipado cancela recheck')
    app.detach('a'); app.detach('b')
    assert.deepEqual(delays(), [])
  } finally { f.close() }
})

test('matriz fail-closed: app, Hub, mínimos e variantes do parser, preservando dev e ordem 0.x', async () => {
  const cases: [string | null | undefined, string | null, string | null | undefined, boolean][] = [
    [undefined, '2.20.3', '2.20.1', false], [null, '2.20.3', '2.20.1', false], ['', '2.20.3', '2.20.1', false],
    ['lixo', '2.20.3', '2.20.1', false], ['2.20.3', null, '2.20.1', false],
    ['2.20.3', 'lixo', '2.20.1', false], ['2.20.3', '2.20.3', undefined, false], ['2.20.3', '2.20.3', null, false],
    ['2.20.3', '2.20.3', '', false], ['2.20.3', '2.20.3', '2.x', false],
    ['2.20.3', '2.20.3', '9.9.9', false], ['2.20.3', '2.0.0', '2', false],
    ['dev', '0.1.0', '9.9.9', true], ['dev', 'lixo', '2.20.1', false],
    ['dev', '2.20.3', '', false], ['0.1.0', '2.20.3', '2.20.1', true],
    ['2.20.3', '0.1.0', '0.1.0', false], ['v2.20.3', ' v2.20.3-beta+1 ', 'v2.20.1', true],
    [' v2.20.3-beta+1 ', '2.20.3', '2.20.1-beta+1', true],
    ['2.21', '2.21', '2.20', true], ['3', '3', '2', true],
    ['V2.20.3', '2.20.3', '2.20.1', false], ['2.20.3.1', '2.20.3', '2', false],
    ['2.20.3', '2.20.3', 'V2.20.1', false], ['2.20.3', '2.x', '2', false],
    ['9999999999999999999999999', '2.20.3', '2', false],
  ]
  const f = fixture()
  try {
    for (const [app, host, minApp, allowed] of cases) {
      f.codync.appVersion = app === undefined ? undefined : '2.20.3'
      const store = f.loja('matrix', 'matrix', { version: host, minApp }, app as string)
      store.start(); await f.flush()
      assert.equal(!!store.connectedClient, allowed, JSON.stringify({ app, host, minApp }))
      if (!allowed) await assert.rejects(store.ready(), e => e instanceof f.b.Desatualizado)
      store.retire()
    }
  } finally { f.close() }
})


test('hello bom atrasado de outra geração não autoriza o transporte atual', async () => {
  const f = fixture()
  try {
    const store = f.loja('old-hello', 'old')
    store.start(); await f.flush()
    const releaseOld = f.hold('old', 'hello')
    store.restartStream(); await f.flush()
    f.respostas.set('old', { version: '2.0.0', minApp: '2.20.1' })
    const releaseNew = f.hold('old', 'hello')
    store.restartStream(); await f.flush()
    releaseOld(); await f.flush()
    assert.equal(store.client, null)
    await assert.rejects(store.ready(Date.now()), semCulpa(f.b))
    releaseNew(); await f.flush()
    assert.ok(store.desatualizado)
    assert.equal(store.client, null)
    assert.equal(f.chamadas.filter(c => c === 'old:stream').length, 1)
    store.retire()
  } finally { f.close() }
})

test('save: troca na continuação entre resposta RPC e gravação ainda recusa a projeção', async () => {
  const f = fixture()
  try {
    const store = f.loja('commit-boundary', 'boundary')
    store.start(); await f.flush()
    const client = store.client!, bot = store.roster[0]!
    const call = client.call.bind(client)
    const release = f.hold('boundary', 'hello')
    client.call = async <T>(method: string, body?: unknown, timeout?: number): Promise<T> => {
      const response = await call<T>(method, body, timeout)
      if (method === 'updateBot') store.restartStream()
      return response
    }
    await assert.rejects(store.save({ ...bot, name: 'novo nome' } as never), semCulpa(f.b))
    assert.equal(store.client, null)
    release(); store.retire()
  } finally { f.close() }
})

type Interno = { apply(event: unknown): void; scheduleSave(): void; versionGate: InstanceType<Bundle['VersionGate']> }

/** Ação pedida antes do hello (partida, reconexão): espera o link como na main — sem "Atualize o app". */
const espera = async (f: ReturnType<typeof fixture>, store: ReturnType<ReturnType<typeof fixture>['loja']>, nome: string, soltar: () => void) => {
  let feita = false
  const acao = store.stop('qualquer-bot').then(r => { feita = true; return r })
  for (let i = 0; i < 3; i++) await f.flush(100)
  assert.equal(feita, false, 'a ação espera o hello em vez de ser recusada')
  assert.equal(store.mismatch, null)
  assert.equal(store.acaoRecusada, null)
  assert.equal(store.erroAcao, null)
  soltar(); await f.flush(100)
  const r = await acao
  assert.ok(f.chamadas.includes(`${nome}:stop`), 'a ação saiu depois da prova, no replay de 20 s')
  assert.equal(store.acaoRecusada, null)
  assert.equal(store.erroAcao, null)
  return r
}

test('P1-1: Hub compatível com hello pendente na partida — a ação espera e sai, sem motivo falso', async () => {
  const f = fixture()
  try {
    const soltar = f.hold('w', 'hello')
    const s = f.loja('espera', 'w')
    s.start(); await f.flush()
    await espera(f, s, 'w', soltar)
    s.retire()
  } finally { f.close() }
})

test('P1-1: reconexão após o fim do stream — a ação espera o novo hello e sai', async () => {
  const f = fixture()
  try {
    const s = f.loja('reconecta', 'r')
    s.start(); await f.flush()
    const soltar = f.hold('r', 'hello')
    f.streams.at(-1)!.end(null)
    await f.flush(1000)
    await espera(f, s, 'r', soltar)
    s.retire()
  } finally { f.close() }
})

test('P1-1: restartStream (resync/rewound) com hello pendente — a ação espera e sai', async () => {
  const f = fixture()
  try {
    const s = f.loja('restart', 'rs')
    s.start(); await f.flush()
    const soltar = f.hold('rs', 'hello')
    s.restartStream(); await f.flush()
    await espera(f, s, 'rs', soltar)
    s.retire()
  } finally { f.close() }
})

/** Mismatch conhecido (hello 2.0.0) e depois uma retomada com o hello da nova geração ainda pendente. */
async function mismatchEDepoisRetomada(f: ReturnType<typeof fixture>, nome: string, proximo: Resposta = { version: '2.20.3' }) {
  const store = f.loja(nome, nome, { version: '2.0.0', minApp: '2.0.0' })
  store.start(); await f.flush()
  assert.ok(store.mismatch, 'o primeiro hello provou o mismatch')
  const antigo = (store as unknown as { link: NonNullable<typeof store.client> }).link
  f.respostas.set(nome, proximo)
  const soltar = f.hold(nome, 'hello')
  store.restartStream(); await f.flush()
  // A geração nova ainda não tem prova: o mismatch retido é só informação para a UI e o cache.
  assert.ok(store.mismatch, 'a UI ainda mostra a recusa anterior')
  assert.equal(store.client, null)
  return { store, antigo, soltar }
}

test('P2: mismatch → restart → hello compatível pendente — a ação espera o link e sai', async () => {
  const f = fixture()
  try {
    const { store, antigo, soltar } = await mismatchEDepoisRetomada(f, 'm1')
    const cache = new Map(f.storage)
    // O transporte guardado da geração pendente falha como link caído (status 0), sem culpar a versão.
    await assert.rejects(antigo.call('skills'), semCulpa(f.b))
    let feita = false
    const acao = store.stop('qualquer-bot').then(r => { feita = true; return r })
    for (let i = 0; i < 3; i++) await f.flush(100)
    assert.equal(feita, false, 'a ação espera o hello em vez de ser recusada pela prova antiga')
    assert.equal(store.erroAcao, null)
    assert.equal(store.acaoRecusada, null)
    assert.ok(!f.chamadas.includes('m1:stop'))
    store.saveCache()
    assert.deepEqual(f.storage, cache, 'o mismatch retido continua protegendo o cache')
    soltar(); await f.flush(100)
    const r = await acao
    assert.ok(f.chamadas.includes('m1:stop'), 'a ação saiu depois da prova compatível')
    assert.equal(r.ok, true)
    assert.equal(store.mismatch, null)
    assert.equal(store.erroAcao, null)
    store.retire()
  } finally { f.close() }
})

test('P2: mismatch → restart → novo hello continua incompatível — recusa tipada Desatualizado', async () => {
  const f = fixture()
  try {
    const { store, soltar } = await mismatchEDepoisRetomada(f, 'm2', { version: '2.0.1', minApp: '2.0.1' })
    let feita = false
    const acao = store.stop('qualquer-bot').then(r => { feita = true; return r })
    for (let i = 0; i < 3; i++) await f.flush(100)
    assert.equal(feita, false, 'sem prova da geração nova a ação ainda espera')
    soltar(); await f.flush(100)
    const r = await acao
    assert.equal(r.ok, false)
    if (!r.ok) {
      assert.ok(r.error instanceof f.b.Desatualizado)
      assert.equal(r.error.versao, '2.0.1')
    }
    assert.ok(!f.chamadas.includes('m2:stop'))
    store.retire()
  } finally { f.close() }
})

test('P3: versão do host ilegível não vira texto em `encontrada`', async () => {
  const f = fixture()
  try {
    const lixo = f.loja('lixo', 'lx', { version: 'lixo', minApp: '2.20.1' })
    lixo.start(); await f.flush()
    assert.equal(lixo.mismatch?.kind, 'updateHost')
    assert.deepEqual({ ...lixo.desatualizado }, { alvo: 'hub', encontrada: null, minimo: '2.20.1' })
    lixo.retire()
  } finally { f.close() }
})

test('hello sem minApp: Hub provavelmente antigo (updateHost); host sem versão: encontrada null', async () => {
  const f = fixture()
  try {
    const antigo = f.loja('sem-min', 'sm', { version: '2.20.3', minApp: undefined })
    const mudo = f.loja('sem-versao', 'sv', { version: null })
    antigo.start(); mudo.start(); await f.flush()
    assert.equal(antigo.mismatch?.kind, 'updateHost')
    assert.deepEqual({ ...antigo.desatualizado }, { alvo: 'hub', encontrada: '2.20.3', minimo: '2.20.1' })
    assert.equal(mudo.mismatch?.kind, 'updateHost')
    assert.deepEqual({ ...mudo.desatualizado }, { alvo: 'hub', encontrada: null, minimo: '2.20.1' })
    antigo.retire(); mudo.retire()
  } finally { f.close() }
})

test('guarda: resposta que chega depois da troca de geração é descartada no transporte', async () => {
  const f = fixture()
  try {
    const s = f.loja('g-call', 'gc'); s.start(); await f.flush()
    const client = s.client!
    const soltarSkills = f.hold('gc', 'skills')
    const pendente = client.call('skills')
    await settle()
    const soltarHello = f.hold('gc', 'hello')
    s.restartStream()
    soltarSkills()
    await assert.rejects(pendente, semCulpa(f.b))
    soltarHello(); s.retire()
  } finally { f.close() }
})

test('guarda: apply() não aplica evento sob mismatch nem sem prova', async () => {
  const f = fixture()
  try {
    const s = f.loja('g-apply', 'ga'); s.start(); await f.flush()
    const bot = s.roster[0]!
    f.respostas.set('ga', { version: '2.0.0', minApp: '2.0.0' })
    s.restartStream(); await f.flush()
    assert.ok(s.mismatch)
    ;(s as unknown as Interno).apply({ type: 'botDeleted', id: bot.id, rev: 99999 })
    assert.ok(s.bots.has(bot.id), 'evento direto no apply não altera o espelho sob mismatch')
    s.retire()
  } finally { f.close() }
})

test('guarda: entrega tardia de stream guardado não passa sob mismatch da mesma geração', async () => {
  const f = fixture()
  try {
    const s = f.loja('g-data', 'gd'); s.start(); await f.flush()
    const client = s.client!
    const recebidas: string[] = []
    client.transport.stream('/events?since=0', line => recebidas.push(line), () => {})
    const raw = f.streams.at(-1)!
    raw.data('{"type":"ok"}')
    assert.equal(recebidas.length, 1, 'com prova, a linha passa')
    const gate = (s as unknown as Interno).versionGate
    gate.prove(gate.currentGeneration, { version: '2.0.0', minApp: '2.0.0' })
    raw.data('{"type":"tarde"}')
    assert.equal(recebidas.length, 1, 'sob mismatch da geração, a linha não passa')
    s.retire()
  } finally { f.close() }
})

test('guarda: prova de geração velha não autoriza nem marca mismatch', () => {
  const f = fixture()
  try {
    const gate = new f.b.VersionGate('2.20.3', () => {})
    const velha = gate.currentGeneration
    gate.invalidate()
    assert.equal(gate.prove(velha, { version: '2.20.3', minApp: '2.20.1' }), false)
    assert.equal(gate.provenGeneration, -1)
    assert.equal(gate.allowed(), false)
    assert.equal(gate.prove(velha, { version: '2.0.0', minApp: '2.0.0' }), false)
    assert.equal(gate.mismatch, null, 'hello velho nem culpa a versão')
    assert.equal(gate.prove(gate.currentGeneration, { version: '2.20.3', minApp: '2.20.1' }), true)
    assert.equal(gate.allowed(), true)
  } finally { f.close() }
})

test('guarda: scheduleSave não agenda timer sob mismatch conhecido', async () => {
  const f = fixture()
  try {
    const s = f.loja('g-save', 'gs'); s.start(); await f.flush(); await f.flush(2000)
    const timers = () => [...f.timers.values()].filter(t => t.ms === 2000).length
    ;(s as unknown as Interno).scheduleSave()
    assert.equal(timers(), 1, 'sem mismatch, agenda')
    f.respostas.set('gs', { version: '2.0.0', minApp: '2.0.0' })
    s.restartStream(); await f.flush(2000)
    assert.ok(s.mismatch)
    assert.equal(timers(), 0)
    ;(s as unknown as Interno).scheduleSave()
    assert.equal(timers(), 0, 'sob mismatch, não agenda')
    s.retire()
  } finally { f.close() }
})

test('ErroFrase mora em módulo sem catálogos: o grafo do store não puxa os textos (memória dos testes de voz)', async () => {
  const { readFileSync } = await import('node:fs')
  const fonte = (nome: string) => readFileSync(new URL(nome, import.meta.url), 'utf8')
  for (const arquivo of ['./bot-store.ts', './bot-sync.ts']) {
    assert.doesNotMatch(fonte(arquivo), /^import (?!type).*fork-i18n\/frase/m, `${arquivo} não importa frase.ts (puxa ~4,5 MB de catálogos)`)
  }
  assert.doesNotMatch(fonte('../lib/fork-i18n/erro-frase.ts'), /^import (?!type)/m, 'erro-frase.ts não importa nada em tempo de execução')
  const modulo = await import('../lib/fork-i18n/frase.ts')
  const { ErroFrase } = await import('../lib/fork-i18n/erro-frase.ts')
  const erro = new ErroFrase(modulo.frase('spaces.recusa.atualize'))
  assert.ok(erro instanceof modulo.ErroFrase, 'a mesma classe pelos dois caminhos')
  assert.ok(erro.message.length > 0, 'mensagem crua em inglês quando o catálogo está carregado')
})
