import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate as tick } from 'node:timers/promises'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'
import type { Bot, Computer, Entry } from '../../shared/models'
import type { HostTransport } from '../client/host-client'
import { engineOf, parseSessionActions } from './fork-sessions.ts'
import { rotuloModelo } from '../views/fork/sessoes/rotulo.ts'
import { ChoiceAnswers } from './fork-genui-choice.ts'

// UI-01 (D70): criar e enviar numa extra só com o anúncio do Hub, no link e na época certos, e nunca cair na
// principal. BotStore, HostClient e o modelo de sessões são reais; só o transporte e a ponte do Electron são fictícios.

const bundle = buildSync({
  stdin: {
    contents: `export { BotStore } from './store/bot-store'; export { normalizeBot } from '../shared/models'; export { extrasAllowed } from './views/fork/sessoes/ganchos'; export { HostError } from './client/host-client'`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, loader: { '.css': 'empty' }, define: { 'process.env.NODE_ENV': '"production"' },
})
type Bundle = {
  BotStore: typeof import('./bot-store').BotStore
  normalizeBot: typeof import('../../shared/models').normalizeBot
  extrasAllowed: typeof import('../views/fork/sessoes/ganchos').extrasAllowed
  HostError: typeof import('../client/host-client').HostError
}
const settle = async () => { for (let i = 0; i < 12; i++) await tick() }
const bridge: unknown = new Proxy(function () {}, { get: (_t, key) => key === 'then' ? undefined : bridge, apply: () => bridge })

const row = (id: string, principal = false, state: 'open' | 'closed' = 'open') => ({
  id, chatId: 'byte', principal, origin: { kind: 'owner' }, sourceSession: null as string | null, parentSession: null, depth: 0, title: '', titleSource: principal ? 'principal' : 'first_message',
  state, closedReason: null as string | null, closedAt: null as number | null, createdAt: 1, lastUsedAt: 1, revision: 1,
})

/** Um Hub fictício: `actions` é o que `sessions` anuncia (undefined = Hub antigo, sem o campo). */
function hub(options: { actions?: unknown; sendError?: string; ignoreSessionId?: boolean; hold?: (method: string) => Promise<void> | void; failNext?: { method: string; error: unknown } | null; holdHello?: boolean; deleteReply?: Record<string, unknown>; historyEntries?: Entry[] } = {}) {
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const sessions = [row('byte', true), row('s-a'), row('s-b'), row('s-old', false, 'closed')]
  let emit: (event: unknown) => void = () => {}
  let seq = 0
  const transport: HostTransport = {
    async call(method, raw) {
      const body = raw as Record<string, unknown>
      calls.push({ method, body })
      // A resposta do catálogo é a do instante do pedido, mesmo se chegar depois (segurada por `hold`).
      const catalog = { sessions: [...sessions], ...(options.actions === undefined ? {} : { sessionActions: options.actions }) }
      await options.hold?.(method)
      if (options.failNext?.method === method) {
        const { error } = options.failNext
        options.failNext = null
        throw error
      }
      switch (method) {
        case 'hello': return { version: '2.20.3', minApp: '2.20.1', name: 'Fixture', backends: [] }
        case 'sessions': return catalog
        case 'sessionCreate': {
          const created = row(`s-new-${++seq}`)
          sessions.push(created)
          return { session: created }
        }
        // DON-907 C5: o contrato de encerrar.rs; a linha é trocada (não mutada) para não mudar respostas já dadas.
        case 'sessionClose': {
          const at = sessions.findIndex((r) => r.id === body.sessionId)
          sessions[at] = { ...sessions[at]!, state: 'closed', closedReason: 'owner', closedAt: 2, revision: sessions[at]!.revision + 1 }
          return { session: sessions[at], cancelled: { entryIds: [] }, stopRequested: false }
        }
        case 'history': return { entries: options.historyEntries ?? [] }
        case 'sessionDelete': {
          if (options.deleteReply) return options.deleteReply
          const at = sessions.findIndex((r) => r.id === body.sessionId && r.chatId === body.botId)
          if (at < 0) throw new Error('unknown_session')
          sessions.splice(at, 1)
          return { type: 'sessionDeleted', spaceId: body.spaceId, chatId: body.botId, sessionId: body.sessionId, rev: 90 }
        }
        case 'sessionContinue': {
          const created = { ...row(`s-cont-${++seq}`), origin: { kind: 'continuation' }, sourceSession: body.sessionId as string }
          sessions.push(created)
          return { session: created, source: sessions.find((r) => r.id === body.sessionId), handoff: { id: `h-${seq}` } }
        }
        case 'send': {
          if (options.sendError) throw new Error(options.sendError)
          // Um Hub antigo ignora `sessionId` e grava na principal; o novo grava na extra pedida.
          const sessionId = options.actions === undefined || options.ignoreSessionId ? 'byte' : (body.sessionId as string | undefined) ?? 'byte'
          return { entry: { id: `e-${++seq}`, seq, botId: 'byte', rev: 10 + seq, kind: 'user', turn: 1, createdAt: 1, updatedAt: 1, sessionId, data: { text: body.text, status: 'sent', clientNonce: body.clientNonce } } }
        }
        case 'sessionChoiceAnswer': return { session: sessions.find((s) => s.id === body.sessionId), question: { questionId: body.questionId }, entry: { botId: 'byte', sessionId: body.sessionId }, replayed: false }
        default: return {}
      }
    },
    stream(_path, onData) {
      emit = (event) => onData(JSON.stringify(event))
      // `holdHello`: o stream novo abre, mas o hello dele demora (como depois de Reconectar, antes de chegar).
      if (options.holdHello) pendingHello = () => emit({ type: 'hello', hostId: 'hub', rev: 1, usage: { providers: [] } })
      else queueMicrotask(() => emit({ type: 'hello', hostId: 'hub', rev: 1, usage: { providers: [] } }))
      return () => {}
    },
  }
  let pendingHello: (() => void) | null = null
  return { transport, calls, sessions, options, releaseHello: () => pendingHello?.(), hello: (hostId = 'hub') => emit({ type: 'hello', hostId, rev: 1, usage: { providers: [] } }) }
}

async function open(fake: ReturnType<typeof hub>) {
  const module = { exports: {} }
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, queueMicrotask, URLSearchParams, Date, setTimeout, clearTimeout, crypto, btoa, Uint8Array,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    // DON-903: o módulo de idioma (via ganchos) sincroniza `lang`/`dir` do documento ao carregar.
    document: { documentElement: {}, body: { setAttribute() {} } },
    window: { addEventListener() {}, codync: Object.assign(Object.create(bridge as object), { appVersion: '2.20.3', platform: 'darwin' }), location: { search: '' } },
  })
  const exports = module.exports as Bundle
  const store = new exports.BotStore({ id: 'mac', name: 'Mac', signKey: '', urls: [] } as Computer, true, () => fake.transport)
  store.start()
  await settle()
  store.bots.set('byte', exports.normalizeBot({ id: 'byte', name: 'Byte', backend: 'codex' }))
  await store.sessions.load('byte')
  return { store, ...exports }
}

