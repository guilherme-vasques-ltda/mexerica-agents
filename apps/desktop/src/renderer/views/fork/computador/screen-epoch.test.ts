import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Computer } from '../../../../shared/models'
import type { HostTransport } from '../../../client/host-client'

// R3 (runtime-audiencia): uma resposta de tela que saiu antes de uma queda não repõe quem usa o computador depois do
// novo hello, mesmo com o MESMO HostClient reconectando. BotStore, BotSync, HostClient e ComputerAccess são reais;
// só o transporte, a ponte do Electron e o relógio são fictícios (como em fork/donna/empty-roster.test.ts).

const bundle = buildSync({
  stdin: { contents: `export { BotStore } from './store/bot-store'; export { ComputerAccess } from './store/computer-access'; export { HostClient } from './client/host-client'; export { SpaceContext } from './store/fork-space-context'`, resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
type Bundle = {
  BotStore: typeof import('../../../store/bot-store').BotStore; ComputerAccess: typeof import('../../../store/computer-access').ComputerAccess
  HostClient: typeof import('../../../client/host-client').HostClient; SpaceContext: typeof import('../../../store/fork-space-context').SpaceContext
}

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
const screen = (agentUse: string, agentBot: string | null) => ({ enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0, agentUse, agentBot })
const deferred = () => { let resolve!: (v: unknown) => void; const promise = new Promise((r) => { resolve = r }); return { promise, resolve } }

function fixture(options: { holdHello?: boolean; times?: boolean } = {}) {
  const time = clock()
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date: time.Date, setTimeout: time.setTimeout, clearTimeout: time.clearTimeout,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const { BotStore, ComputerAccess, HostClient, SpaceContext } = module.exports as Bundle
  let audiencia = 'using' // o que o Hub projeta para o Pessoal agora
  const pending: Record<string, ReturnType<typeof deferred>[]> = {}
  let emit!: Parameters<HostTransport['stream']>[1]
  let end!: Parameters<HostTransport['stream']>[2]
  let streams = 0
  let rpcHellos = 0
  let holdHello = options.holdHello ?? false
  let heldHello: (() => void) | null = null
  const spaces = options.times ? { version: 1, revision: 1, personalSpaceId: 'personal', spaces: [] } : undefined
  const transport: HostTransport = {
    async call(method) {
      if (method === 'hello') { rpcHellos++; return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [], spaces } }
      if (method === 'teamSettings') return { primaryBotId: 'donna' }
      if ((options.times && method === 'donnaTimes') || ['screenTakeover', 'setScreenEnabled', 'screenStatus'].includes(method)) { const d = deferred(); (pending[method] ??= []).push(d); return d.promise }
      return {}
    },
    stream(_path, onData, onEnd) {
      streams++
      emit = onData; end = onEnd
      // O hello é projetado quando sai do Hub; segurado, sai com a audiência do momento em que for liberado.
      const send = () => onData(JSON.stringify({ type: 'hello', hostId: 'hub', rev: 9, spaces, usage: { providers: [] }, screen: audiencia === 'using' ? screen('using', 'donna') : screen('unavailable', null) }))
      if (holdHello) heldHello = send
      else queueMicrotask(send)
      return () => {}
    },
  }
  const store = new BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => transport)
  store.start()
  return {
    store, time, pending, ComputerAccess, HostClient, SpaceContext,
    streams: () => streams,
    revision: () => (store as unknown as { rev: number }).rev,
    client: () => (store as unknown as { client: unknown }).client,
    agent: () => [store.screen?.agentUse ?? null, store.screen?.agentBot ?? null],
    move(to: string) { audiencia = to },
    /** Segura o hello do PRÓXIMO stream (o atual não muda). */
    holdNextHello() { holdHello = true },
    releaseHello() { holdHello = false; const send = heldHello; heldHello = null; send?.() },
    helloHeld: () => heldHello !== null,
    rpcHellos: () => rpcHellos,
    emit: (event: object) => emit(JSON.stringify(event)),
    /** O emissor do stream aberto agora; guardado, vira o de um stream que já terminou. */
    emitter: () => { const e = emit; return (event: object) => e(JSON.stringify(event)) },
    drop() { end(Object.assign(new Error('stream'), { status: 0 }) as never) },
    close: () => store.retire(),
  }
}

for (const method of ['screenTakeover', 'setScreenEnabled'] as const) {
  test(`R3: ${method} pendurado antes da queda não repõe a Donna depois do hello da geração nova`, async () => {
    const f = fixture()
    try {
      await settle()
      assert.deepEqual(f.agent(), ['using', 'donna'], 'online: o Hub projeta a Donna no Pessoal')
      const call = method === 'screenTakeover' ? f.store.screenTakeover(true) : f.store.setScreenEnabled(true).then(() => ({ ok: true }), error => ({ ok: false, error }))
      await settle()
      assert.equal(f.pending[method]?.length, 1, 'a RPC saiu e ficou pendurada')
      const client = f.client()
      f.drop(); await settle()
      assert.deepEqual(f.agent(), ['unavailable', null], 'a queda tira quem usa')
      f.move('unavailable') // a audiência muda enquanto o link está fora (a Donna passa a trabalhar na Loja)
      await f.time.advance(6000)
      assert.ok(f.streams() >= 2, 'reconectou')
      assert.notEqual(f.client(), client, 'cliente de outra geração')
      assert.deepEqual(f.agent(), ['unavailable', null], 'o novo hello diz indisponível')
      f.pending[method]![0]!.resolve(screen('using', 'donna')) // a resposta velha chega agora
      const refused = await call; await settle()
      assert.equal(refused.ok, false, 'resposta da geração antiga é recusada')
      assert.deepEqual(f.agent(), ['unavailable', null], 'a resposta velha não repõe a identidade')
      assert.equal(f.store.screen?.computerUse, true, 'o estado do hello atual permanece')
      // Uma RPC da época atual funciona: sai e volta sem queda, hello ou evento no meio.
      f.move('using')
      const again = method === 'screenTakeover' ? (f.store.screenTakeover(true), null) : f.store.setScreenEnabled(true)
      await settle()
      f.pending[method]![1]!.resolve(screen('using', 'donna'))
      await again; await settle()
      assert.deepEqual(f.agent(), ['using', 'donna'], 'resposta atual nomeia')
    } finally { f.close() }
  })
}

test('R3: evento screen ou hello no meio torna a resposta velha; ComputerAccess também descarta quem usa', async () => {
  const f = fixture()
  try {
    await settle()
    f.store.screenTakeover(true); await settle()
    f.emit({ type: 'screen', screen: screen('unavailable', null) }); await settle()
    f.pending.screenTakeover![0]!.resolve(screen('using', 'donna')); await settle()
    assert.deepEqual(f.agent(), ['unavailable', null], 'o evento atual vence a resposta que saiu antes dele')
    const access = new f.ComputerAccess({ local: f.store } as never)
    const refreshing = access.refresh(); await settle()
    f.drop(); await settle()
    await f.time.advance(6000)
    f.pending.screenStatus![0]!.resolve(screen('using', 'donna'))
    await refreshing; await settle()
    assert.equal(access.screen, null, 'nenhuma projeção do RPC da geração antiga é aceita')
    assert.ok(access.error, 'recusa do transporte fica observável')
  } finally { f.close() }
})

test('R3: evento atrasado do stream que já terminou não repõe quem usa (mesmo HostClient, mesmo loop)', async () => {
  const f = fixture()
  try {
    await settle()
    const old = f.emitter()
    f.drop(); await settle()
    f.move('unavailable')
    await f.time.advance(6000)
    assert.deepEqual(f.agent(), ['unavailable', null], 'novo hello indisponível')
    old({ type: 'screen', screen: screen('using', 'donna') }); await settle()
    assert.deepEqual(f.agent(), ['unavailable', null], 'o stream velho já terminou: o evento dele é descartado')
    f.emit({ type: 'screen', screen: screen('using', 'donna') }); await settle()
    assert.deepEqual(f.agent(), ['using', 'donna'], 'o stream atual continua valendo')
  } finally { f.close() }
})

// R4: reconectar à mão (⌘R / Reconectar) ou um resync com a conexão ainda online também encerram o ciclo do stream.
for (const how of ['restartStream', 'resync'] as const) {
  test(`R4: ${how} com a conexão online tira quem usa antes do novo hello; a RPC velha não volta`, async () => {
    const f = fixture()
    try {
      await settle()
      assert.deepEqual(f.agent(), ['using', 'donna'])
      f.store.screenTakeover(true); await settle()
      assert.equal(f.pending.screenTakeover?.length, 1, 'RPC da época antiga pendurada')
      f.move('unavailable') // a audiência muda: a Donna vai trabalhar na Loja
      const streams = f.streams()
      if (how === 'restartStream') f.store.restartStream()
      else f.emit({ type: 'resync' })
      assert.equal(f.store.connection.kind, 'online', 'a conexão nem chegou a sair de online')
      assert.deepEqual(f.agent(), ['unavailable', null], 'já sem identidade, antes de qualquer resposta do stream novo')
      await settle()
      assert.ok(f.streams() > streams, 'stream novo')
      assert.deepEqual(f.agent(), ['unavailable', null], 'novo hello indisponível')
      f.pending.screenTakeover![0]!.resolve(screen('using', 'donna')); await settle()
      assert.deepEqual(f.agent(), ['unavailable', null], 'a resposta da época antiga não repõe a Donna')
      f.move('using')
      f.store.screenTakeover(true); await settle()
      f.pending.screenTakeover![1]!.resolve(screen('using', 'donna')); await settle()
      assert.deepEqual(f.agent(), ['using', 'donna'], 'RPC da época atual nomeia')
    } finally { f.close() }
  })
}

// R4: o fim de um stream é definitivo no HostClient real (para o Pessoal, os spaces e Times) e no SpaceContext real.
test('R4: SpaceContext + HostClient reais: depois do onEnd, um screen tardio do mesmo stream não reativa a Loja; retry vale', async () => {
  const f = fixture()
  try {
    const lines: { onData: (l: string) => void; onEnd: (e: null) => void }[] = []
    const transport = {
      call: async (method: string, body: Record<string, unknown>) => method === 'donnaConversations' ? { hubId: 'hub', spaceId: body.spaceId, conversations: [], principal: { id: 'donna', name: 'Donna' } } : {},
      stream: (_path: string, onData: (l: string) => void, onEnd: (e: null) => void) => { lines.push({ onData, onEnd }); return () => {} },
    }
    const client = new f.HostClient(transport as never)
    const source = { hello: { hostId: 'hub', rev: 1, spacesContract: 1 }, connectedClient: client, principalEpoch: 1, spaces: null, subscribe: () => () => {} }
    const ctx = new f.SpaceContext()
    ctx.open(source as never, 'loja'); await settle()
    const using = JSON.stringify({ type: 'screen', spaceId: 'loja', screen: screen('using', 'donna') })
    lines[0]!.onData(using)
    assert.equal(ctx.screen?.agentBot, 'donna')
    lines[0]!.onEnd(null)
    assert.deepEqual([ctx.link, ctx.screen], ['paused', null])
    lines[0]!.onData(using) // a linha tardia do stream que já terminou
    assert.deepEqual([ctx.link, ctx.screen], ['paused', null], 'continua pausado e sem identidade')
    ctx.retry(); await settle()
    lines[1]!.onData(using)
    assert.deepEqual([ctx.link, ctx.screen?.agentBot], ['live', 'donna'], 'o stream novo do retry vale')
  } finally { f.close() }
})

// R5: a RPC iniciada pelo cliente NOVO, depois do reinício e antes do hello do stream novo, não pode dar identidade:
// a época nova ainda não foi observada pelo próprio stream. Vale para conexão inicial, ⌘R, resync e linha ilegível.
for (const how of ['inicial', 'restartStream', 'resync', 'undecodable'] as const) {
  for (const method of ['setScreenEnabled', 'screenTakeover'] as const) {
    test(`R5: ${how} — ${method} da época nova, antes do hello novo, não nomeia; hello atual e RPC observada nomeiam`, async () => {
      const f = fixture({ holdHello: how === 'inicial' })
      try {
        await settle()
        if (how !== 'inicial') {
          assert.deepEqual(f.agent(), ['using', 'donna'], 'online e observado')
          f.holdNextHello()
          const before = f.streams()
          const hellos = f.rpcHellos()
          if (how === 'restartStream') f.store.restartStream()
          else if (how === 'resync') f.emit({ type: 'resync' })
          else f.emit({ type: 'bot' }) // sem bot: não decodificável, primeira vez → rewind + restartEvents
          await settle()
          assert.ok(f.streams() > before, 'stream novo aberto')
          assert.ok(f.rpcHellos() > hellos, 'a RPC hello do ciclo novo já resolveu (runEvents a aguarda antes de /events)')
        }
        assert.ok(f.helloHeld(), 'o hello do SSE novo ainda pendente: só ele observaria a época')
        if (how !== 'inicial') assert.equal(f.store.connection.kind, 'online', 'a conexão segue online (ready/live dão o cliente novo)')
        const asked = f.client()
        const call = method === 'setScreenEnabled' ? f.store.setScreenEnabled(true) : (f.store.screenTakeover(true), null)
        await settle()
        if (how === 'inicial') {
          // Sem nenhuma observação, ready() nem entrega cliente: a ação espera o stream. Liberar o hello abre a época.
          assert.equal(f.pending[method]?.length ?? 0, 0, 'antes do primeiro hello a RPC nem sai')
          f.move('unavailable'); f.releaseHello(); await f.time.advance(2000)
          assert.deepEqual(f.agent(), ['unavailable', null], 'primeiro hello: indisponível')
          assert.equal(f.pending[method]?.length, 1, 'com o stream observado, a ação sai')
          f.move('using')
          f.pending[method]![0]!.resolve(screen('using', 'donna')); await call; await settle()
          assert.deepEqual(f.agent(), ['using', 'donna'], 'saiu numa época observada: vale (o Hub respondeu depois do hello)')
          return
        }
        assert.equal(f.pending[method]?.length, 1, 'a RPC saiu pelo cliente novo, antes do hello')
        assert.equal(f.client(), asked)
        // O Hub respondeu using + Donna naquele instante (legítimo); o transporte segura; a Donna vai para a Loja.
        f.move('unavailable')
        f.pending[method]![0]!.resolve(screen('using', 'donna')); await call; await settle()
        assert.deepEqual(f.agent(), ['unavailable', null], 'antes do hello novo: Pessoal sem identidade')
        assert.equal(f.store.screen?.computerUse, true, 'o resto da resposta vale (computerUse, permissões)')
        f.releaseHello(); await settle()
        assert.deepEqual(f.agent(), ['unavailable', null], 'hello atual: indisponível')
        f.move('using')
        const again = method === 'setScreenEnabled' ? f.store.setScreenEnabled(true) : (f.store.screenTakeover(true), null)
        await settle()
        f.pending[method]![1]!.resolve(screen('using', 'donna')); await again; await settle()
        assert.deepEqual(f.agent(), ['using', 'donna'], 'RPC de uma época observada pelo stream nomeia')
      } finally { f.close() }
    })
  }
}

// Composição #241/Privacidade: o BotSync real compartilha o transporte, mas conserva as duas invalidações.
for (const how of ['drop', 'restartStream', 'resync'] as const) {
  test(`Times + Screen: ${how} descarta respostas antigas, conserva só a organização em RAM e relê após hello`, async () => {
    const f = fixture({ times: true })
    const snapshot = (catalogVersion: string) => ({ version: 1, spaceId: 'personal', revision: 7, catalogVersion,
      primaryBotId: 'donna', nodes: [], members: [], routes: [], bots: [] })
    try {
      await settle(); await f.time.advance(250)
      assert.equal(f.streams(), 1, 'Times lê pelo cliente existente, sem stream próprio')
      assert.equal(f.pending.donnaTimes?.length, 1)
      f.pending.donnaTimes![0]!.resolve(snapshot('salvo')); await settle()
      const saved = f.store.timesRoster.snapshot
      assert.equal(saved?.catalogVersion, 'salvo')
      assert.equal(f.store.timesRoster.phase, 'ready')
      const domain = f.store.timesRoster.domain
      const beforeTimes = f.revision()
      f.emit({ type: 'times', spaceId: 'personal', revision: 100 })
      await f.time.advance(250)
      assert.equal(f.revision(), beforeTimes, 'evento Times atual não avança rev do espelho')
      assert.equal(f.pending.donnaTimes?.length, 2)
      f.store.screenTakeover(true); await settle()
      const old = f.emitter()
      f.holdNextHello(); f.move('unavailable')
      if (how === 'drop') f.drop()
      else if (how === 'restartStream') f.store.restartStream()
      else f.emit({ type: 'resync' })
      await settle()
      assert.deepEqual(f.agent(), ['unavailable', null], 'a época de Screen termina imediatamente')
      assert.equal(f.store.timesRoster.snapshot, saved, 'organização administrativa permanece só em RAM')
      if (how !== 'resync') assert.equal(f.store.timesRoster.phase, 'offline', 'stop/queda suspende Times')
      f.pending.donnaTimes![1]!.resolve(snapshot('atrasado'))
      f.pending.screenTakeover![0]!.resolve(screen('using', 'donna')); await settle()
      assert.equal(f.store.timesRoster.snapshot, saved, 'resposta antiga de Times descartada')
      assert.deepEqual(f.agent(), ['unavailable', null], 'RPC antiga de Screen descartada')
      if (how === 'drop') await f.time.advance(6000)
      assert.ok(f.helloHeld())
      f.releaseHello(); await settle(); await f.time.advance(250)
      assert.equal(f.streams(), 2, 'só o stream de reconexão, sem stream paralelo de Times')
      assert.equal(f.pending.donnaTimes?.length, 3)
      f.pending.donnaTimes![2]!.resolve(snapshot('atual')); await settle()
      assert.equal(f.store.timesRoster.snapshot?.catalogVersion, 'atual')
      assert.equal(f.store.timesRoster.phase, 'ready')
      assert.equal(f.store.timesRoster.domain, domain, 'reconexão não troca domínio das preferências')
      assert.deepEqual(f.agent(), ['unavailable', null])
      const rev = f.revision()
      old({ type: 'screen', screen: screen('using', 'donna') })
      old({ type: 'times', spaceId: 'personal', revision: 100 })
      await f.time.advance(250)
      assert.deepEqual(f.agent(), ['unavailable', null], 'stream encerrado não repõe identidade')
      assert.equal(f.pending.donnaTimes?.length, 3, 'nem invalida o catálogo por evento tardio')
      assert.equal(f.revision(), rev, 'Times não avança o rev do espelho')
      assert.equal(f.store.screen?.computerUse, true)
    } finally { f.close() }
    assert.equal(f.store.timesRoster.snapshot, null, 'retire limpa Times')
    assert.deepEqual(f.agent(), ['unavailable', null], 'retire encerra também a época de Screen')
  })
}