const sends = (fake: ReturnType<typeof hub>) => fake.calls.filter((c) => c.method === 'send').map((c) => c.body)
const local = (store: { allEntries: (id: string) => Entry[] }) => store.allEntries('byte').filter((e) => e.id.startsWith('local-'))

test('UI-01: o anúncio só libera ação conhecida e explícita', () => {
  for (const raw of [undefined, null, 'create,send', { create: true }, [1, 2], ['close', 7], ['create', 7]]) assert.equal(parseSessionActions(raw).size, 0, String(raw))
  assert.deepEqual([...parseSessionActions(['create'])], ['create'], 'criar não traz enviar junto')
  assert.deepEqual([...parseSessionActions(['archive', 'send', 'create'])].sort(), ['create', 'send'], 'nome futuro não atrapalha nem libera outro')
  // DON-907 C5: encerrar e continuar são ações próprias; uma não libera a outra nem criar/enviar.
  assert.deepEqual([...parseSessionActions(['close'])], ['close'])
  assert.deepEqual([...parseSessionActions(['continue', 'create'])].sort(), ['continue', 'create'])
})

test('UI-01: Hub novo cria sem enviar, abre a devolvida, e envia na extra exata; a principal envia como antes', async () => {
  const fake = hub({ actions: ['create', 'send'] })
  const { store } = await open(fake)
  try {
    assert.ok(store.sessions.can('byte', 'create') && store.sessions.can('byte', 'send'))
    // Dois cliques seguidos: um pedido só.
    const [first, second] = [store.sessions.create('byte'), store.sessions.create('byte')]
    assert.equal(first, second)
    const created = await first
    assert.ok(created.ok)
    assert.equal(fake.calls.filter((c) => c.method === 'sessionCreate').length, 1)
    assert.equal(sends(fake).length, 0, 'criar não manda nada')
    assert.ok(store.sessions.of('byte')?.some((s) => s.id === created.session.id))
    // Duas extras e a principal, cada uma com o próprio destino.
    store.send('na A', 'byte', { session: store.sessions.intent('byte', 's-a')! })
    store.send('na nova', 'byte', { session: store.sessions.intent('byte', created.session.id)! })
    store.send('na principal', 'byte')
    await settle()
    // A extra relê o anúncio antes, então a principal pode sair primeiro; vale o destino de cada uma.
    const byText = new Map(sends(fake).map((b) => [b.text, b.sessionId]))
    assert.equal(byText.size, 3)
    assert.equal(byText.get('na A'), 's-a')
    assert.equal(byText.get('na nova'), created.session.id)
    assert.equal(byText.get('na principal'), undefined, 'a principal não leva sessionId')
    assert.equal(new Map(store.allEntries('byte').map((e) => [e.data.text, e.sessionId])).get('na A'), 's-a')
    // Encerrada ou a própria principal não viram intenção de extra.
    assert.equal(store.sessions.intent('byte', 's-old'), null)
    assert.equal(store.sessions.intent('byte', 'byte'), null)
    // Sem `close`/`continue` anunciados, nada é oferecido nem chamado (DON-907 C5).
    assert.equal(store.sessions.can('byte', 'close') || store.sessions.can('byte', 'continue'), false)
    assert.deepEqual(JSON.parse(JSON.stringify(await store.sessions.control('byte', 's-a', 'close'))), { ok: false, falha: 'unsupported' })
    assert.deepEqual(JSON.parse(JSON.stringify(await store.sessions.control('byte', 's-old', 'continue'))), { ok: false, falha: 'unsupported' })
    assert.equal(fake.calls.some((c) => c.method === 'sessionClose' || c.method === 'sessionContinue' || c.method === 'newSession'), false)
  } finally { store.retire() }
})

test('UI-01: Hub antigo com catálogo e sessionCreate, que ignora sessionId: nada é criado nem enviado', async () => {
  const fake = hub()
  const { store } = await open(fake)
  try {
    assert.equal(store.sessions.of('byte')?.length, 4, 'o catálogo antigo continua lido')
    assert.equal(store.sessions.can('byte', 'create'), false)
    assert.equal(store.sessions.can('byte', 'send'), false)
    assert.equal(store.sessions.intent('byte', 's-a'), null, 'sem intenção, o compositor não envia e guarda o texto')
    const created = await store.sessions.create('byte')
    assert.equal(created.ok, false)
    assert.equal(!created.ok && created.failure, 'unsupported')
    // Uma falha antiga numa extra também não é reenviada (nem à principal).
    ;(store as unknown as { upsert: (e: Entry) => void }).upsert({ id: 'local-x', seq: Number.MAX_SAFE_INTEGER, botId: 'byte', rev: 0, kind: 'user', turn: 0, createdAt: 1, updatedAt: 0, sessionId: 's-a', data: { text: 'oi', status: 'failed', clientNonce: 'x' } })
    store.retry(store.allEntries('byte').find((e) => e.id === 'local-x')!)
    await settle()
    assert.equal(fake.calls.filter((c) => c.method === 'sessionCreate').length, 0)
    assert.equal(sends(fake).length, 0, 'zero envio: nenhuma escrita na principal')
    assert.equal(store.sessions.failure?.failure, 'unsupported')
    // Anúncio ilegível vale como antigo.
    for (const actions of ['create,send', [true], ['close']]) {
      const odd = hub({ actions })
      const other = await open(odd)
      try { assert.equal(other.store.sessions.can('byte', 'send'), false, JSON.stringify(actions)) } finally { other.store.retire() }
    }
  } finally { store.retire() }
})

test('UI-01: reconectar durante a espera não envia, não adota resposta antiga e não recupera o anúncio velho', async () => {
  let gate: (() => void) | null = null
  const holding = { method: '' }
  const fake = hub({ actions: ['create', 'send'], hold: (method) => method === holding.method ? new Promise<void>((resolve) => { gate = resolve }) : undefined })
  const { store } = await open(fake)
  try {
    // Reconecta enquanto o app relê o catálogo antes de enviar: a mensagem não sai.
    holding.method = 'sessions'
    store.send('durante a troca', 'byte', { session: store.sessions.intent('byte', 's-a')! })
    await settle()
    holding.method = ''
    fake.hello()
    await settle()
    gate!()
    await settle()
    assert.equal(sends(fake).length, 0)
    assert.equal(store.sessions.failure?.failure, 'linkChanged')
    assert.equal(local(store)[0]?.data.status, 'failed', 'o texto fica na bolha com Tentar de novo')
    assert.equal(local(store)[0]?.sessionId, 's-a')
    // Resposta do envio chega depois de reconectar: não é adotada (a verdadeira vem pelo stream).
    await store.sessions.load('byte')
    holding.method = 'send'
    store.send('resposta velha', 'byte', { session: store.sessions.intent('byte', 's-b')! })
    await settle()
    holding.method = ''
    fake.hello()
    await settle()
    gate!()
    await settle()
    assert.equal(sends(fake).length, 1)
    assert.ok(!store.allEntries('byte').some((e) => !e.id.startsWith('local-') && e.data.text === 'resposta velha'))
    // Uma resposta do catálogo de antes da reconexão não devolve o anúncio, se o Hub de agora não o tem.
    await settle()
    holding.method = 'sessions'
    const stale = store.sessions.load('byte')
    await settle()
    holding.method = ''
    fake.options.actions = undefined
    const pending = gate!
    fake.hello()
    await settle()
    pending()
    await stale
    await settle()
    assert.equal(store.sessions.can('byte', 'send'), false)
    assert.equal(store.sessions.can('byte', 'create'), false)
    // Criar e reconectar no meio: a sessão não abre sozinha.
    fake.options.actions = ['create', 'send']
    await store.sessions.load('byte')
    holding.method = 'sessionCreate'
    const creating = store.sessions.create('byte')
    await settle()
    holding.method = ''
    fake.hello()
    await settle()
    gate!()
    const late = await creating
    assert.equal(!late.ok && late.failure, 'linkChanged')
  } finally { store.retire() }
})

test('UI-01: troca por Hub antigo (downgrade) ou outro Hub zera o anúncio; o envio em espera não cai na principal', async () => {
  const fake = hub({ actions: ['create', 'send'] })
  const { store } = await open(fake)
  try {
    const intent = store.sessions.intent('byte', 's-a')!
    // O mesmo computador volta com um Hub sem o anúncio.
    fake.calls.length = 0
    const old = hub()
    ;(fake as { transport: HostTransport }).transport.call = old.transport.call
    fake.hello()
    await settle()
    assert.equal(store.sessions.can('byte', 'send'), false)
    store.send('depois do downgrade', 'byte', { session: intent })
    await settle()
    assert.equal(sends(old).length, 0)
    assert.equal(store.sessions.failure?.failure, 'linkChanged')
    // Outro Hub: as listas do anterior somem.
    fake.hello('outro-hub')
    await settle()
    assert.equal(store.sessions.can('byte', 'create'), false)
  } finally { store.retire() }
})

test('UI-01: upload suspenso atravessando reconexão, downgrade ou troca de link não envia; sem troca, envia uma vez', async () => {
  const file = () => [{ id: 'f', name: 'nota.txt', data: new Uint8Array([1, 2, 3]) }]
  for (const change of ['none', 'reconnect', 'downgrade', 'relink'] as const) {
    const gate: { release: (() => void) | null } = { release: null }
    const holding = { on: true }
    const fake = hub({ actions: ['create', 'send'], hold: (method) => method === 'upload' && holding.on ? new Promise<void>((resolve) => { gate.release = resolve }) : undefined })
    const { store } = await open(fake)
    try {
      store.send('com anexo', 'byte', { files: file(), session: store.sessions.intent('byte', 's-a')! })
      await settle()
      assert.ok(gate.release, 'o upload está suspenso')
      assert.equal(fake.calls.filter((c) => c.method === 'sessions').length, 1, 'nenhuma releitura antes do último upload')
      holding.on = false
      if (change === 'reconnect') fake.hello()
      if (change === 'downgrade') { fake.options.actions = undefined; fake.hello() }
      if (change === 'relink') store.restartStream()
      await settle()
      gate.release!()
      await settle()
      if (change === 'none') {
        assert.deepEqual(sends(fake).map((b) => b.sessionId), ['s-a'], 'controle: exatamente um envio, na extra')
        const order = fake.calls.map((c) => c.method).filter((m) => m === 'upload' || m === 'sessions' || m === 'send')
        assert.deepEqual(order.slice(-3), ['upload', 'sessions', 'send'], 'a releitura é a última antes do send')
        assert.equal(local(store).length, 0)
      } else {
        assert.equal(sends(fake).length, 0, `${change}: zero envio`)
        assert.equal(local(store)[0]?.sessionId, 's-a', `${change}: a bolha fica na extra`)
        assert.equal(local(store)[0]?.data.status, 'failed')
        assert.equal(store.sessions.failure?.failure, 'linkChanged', change)
      }
    } finally { store.retire() }
  }
})

test('UI-01 B1: replay depois de status 0 com Reconectar antes do hello novo não migra a extra para o link novo', async () => {
  const file = () => [{ id: 'f', name: 'nota.txt', data: new Uint8Array([1, 2, 3]) }]
  for (const [failing, relink] of [['upload', true], ['sessions', true], ['upload', false], ['sessions', false]] as const) {
    const fake = hub({ actions: ['create', 'send'] })
    const { store, HostError } = await open(fake)
    try {
      fake.options.failNext = { method: failing, error: new HostError(0, 'falha de rede sintética') }
      const before = fake.calls.filter((c) => c.method === 'upload').length
      store.send('com anexo', 'byte', { files: file(), session: store.sessions.intent('byte', 's-a')! })
      await settle()
      if (relink) {
        // Reconectar durante a espera do replay: link novo, ainda online, hello novo segurado.
        fake.options.holdHello = true
        store.restartStream()
      }
      await new Promise((resolve) => setTimeout(resolve, 700))
      await settle()
      const uploads = fake.calls.filter((c) => c.method === 'upload').length - before
      if (relink) {
        assert.equal(sends(fake).length, 0, `${failing}: zero envio no link novo`)
        assert.equal(uploads, 1, `${failing}: só o upload do link original; nenhum byte reenviado no link novo`)
        assert.equal(local(store)[0]?.sessionId, 's-a', `${failing}: a bolha fica na extra`)
        assert.equal(local(store)[0]?.data.status, 'failed')
        assert.equal(store.sessions.failure?.failure, 'linkChanged')
        fake.releaseHello()
        await settle()
        assert.equal(sends(fake).length, 0, 'o hello novo não ressuscita o envio')
      } else {
        // Controle: o mesmo replay no link original segue e envia uma vez, na extra.
        assert.deepEqual(sends(fake).map((b) => b.sessionId), ['s-a'], `${failing}: controle envia uma vez`)
        assert.equal(local(store).length, 0)
      }
    } finally { store.retire() }
  }
})

test('UI-01 B2: Nova sessão pelo menu cria uma vez em cada computador; voltar ou reconectar não repete', async () => {
  const a = hub({ actions: ['create', 'send'] })
  const b = hub({ actions: ['create', 'send'] })
  const first = await open(a)
  const second = await open(b)
  const created = (fake: ReturnType<typeof hub>) => fake.calls.filter((c) => c.method === 'sessionCreate').length
  try {
    for (const [{ store }, fake] of [[first, a], [second, b]] as const) {
      store.sessions.requestNew('byte')
      // O que o hook faz na conversa aberta: consome no modelo desta origem e cria.
      if (store.sessions.takeNew('byte', 2_000)) await store.sessions.create('byte')
      assert.equal(created(fake), 1, 'cada computador cria a sua, mesmo com seq igual')
      // Voltar à conversa (outro consumidor, depois de desmontar) não consome de novo.
      assert.equal(store.sessions.takeNew('byte', 2_000), false)
    }
    // Pedido de antes da reconexão não roda depois dela.
    first.store.sessions.requestNew('byte')
    a.hello()
    await settle()
    assert.equal(first.store.sessions.takeNew('byte', 2_000), false)
    assert.equal(created(a), 1)
    // Pedido para outra conversa não é consumido aqui.
    second.store.sessions.requestNew('outra')
    assert.equal(second.store.sessions.takeNew('byte', 2_000), false)
  } finally { first.store.retire(); second.store.retire() }
})

test('UI-01: Hub que grava fora da extra depois da releitura: a entrada aparece onde está e o aviso diz isso', async () => {
  const fake = hub({ actions: ['create', 'send'], ignoreSessionId: true })
  const { store } = await open(fake)
  try {
    store.send('para a extra', 'byte', { session: store.sessions.intent('byte', 's-a')! })
    await settle()
    assert.equal(sends(fake).length, 1, 'sem segundo envio')
    assert.equal(local(store).length, 0, 'nada fica como "não enviada"')
    assert.equal(store.allEntries('byte').find((e) => e.data.text === 'para a extra')?.sessionId, 'byte')
    assert.equal(store.sessions.failure?.failure, 'misrouted')
  } finally { store.retire() }
})

test('UI-01: recusa do Hub vira código; nenhum texto do Hub, do motor ou do cofre chega à tela', async () => {
  for (const [error, failure] of [
    ['session_closed', 'closed'],
    ['unknown_session', 'unknownSession'],
    ['session_selection_unavailable: esta sessão precisa de conta gerenciada com modelo escolhido.', 'selectionUnavailable'],
    ['cofre: token sk-123 inválido em /Users/dono', 'failed'],
  ] as const) {
    const fake = hub({ actions: ['create', 'send'], sendError: error })
    const { store } = await open(fake)
    try {
      store.send('texto que fica', 'byte', { session: store.sessions.intent('byte', 's-a')! })
      await settle()
      assert.equal(store.sessions.failure?.failure, failure, error)
      assert.equal(store.sessions.failure?.sessionId, 's-a')
      assert.equal(sends(fake).length, 1, 'sem segundo envio nem fallback à principal')
      assert.equal(local(store)[0]?.data.text, 'texto que fica')
      assert.equal(local(store)[0]?.data.status, 'failed')
      assert.equal(store.lastError, null, 'o erro bruto não vai para o aviso geral')
    } finally { store.retire() }
  }
})

test('UI-01: canal e Space não ganham extras; rascunhos por sessão separados da principal', async () => {
  const fake = hub({ actions: ['create', 'send'] })
  const { store, extrasAllowed, normalizeBot } = await open(fake)
  try {
    const bot = store.bots.get('byte')!
    assert.equal(extrasAllowed(store, bot), true)
    assert.equal(extrasAllowed(store, normalizeBot({ id: 'g', kind: 'group', name: 'Canal', members: ['byte'] }) as Bot), false)
    assert.equal(extrasAllowed({ spaceId: 'space-x', spaces: { personalSpaceId: 'pessoal' } as never }, bot), false)
    assert.equal(extrasAllowed({ spaceId: 'pessoal', spaces: { personalSpaceId: 'pessoal' } as never }, bot), true)
  } finally { store.retire() }
  const { ComposerDrafts, draftKey } = await import('../../shared/composer-drafts.ts')
  const drafts = new ComposerDrafts({ getItem: () => null, setItem() {} }, 'k')
  drafts.edit(draftKey('byte', 's-a'), 'da extra', 'typed')
  drafts.edit(draftKey('byte', null), 'da principal', 'typed')
  assert.equal(drafts.get(draftKey('byte', 's-a')), 'da extra')
  assert.equal(drafts.get('byte'), 'da principal')
  assert.equal(draftKey('byte', 'byte'), 'byte')
  // #431: os anexos são do rascunho; cada aba tem os seus, e trocar de aba não os leva.
  const file = (id: string) => ({ id, name: `${id}.txt`, data: new Uint8Array([1]) })
  drafts.setFiles(draftKey('byte', null), [file('p')])
  drafts.setFiles(draftKey('byte', 's-a'), [file('a')])
  drafts.setFiles(draftKey('byte', 's-b'), [file('b')])
  assert.deepEqual(drafts.files(draftKey('byte', null)).map((f) => f.id), ['p'])
  assert.deepEqual(drafts.files(draftKey('byte', 's-a')).map((f) => f.id), ['a'])
  assert.deepEqual(drafts.files(draftKey('byte', 's-b')).map((f) => f.id), ['b'])
  const guardA = drafts.fileGuard(draftKey('byte', 's-a'))
  drafts.setFiles(draftKey('byte', 's-a'), [])
  assert.equal(guardA(), false, 'leitura atrasada não revive anexo da extra enviada')
  assert.deepEqual(drafts.files(draftKey('byte', null)).map((f) => f.id), ['p'], 'a principal fica com os dela')
  drafts.removeBot('byte')
  assert.equal(drafts.get(draftKey('byte', 's-a')), '')
  assert.equal(drafts.files(draftKey('byte', 's-b')).length, 0)
  assert.equal(drafts.files('byte').length, 0)
})

test('UI-01 + #431: com anexos em três abas, enviar na extra B leva só os dela e só para B', async () => {
  const fake = hub({ actions: ['create', 'send'] })
  const { store } = await open(fake)
  try {
    const drafts = store.composerDrafts
    const { draftKey } = await import('../../shared/composer-drafts.ts')
    const file = (id: string) => ({ id, name: `${id}.txt`, data: new Uint8Array([1]) })
    for (const [session, id] of [[null, 'p'], ['s-a', 'a'], ['s-b', 'b']] as const) drafts.setFiles(draftKey('byte', session), [file(id)])
    // O que o compositor da aba B faz no Enter: intenção e anexos da própria chave, capturados agora.
    const key = draftKey('byte', 's-b')
    store.send('na B', 'byte', { files: drafts.files(key), session: store.sessions.intent('byte', 's-b')! })
    drafts.setFiles(key, [])
    await settle()
    const uploads = fake.calls.filter((c) => c.method === 'upload').map((c) => c.body.name)
    assert.deepEqual(uploads, ['b.txt'])
    assert.deepEqual(sends(fake).map((b) => b.sessionId), ['s-b'])
    assert.deepEqual(drafts.files(draftKey('byte', null)).map((f) => f.id), ['p'])
    assert.deepEqual(drafts.files(draftKey('byte', 's-a')).map((f) => f.id), ['a'])
  } finally { store.retire() }
})

// UI01b / D372: o app só reconhece `engine` com o anúncio positivo deste link/época, valida a projeção da conversa
// e nunca troca o modelo pelo do bot. O seletor é da C9; aqui só o reconhecimento e o rótulo.
const engine = (revision: number, extra: Record<string, unknown> = {}) => ({
  accountId: `ac_${'a'.repeat(64)}`, backend: 'codex', model: 'model-a', modelOptions: { effort: 'high' }, fastMode: false, source: 'own', revision, ...extra,
})

test('UI01b: engine só com anúncio positivo atual; projeção validada; extra sem projeção não cai na principal', async () => {
  const fake = hub({ actions: ['create', 'send', 'engine'] })
  fake.sessions[0] = { ...row('byte', true), engine: engine(1) } as ReturnType<typeof row>
  fake.sessions[1] = { ...row('s-a'), engine: engine(99) } as ReturnType<typeof row>
  fake.sessions[2] = { ...row('s-b'), engine: null } as ReturnType<typeof row>
  const { store } = await open(fake)
  try {
    assert.equal(store.sessions.can('byte', 'engine'), true)
    const list = store.sessions.of('byte')!
    assert.equal(JSON.stringify(list.find((s) => s.principal)?.engine), JSON.stringify(engine(1)))
    assert.equal(list.find((s) => s.id === 's-a')?.engine, null, 'revisão diferente da sessão = indisponível')
    assert.equal(list.find((s) => s.id === 's-b')?.engine, null)
    assert.equal(rotuloModelo(true, list, 'byte', null).kind, 'engine')
    assert.equal(rotuloModelo(true, list, 'byte', 's-a').kind, 'unavailable')
    assert.equal(rotuloModelo(true, list, 'byte', 's-sumiu').kind, 'unavailable', 'nunca o modelo da principal numa extra')
    assert.equal(rotuloModelo(false, list, 'byte', null).kind, 'hidden')
    // Evento `session` também é validado: forma inválida vira indisponível, não o valor anterior.
    store.sessions.receive({ type: 'session', session: { ...row('byte', true), revision: 2, engine: engine(2, { source: 'talvez' }) } } as never)
    assert.equal(store.sessions.of('byte')!.find((s) => s.principal)?.engine, null)
    // Downgrade no mesmo computador: sem o anúncio, nada de `engine`.
    const old = hub({ actions: ['create', 'send'] })
    ;(fake as { transport: HostTransport }).transport.call = old.transport.call
    fake.hello()
    await settle()
    assert.equal(store.sessions.can('byte', 'engine'), false)
  } finally { store.retire() }
})

test('UI01b: projeção engine estrita e sem completar campos', () => {
  assert.deepEqual(engineOf(engine(3, { accountId: null, model: null, source: 'botDefault' }), 3), engine(3, { accountId: null, model: null, source: 'botDefault' }))
  for (const bad of [null, [], engine(3, { fastMode: 'false' }), engine(3, { modelOptions: { fast: false } }), engine(3, { backend: 7 }), { ...engine(3), model: undefined }]) {
    assert.equal(engineOf(bad, 3), null, JSON.stringify(bad))
  }
  assert.equal(engineOf(engine(3), 4), null)
})

// R9 (D372 × C9): o chip usa só `sessions.can(chat, 'engine')`. Com o MESMO cliente, uma falha transitória não deixa
// a troca presa em só leitura, um hello sem o anúncio retira a capacidade anterior e uma resposta tardia da época
// velha não a religa.
test('R9: mesmo cliente — falha, hello com anúncio, hello sem anúncio e resposta tardia da época velha', async () => {
  const fake = hub({ actions: ['create', 'send', 'engine'], failNext: { method: 'sessions', error: new Error('ocupado') } })
  fake.sessions[0] = { ...row('byte', true), engine: engine(1) } as ReturnType<typeof row>
  const { store } = await open(fake)
  try {
    assert.equal(store.sessions.can('byte', 'engine'), false, 'a leitura falhou: sem capacidade, sem chute')
    fake.hello()
    await settle()
    assert.equal(store.sessions.can('byte', 'engine'), true, 'o hello relê no mesmo cliente e recupera')
    // Uma leitura da época atual fica segura; o hub troca para uma versão sem o anúncio e manda outro hello.
    let release: () => void = () => {}
    fake.options.hold = (method) => (method === 'sessions' ? new Promise<void>((r) => { release = r }) : undefined)
    void store.sessions.load('byte')
    await settle()
    fake.options.hold = undefined
    fake.options.actions = ['create', 'send']
    fake.hello()
    await settle()
    assert.equal(store.sessions.can('byte', 'engine'), false, 'hello sem o anúncio: nada da capacidade anterior')
    release()
    await settle()
    assert.equal(store.sessions.can('byte', 'engine'), false, 'a resposta tardia (com engine) da época velha é ignorada')
  } finally { store.retire() }
})

const plain = (v: unknown) => JSON.parse(JSON.stringify(v)) as unknown

test('DON-907 C5: encerrar confirmado muda o catálogo; resposta incerta relê e não mostra encerrada', async () => {
  const fake = hub({ actions: ['create', 'send', 'close', 'continue'] })
  const { store } = await open(fake)
  try {
    const state = (id: string) => store.sessions.of('byte')?.find((s) => s.id === id)?.state
    // `session_control_pending`: o Hub não respondeu em 10 s. A lista é relida e a extra continua como o Hub diz.
    fake.options.failNext = { method: 'sessionClose', error: new Error('session_control_pending') }
    const reads = fake.calls.filter((c) => c.method === 'sessions').length
    const pending = await store.sessions.control('byte', 's-a', 'close')
    assert.deepEqual(plain(pending), { ok: false, falha: 'pending' })
    assert.equal(fake.calls.filter((c) => c.method === 'sessions').length, reads + 1, 'relê `sessions`')
    assert.equal(state('s-a'), 'open', 'sem otimismo')
    // Confirmado: com a revisão vista, e a extra fica encerrada pela resposta do Hub.
    const closed = await store.sessions.control('byte', 's-a', 'close')
    assert.ok(closed.ok)
    assert.deepEqual(plain(fake.calls.filter((c) => c.method === 'sessionClose').at(-1)?.body), { botId: 'byte', sessionId: 's-a', expectedRevision: 1 })
    assert.equal(state('s-a'), 'closed')
    assert.equal(store.sessions.intent('byte', 's-a'), null, 'encerrada não recebe envio')
    // A principal e uma extra já encerrada não são encerradas (nem chamadas).
    const before = fake.calls.filter((c) => c.method === 'sessionClose').length
    assert.equal((await store.sessions.control('byte', 'byte', 'close')).ok, false)
    assert.equal((await store.sessions.control('byte', 's-old', 'close')).ok, false)
    assert.equal(fake.calls.filter((c) => c.method === 'sessionClose').length, before)
  } finally { store.retire() }
})

test('DON-907 C5: continuar só numa extra encerrada, ocupado até a resposta, e devolve a sessão nova', async () => {
  let release: (() => void) | null = null
  const fake = hub({ actions: ['create', 'send', 'close', 'continue'], hold: (method) => method === 'sessionContinue' ? new Promise<void>((r) => { release = r }) : undefined })
  const { store } = await open(fake)
  try {
    // Aberta não continua (sem chamada).
    assert.deepEqual(plain(await store.sessions.control('byte', 's-a', 'continue')), { ok: false, falha: 'failed' })
    assert.equal(fake.calls.some((c) => c.method === 'sessionContinue'), false)
    const first = store.sessions.control('byte', 's-old', 'continue')
    const second = store.sessions.control('byte', 's-old', 'continue')
    assert.equal(first, second, 'dois cliques = um pedido')
    assert.equal(store.sessions.controlBusy('s-old'), true, 'o botão fica desabilitado até a resposta')
    await settle()
    release!()
    const done = await first
    assert.ok(done.ok)
    assert.equal(store.sessions.controlBusy('s-old'), false)
    assert.equal(fake.calls.filter((c) => c.method === 'sessionContinue').length, 1)
    assert.deepEqual(plain(fake.calls.find((c) => c.method === 'sessionContinue')?.body), { botId: 'byte', sessionId: 's-old', expectedRevision: 1 })
    assert.equal(done.session.state, 'open')
    assert.notEqual(done.session.id, 's-old')
    assert.ok(store.sessions.of('byte')?.some((s) => s.id === done.session.id), 'a nova entra no catálogo para abrir')
    assert.equal(store.sessions.of('byte')?.find((s) => s.id === 's-old')?.state, 'closed', 'a origem não reabre')
    // Erro do Hub vira código e relê o catálogo.
    fake.options.failNext = { method: 'sessionContinue', error: new Error('session_revision_conflict') }
    const reads = fake.calls.filter((c) => c.method === 'sessions').length
    const conflict = store.sessions.control('byte', 's-old', 'continue')
    await settle()
    release!()
    assert.deepEqual(plain(await conflict), { ok: false, falha: 'conflict' })
    assert.equal(fake.calls.filter((c) => c.method === 'sessions').length, reads + 1)
  } finally { store.retire() }
})

test('DON-907 C5 R1: releitura antiga (de uma resposta incerta) não desfaz um encerrar ou continuar confirmado depois', async () => {
  let release: (() => void) | null = null
  let holdSessions = false
  const fake = hub({ actions: ['create', 'send', 'close', 'continue'], hold: (method) => method === 'sessions' && holdSessions ? new Promise<void>((r) => { release = r }) : undefined })
  const { store } = await open(fake)
  try {
    const row = (id: string) => store.sessions.of('byte')?.find((s) => s.id === id)
    // A fica incerta e dispara a releitura, que demora; B confirma enquanto isso; a leitura chega com B aberta.
    holdSessions = true
    fake.options.failNext = { method: 'sessionClose', error: new Error('session_control_pending') }
    const a = store.sessions.control('byte', 's-a', 'close')
    await settle()
    holdSessions = false
    const b = store.sessions.control('byte', 's-b', 'close')
    await settle()
    release!()
    assert.ok((await b).ok)
    await a
    await settle()
    assert.equal(row('s-b')?.state, 'closed', 'a confirmação de B vale depois da leitura antiga')
    // Continuar com a leitura antiga em voo: a nova sessão não some do catálogo.
    holdSessions = true
    fake.options.failNext = { method: 'sessionClose', error: new Error('session_control_pending') }
    const c = store.sessions.control('byte', 's-a', 'close')
    await settle()
    holdSessions = false
    const cont = store.sessions.control('byte', 's-old', 'continue')
    await settle()
    release!()
    const done = await cont
    await c
    await settle()
    assert.ok(done.ok)
    assert.equal(row(done.session.id)?.state, 'open')
  } finally { store.retire() }
})

test('DON-907 C5 R2: reconectar durante a espera da leitura antiga devolve linkChanged, sem sucesso nem sessão para abrir', async () => {
  // (1) Reconectar troca o link antes do hello novo (a época ainda é a mesma); (2) o hello chega durante a espera.
  for (const mode of ['relink', 'hello'] as const) {
    let release: (() => void) | null = null
    let holdSessions = false
    const fake = hub({ actions: ['create', 'send', 'close', 'continue'], hold: (method) => method === 'sessions' && holdSessions ? new Promise<void>((r) => { release = r }) : undefined })
    const { store } = await open(fake)
    try {
      holdSessions = true
      fake.options.failNext = { method: 'sessionClose', error: new Error('session_control_pending') }
      const pending = store.sessions.control('byte', 's-a', 'close')
      await settle()
      holdSessions = false
      const late = store.sessions.control('byte', 's-old', 'continue')
      await settle()
      if (mode === 'relink') {
        fake.options.holdHello = true
        store.restartStream()
      } else fake.hello()
      await settle()
      release!()
      assert.deepEqual(plain(await late), { ok: false, falha: 'linkChanged' }, `${mode}: sem sucesso, nada a abrir`)
      await pending
      if (mode === 'relink') fake.releaseHello()
      await settle()
    } finally { store.retire() }
  }
})

// G1b (sessionChoiceAnswer v1, R3 B1): no modelo REAL, `choice` vale sozinho (sem `send`) e um hello no mesmo cliente
// durante a releitura invalida a intenção: nenhuma RPC nem adoção.
const choiceCard = { questionId: 'q1', snapshot: '{}', hub: { state: 'pending' as const, revision: 1 }, spaceId: null, conversationId: 'byte', botId: 'byte', sessionId: 's-a' }
const choiceIntent = { questionId: 'q1', revision: 1, snapshot: '{}', answer: { kind: 'option', id: 'a' } as const }

test('G1b: anúncio choice sem send libera só a intenção choice e a resposta sai com os seis campos', async () => {
  assert.deepEqual([...parseSessionActions(['choice'])], ['choice'])
  const fake = hub({ actions: ['choice'] })
  const { store } = await open(fake)
  // O `hello` RPC do Hub real traz `hostId` (o fixture não): o envio transitório é guardado por Hub.
  store.hello = { ...store.hello!, hostId: 'hub' }
  try {
    assert.ok(store.sessions.intent('byte', 's-a', 'choice'))
    assert.equal(store.sessions.intent('byte', 's-a'), null, 'o padrão continua exigindo send')
    assert.equal(store.sessions.intent('byte', 'byte', 'choice'), null, 'nunca a principal')
    assert.equal(store.sessions.intent('byte', 's-old', 'choice'), null, 'extra encerrada')
    const model = new ChoiceAnswers()
    await model.answer(store, choiceCard, choiceIntent)
    const answers = fake.calls.filter((c) => c.method === 'sessionChoiceAnswer')
    assert.deepEqual(answers.map((c) => c.body), [{ botId: 'byte', sessionId: 's-a', expectedRevision: 1, questionId: 'q1', revision: 1, answer: { kind: 'option', id: 'a' } }])
    assert.equal(model.submission(store, 'q1')?.error, null)
    // Sem `choice` (Hub com só create/send): nenhuma RPC de resposta.
    const old = hub({ actions: ['create', 'send'] })
    const legacy = await open(old)
    legacy.store.hello = { ...legacy.store.hello!, hostId: 'hub' }
    try {
      const m2 = new ChoiceAnswers()
      await m2.answer(legacy.store, choiceCard, choiceIntent)
      assert.equal(old.calls.filter((c) => c.method === 'sessionChoiceAnswer').length, 0)
      assert.equal(m2.submission(legacy.store, 'q1')?.error, 'unsupported')
    } finally { legacy.store.retire() }
  } finally { store.retire() }
})

test('G1b: hello no mesmo cliente durante a releitura invalida a intenção choice (sem RPC nem adoção)', async () => {
  const fake = hub({ actions: ['choice'] })
  const { store } = await open(fake)
  // O `hello` RPC do Hub real traz `hostId` (o fixture não): o envio transitório é guardado por Hub.
  store.hello = { ...store.hello!, hostId: 'hub' }
  try {
    let release: () => void = () => {}
    fake.options.hold = (method) => (method === 'sessions' ? new Promise<void>((r) => { release = r }) : undefined)
    const model = new ChoiceAnswers()
    const sending = model.answer(store, choiceCard, choiceIntent)
    await settle()
    fake.options.hold = undefined
    fake.hello()
    await settle()
    release()
    await sending
    assert.equal(fake.calls.filter((c) => c.method === 'sessionChoiceAnswer').length, 0)
    assert.equal(model.submission(store, 'q1')?.error, 'link')
  } finally { store.retire() }
})

test('D545: arquivar ID igual em dois stores não cruza catálogos; hello invalida a época do menu', async () => {
  const a = hub({ actions: ['close'] })
  const b = hub({ actions: ['close'] })
  const first = await open(a), second = await open(b)
  try {
    const epoch = first.store.sessions.epoch
    assert.ok((await first.store.sessions.control('byte', 's-a', 'close')).ok)
    assert.equal(first.store.sessions.of('byte')?.find((s) => s.id === 's-a')?.state, 'closed')
    assert.equal(second.store.sessions.of('byte')?.find((s) => s.id === 's-a')?.state, 'open')
    assert.equal(b.calls.filter((c) => c.method === 'sessionClose').length, 0)
    assert.deepEqual(JSON.parse(JSON.stringify(a.calls.find((c) => c.method === 'sessionClose')?.body)), { botId: 'byte', sessionId: 's-a', expectedRevision: 1 })
    a.hello()
    await settle()
    assert.ok(first.store.sessions.epoch > epoch)
  } finally { first.store.retire(); second.store.retire() }
})

test('D545 exclusão: resposta confirmada remove catálogo/mensagens só do store e da sessão alvo', async () => {
  const fake = hub({ actions: ['delete'] }), other = hub({ actions: ['delete'] })
  const a = await open(fake), b = await open(other)
  try {
    a.store.spaces = b.store.spaces = { personalSpaceId: 'pessoal' } as never
    const insert = (store: typeof a.store, sessionId: string, id: string) => (store as unknown as { upsert: (e: Entry) => void }).upsert({ id, seq: 1, botId: 'byte', rev: 1, kind: 'user', turn: 1, createdAt: 1, updatedAt: 1, sessionId, data: { text: 'fixture', status: 'sent' } })
    insert(a.store, 's-a', 'apagar'); insert(a.store, 's-b', 'preservar'); insert(b.store, 's-a', 'outro-store')
    const result = await a.store.sessions.remove('byte', 's-a', 'pessoal', 1)
    assert.equal(result.ok, true)
    assert.equal(a.store.sessions.of('byte')?.some((s) => s.id === 's-a'), false)
    assert.deepEqual(Array.from(a.store.allEntries('byte'), (e) => e.id), ['preservar'])
    assert.equal(b.store.allEntries('byte')[0]?.id, 'outro-store')
    assert.equal(b.store.sessions.of('byte')?.some((s) => s.id === 's-a'), true)
    assert.deepEqual(JSON.parse(JSON.stringify(fake.calls.find((c) => c.method === 'sessionDelete')?.body)), { hubId: 'hub', spaceId: 'pessoal', botId: 'byte', sessionId: 's-a', expectedRevision: 1 })
    insert(a.store, 's-a', 'histórico-atrasado')
    assert.deepEqual(Array.from(a.store.allEntries('byte'), (e) => e.id), ['preservar'])
    assert.equal(other.calls.some((c) => c.method === 'sessionDelete'), false)
  } finally { a.store.retire(); b.store.retire() }
})

test('D545 exclusão: Hub antigo, principal, revisão alterada e resposta de ID/Space trocados não removem dados', async () => {
  for (const change of [undefined, { sessionId: 's-b' }, { chatId: 'outro' }, { spaceId: 'outro-space' }, { rev: -1 }]) {
    const fake = hub({ actions: change ? ['delete'] : [], deleteReply: change ? { type: 'sessionDeleted', spaceId: 'pessoal', chatId: 'byte', sessionId: 's-a', rev: 90, ...change } : undefined })
    const { store } = await open(fake)
    try {
      store.spaces = { personalSpaceId: 'pessoal' } as never
      assert.equal((await store.sessions.remove('byte', 'byte', 'pessoal')).ok, false)
      assert.equal((await store.sessions.remove('byte', 's-a', 'pessoal', 9)).ok, false)
      assert.equal((await store.sessions.remove('byte', 's-a', 'pessoal', 1)).ok, false)
      assert.equal(store.sessions.of('byte')?.some((s) => s.id === 's-a'), true)
      assert.equal(store.sessions.isDeleted('byte', 's-a'), false)
    } finally { store.retire() }
  }
})

test('D545 exclusão: tombstone vence catálogo antigo; reconexão durante pedido invalida resposta', async () => {
  let release: () => void = () => {}
  let holdCatalog = false
  const fake = hub({ actions: ['delete'], hold: (method) => method === 'sessions' && holdCatalog ? new Promise<void>((r) => { release = r }) : undefined })
  const { store } = await open(fake)
  try {
    store.spaces = { personalSpaceId: 'pessoal' } as never
    holdCatalog = true
    const stale = store.sessions.load('byte'); await settle()
    assert.equal((await store.sessions.remove('byte', 's-a', 'pessoal')).ok, true)
    holdCatalog = false; release(); await stale
    assert.equal(store.sessions.of('byte')?.some((s) => s.id === 's-a'), false)
    let resume: () => void = () => {}
    fake.options.hold = (method) => method === 'sessionDelete' ? new Promise<void>((r) => { resume = r }) : undefined
    const pending = store.sessions.remove('byte', 's-b', 'pessoal'); await settle()
    fake.hello('outro-hub'); await settle(); resume()
    const result = await pending
    assert.equal(result.ok, false)
    assert.equal(result.falha, 'linkChanged')
    assert.equal(store.sessions.isDeleted('byte', 's-b'), false)
  } finally { release(); store.retire() }
})

test('D545 exclusão: session_busy do Host preserva o catálogo e orienta encerrar antes', async () => {
  const fake = hub({ actions: ['delete'] }); const { store } = await open(fake)
  try {
    store.spaces = { personalSpaceId: 'pessoal' } as never
    fake.options.failNext = { method: 'sessionDelete', error: new Error('session_busy') }
    const result = await store.sessions.remove('byte', 's-a', 'pessoal')
    assert.equal(result.falha, 'busy')
    assert.equal(store.sessions.of('byte')?.some((s) => s.id === 's-a'), true)
  } finally { store.retire() }
})


test('D545 exclusão: janela só da sessão removida recarrega mensagens restantes sem ressuscitar apagadas', async () => {
  const entry = (id: string, sessionId: string): Entry => ({ id, sessionId, botId: 'byte', seq: 1, rev: 1, kind: 'user', turn: 1, createdAt: 1, updatedAt: 1, data: { text: 'fixture', status: 'sent' } })
  const fake = hub({ actions: ['delete'], historyEntries: [entry('atrasada', 's-a'), entry('restante', 's-b')] })
  const { store } = await open(fake)
  try {
    store.spaces = { personalSpaceId: 'pessoal' } as never
    ;(store as unknown as { upsert: (e: Entry) => void }).upsert(entry('apagar', 's-a'))
    assert.equal((await store.sessions.remove('byte', 's-a', 'pessoal')).ok, true)
    await settle()
    assert.deepEqual(Array.from(store.allEntries('byte'), (e) => e.id), ['restante'])
  } finally { store.retire() }
})
