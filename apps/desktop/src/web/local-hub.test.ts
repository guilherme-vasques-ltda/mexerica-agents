import assert from 'node:assert/strict'
import test from 'node:test'
import type { Entry } from '../shared/models.ts'
import { createLocalWebHost, LocalWebSession } from './local-hub.ts'
import { createWebBridge } from './platform.ts'

const ORIGIN = 'http://127.0.0.1:19423'

function resposta(status: number, corpo?: unknown): Response {
  return new Response(corpo === undefined ? null : JSON.stringify(corpo), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

const SESSAO = { authenticated: true, csrfToken: 'csrf-1', computerId: 'c1', hostId: 'h1', hostName: 'Meu computador' }

/** fetch falso com fila de respostas (Response, Erro ou fábrica que recebe o init) e registro das chamadas. */
function fetchFalso() {
  const chamadas: { url: string; init?: RequestInit }[] = []
  const fila: Array<Response | Error | ((init: RequestInit) => Response | Promise<Response>)> = []
  const impl = (async (url: string | URL, init?: RequestInit) => {
    chamadas.push({ url: String(url), init })
    const proxima = fila.shift()
    if (proxima instanceof Error) throw proxima
    if (typeof proxima === 'function') return proxima(init ?? {})
    if (!proxima) throw new Error('fila vazia')
    return proxima
  }) as typeof fetch
  return { chamadas, fila, impl }
}

async function sessaoAutenticada(origin = ORIGIN): Promise<{ session: LocalWebSession; fetch: ReturnType<typeof fetchFalso> }> {
  const falso = fetchFalso()
  falso.fila.push(resposta(200, SESSAO))
  const session = new LocalWebSession(origin, { fetchImpl: falso.impl })
  await session.checkSession()
  return { session, fetch: falso }
}

test('checkSession autentica com o corpo completo e pede os headers da sessão', async () => {
  const { session, fetch } = await sessaoAutenticada()
  assert.equal(session.isAuthenticated, true)
  assert.equal(session.csrfToken, 'csrf-1')
  assert.equal(session.computerId, 'c1')
  assert.equal(session.hostName, 'Meu computador')
  const init = fetch.chamadas[0]!.init!
  assert.equal(init.credentials, 'same-origin')
  assert.equal(init.cache, 'no-store')
  assert.equal(new Headers(init.headers as HeadersInit).get('X-Mexerica-Web'), '1')
})

test('checkSession com 401 desautentica e avisa quem assina', async () => {
  const falso = fetchFalso()
  falso.fila.push(resposta(200, SESSAO), resposta(401, { authenticated: false }))
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  await session.checkSession()
  const eventos: boolean[] = []
  session.subscribe((a) => eventos.push(a))
  assert.equal(await session.checkSession(), false)
  assert.equal(session.isAuthenticated, false)
  assert.deepEqual(eventos, [false])
})

test('corpo inválido não autentica (fail-closed), mesmo com 200', async () => {
  const falso = fetchFalso()
  falso.fila.push(resposta(200, { authenticated: true, csrfToken: 'csrf-1' }))
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  assert.equal(await session.checkSession(), false)
  assert.equal(session.csrfToken, null)
})

test('rede caída no checkSession não desautentica uma sessão viva', async () => {
  const { session, fetch: falso } = await sessaoAutenticada()
  falso.fila.push(new Error('sem rede'))
  assert.equal(await session.checkSession(), true)
})

test('login recusa token vazio sem falar com o Hub', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  assert.equal(await session.login(''), 'invalid')
  assert.equal(await session.login('   '), 'invalid')
  assert.equal(falso.chamadas.length, 0)
})

test('login devolve categoria limpa para 401, 429, 500 e rede caída', async () => {
  for (const [status, esperado] of [[401, 'invalid'], [429, 'rateLimited'], [500, 'error']] as const) {
    const falso = fetchFalso()
    falso.fila.push(resposta(status, {}))
    const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
    assert.equal(await session.login('tokem'), esperado)
    assert.equal(session.isAuthenticated, false)
  }
  const falso = fetchFalso()
  falso.fila.push(new Error('sem rede'))
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  assert.equal(await session.login('tokem'), 'offline')
})

test('login aceito guarda a sessão e avisa quem assina', async () => {
  const falso = fetchFalso()
  falso.fila.push(resposta(200, SESSAO))
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  const eventos: boolean[] = []
  session.subscribe((a) => eventos.push(a))
  assert.equal(await session.login('token-bom'), null)
  assert.equal(session.isAuthenticated, true)
  assert.deepEqual(eventos, [true])
  const init = falso.chamadas[0]!.init!
  assert.equal(init.credentials, 'same-origin')
  const headers = new Headers(init.headers as HeadersInit)
  assert.equal(headers.get('Content-Type'), 'application/json')
  assert.equal(headers.get('X-Mexerica-Web'), '1')
  assert.equal(JSON.parse(String(init.body)).token, 'token-bom')
})

test('logout envia o CSRF e esquece a sessão', async () => {
  const { session, fetch } = await sessaoAutenticada()
  fetch.fila.push(resposta(204))
  await session.logout()
  assert.equal(session.isAuthenticated, false)
  const init = fetch.chamadas[1]!.init!
  assert.equal(init.method, 'POST')
  assert.equal(String(fetch.chamadas[1]!.url), `${ORIGIN}/web/auth/logout`)
  assert.equal(new Headers(init.headers as HeadersInit).get('X-Mexerica-Csrf'), 'csrf-1')
})

test('logout sem sessão não fala com o Hub', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  await session.logout()
  assert.equal(falso.chamadas.length, 0)
})

test('handleUnauthorized só dispara expiração quando havia sessão', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  let expirou = 0
  session.onExpired(() => { expirou += 1 })
  session.handleUnauthorized()
  assert.equal(expirou, 0)

  const { session: viva } = await sessaoAutenticada()
  let expirouViva = 0
  viva.onExpired(() => { expirouViva += 1 })
  const eventos: boolean[] = []
  viva.subscribe((a) => eventos.push(a))
  viva.handleUnauthorized()
  assert.equal(viva.isAuthenticated, false)
  assert.equal(expirouViva, 1)
  assert.deepEqual(eventos, [false])
})

test('snapshot do host: running com a origem quando autenticado, failed sem sessão', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  const host = createLocalWebHost(session)
  assert.deepEqual(await host.snapshot(), { state: { kind: 'failed', message: '' }, local: null, dev: false, logPath: '' })

  falso.fila.push(resposta(200, SESSAO))
  await session.checkSession()
  assert.deepEqual(await host.snapshot(), {
    state: { kind: 'running' },
    local: { computerId: 'c1', baseURL: ORIGIN, token: 'web-session' },
    dev: false,
    logPath: '',
  })
})

test('onChange do host segue a sessão', async () => {
  const falso = fetchFalso()
  falso.fila.push(resposta(200, SESSAO))
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  const host = createLocalWebHost(session)
  const instantaneos: unknown[] = []
  host.onChange((s) => instantaneos.push(s))
  await session.checkSession()
  assert.equal((instantaneos[0] as { state: { kind: string } }).state.kind, 'running')
  await session.logout()
  assert.equal((instantaneos[1] as { state: { kind: string } }).state.kind, 'failed')
})

test('host.call recusa base de outra origem e método inválido', async () => {
  const { session } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  await assert.rejects(host.call('http://127.0.0.1:9999', '', 'bots_list', {}, 1000), (e: { status: number }) => e.status === 0)
  await assert.rejects(host.call(ORIGIN, '', 'bots list', {}, 1000), (e: { status: number }) => e.status === 0)
  await assert.rejects(host.call(ORIGIN, '', 'a/b', {}, 1000), (e: { status: number }) => e.status === 0)
})

test('host.call sem sessão recusa sem falar com o Hub', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  const host = createLocalWebHost(session)
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), (e: { status: number }) => e.status === 401)
  assert.equal(falso.chamadas.length, 0)
})

test('host.call manda CSRF, credencial de mesma origem e devolve o JSON', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  fetch.fila.push(resposta(200, { ok: true, lista: [1, 2] }))
  const valor = await host.call(ORIGIN, 'web-session', 'bots_list', { filtro: null }, 5000)
  assert.deepEqual(valor, { ok: true, lista: [1, 2] })
  const [url, init] = [fetch.chamadas[1]!.url, fetch.chamadas[1]!.init!]
  assert.equal(url, `${ORIGIN}/api/bots_list`)
  assert.equal(init.method, 'POST')
  assert.equal(init.credentials, 'same-origin')
  assert.equal(init.cache, 'no-store')
  const headers = new Headers(init.headers as HeadersInit)
  assert.equal(headers.get('Content-Type'), 'application/json')
  assert.equal(headers.get('X-Mexerica-Csrf'), 'csrf-1')
})

test('host.call com 401 derruba a sessão e dispara expiração; erro comum preserva só status e código conhecido', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  let expirou = 0
  session.onExpired(() => { expirou += 1 })

  fetch.fila.push(resposta(401, { authenticated: false }))
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), (e: { status: number }) => e.status === 401)
  assert.equal(session.isAuthenticated, false)
  assert.equal(expirou, 1)

  fetch.fila.push(resposta(200, SESSAO))
  await session.checkSession()
  fetch.fila.push(resposta(400, { error: 'regra recusada', code: 'name_taken' }))
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), { status: 400, message: 'Host error 400', code: 'name_taken' })
})

test('host.call com rede caída recusa com status 0', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  fetch.fila.push(new Error('sem rede'))
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), (e: { status: number }) => e.status === 0)
})

/** Resposta cujos headers chegaram mas o corpo não pode ser lido (timeout após os headers). */
function respostaCorpoPreso(status: number): Response {
  return { status, text: () => Promise.reject(new Error('corpo perdido')) } as unknown as Response
}

test('host.call com 401 derruba a sessão mesmo quando o corpo não pode ser lido', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  let expirou = 0
  session.onExpired(() => { expirou += 1 })
  fetch.fila.push(respostaCorpoPreso(401))
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), { status: 401, message: 'no web session' })
  assert.equal(session.isAuthenticated, false)
  assert.equal(expirou, 1)
})

test('host.call com corpo ilegível fora do 401 recusa com status 0 sem tocar a sessão', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  let expirou = 0
  session.onExpired(() => { expirou += 1 })
  fetch.fila.push(respostaCorpoPreso(500))
  await assert.rejects(host.call(ORIGIN, '', 'bots_list', {}, 1000), (e: { status: number }) => e.status === 0)
  assert.equal(session.isAuthenticated, true)
  assert.equal(expirou, 0)
})

/** Resposta SSE com os trechos dados; `aberta` deixa o corpo aberto e o `sinal` é honrado (como o fetch de verdade). */
function respostaSSE(trechos: string[], aberta = false, sinal?: AbortSignal): Response {
  const codificar = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      if (sinal) sinal.addEventListener('abort', () => { try { controller.error(new Error('aborted')) } catch { /* já fechado */ } }, { once: true })
      for (const trecho of trechos) {
        controller.enqueue(codificar.encode(trecho))
        await new Promise((esperar) => setTimeout(esperar, 5))
      }
      if (!aberta) controller.close()
    },
  })
  return new Response(stream, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })
}

test('host.stream recusa URL de outra origem sem falar com o Hub', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  const fim = new Promise<{ status: number }>((resolve) => host.stream('http://outro.lugar/events', '', () => {}, (e) => resolve({ status: e!.status })))
  assert.equal((await fim).status, 0)
  // Só a leitura de sessão falou com o Hub; o stream de outra origem nem saiu do navegador.
  assert.equal(fetch.chamadas.length, 1)
})

test('host.stream sem sessão recusa e dispara expiração', async () => {
  const falso = fetchFalso()
  const session = new LocalWebSession(ORIGIN, { fetchImpl: falso.impl })
  const host = createLocalWebHost(session)
  let expirou = 0
  session.onExpired(() => { expirou += 1 })
  const fim = new Promise<{ status: number }>((resolve) => host.stream(`${ORIGIN}/events`, '', () => {}, (e) => resolve({ status: e!.status })))
  assert.equal((await fim).status, 401)
  assert.equal(falso.chamadas.length, 0)
  assert.equal(expirou, 0)
})

test('host.stream lê linhas data: e termina limpo, com CSRF e Accept', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  fetch.fila.push(respostaSSE(['data: {"tipo":"hello"}\n\n', 'data: ping\r\n', 'evento: x\ndata: ultimo\n\n']))
  const linhas: string[] = []
  const fim = new Promise<{ erro: unknown }>((resolve) => host.stream(`${ORIGIN}/events?desde=1`, '', (l) => linhas.push(l), (e) => resolve({ erro: e })))
  assert.equal((await fim).erro, null)
  assert.deepEqual(linhas, ['{"tipo":"hello"}', 'ping', 'ultimo'])
  const init = fetch.chamadas[1]!.init!
  assert.equal(init.credentials, 'same-origin')
  assert.equal(init.cache, 'no-store')
  const headers = new Headers(init.headers as HeadersInit)
  assert.equal(headers.get('Accept'), 'text/event-stream')
  assert.equal(headers.get('X-Mexerica-Csrf'), 'csrf-1')
})

test('host.stream com 401 derruba a sessão', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  fetch.fila.push(resposta(401, { authenticated: false }))
  const fim = new Promise<{ status: number }>((resolve) => host.stream(`${ORIGIN}/events`, '', () => {}, (e) => resolve({ status: e!.status })))
  assert.equal((await fim).status, 401)
  assert.equal(session.isAuthenticated, false)
})

test('host.stream encerra com erro quando o Hub silencia além do timeout ocioso', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session, { timeoutOciosoMs: 60 })
  fetch.fila.push((init) => respostaSSE([], true, init.signal ?? undefined))
  const fim = new Promise<{ status: number }>((resolve) => host.stream(`${ORIGIN}/events`, '', () => {}, (e) => resolve({ status: e!.status })))
  assert.equal((await fim).status, 0)
})

test('host.stream cancelado encerra com erro, sem tocar a sessão', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session, { timeoutOciosoMs: 5_000 })
  fetch.fila.push((init) => respostaSSE([], true, init.signal ?? undefined))
  let fim: { status: number } | undefined
  let cancelar: (() => void) | undefined
  const promessa = new Promise<void>((resolve) => {
    cancelar = host.stream(`${ORIGIN}/events`, '', () => {}, (e) => { fim = { status: e!.status }; resolve() })
  })
  await new Promise((esperar) => setTimeout(esperar, 20))
  cancelar!()
  await promessa
  assert.equal(fim!.status, 0)
  assert.equal(session.isAuthenticated, true)
})

const FIO = {
  threadId: 'fio-fixture', subject: 'Assunto fictício', truncated: false,
  messages: [
    { messageId: 'mensagem-a', fromDisplay: 'Remetente A', fromDomain: 'a.example.invalid', sentAt: '2026-01-01T00:00:00Z', bodyText: 'Primeiro corpo fictício' },
    { messageId: 'mensagem-b', fromDisplay: 'Remetente B', fromDomain: 'b.example.invalid', sentAt: '2026-01-02T00:00:00Z', bodyText: 'Segundo corpo fictício' },
  ],
}
const VAZIA = { spaceId: 'workspace-vazio', mailboxId: 'caixa-vazia' }
const COM_FIO = { spaceId: 'workspace-com-fio', mailboxId: 'caixa-com-fio', threadId: FIO.threadId }
const SEM_SESSAO = { status: 401, message: 'no web session' }

function pendente<T>() {
  let resolve!: (valor: T) => void
  const promise = new Promise<T>((resolver) => { resolve = resolver })
  return { promise, resolve }
}

test('a ponte retoma do cursor do chamador após a queda no meio de uma resposta', { timeout: 2000 }, async () => {
  const origin = 'https://hub.example.invalid'
  const { session, fetch: falso } = await sessaoAutenticada(origin)
  const { host } = createWebBridge({ host: createLocalWebHost(session) })
  const historico = ['Olá', 'Olá, mundo', 'Olá, mundo!'].map((text, i) => ({
    type: 'entry', rev: 41 + i,
    entry: {
      id: 'resposta-fixture', botId: 'bot-fixture', seq: 1, rev: 41 + i, kind: 'agent', turn: 1,
      data: { text }, createdAt: 1000, updatedAt: 1000 + i,
    } satisfies Entry,
  }))
  const quadro = (evento: typeof historico[number]) => `data: ${JSON.stringify(evento)}\n\n`
  const trechos = [quadro(historico[0]!), quadro(historico[1]!).slice(0, -4)]
  const encoder = new TextEncoder()
  falso.fila.push(new Response(new ReadableStream<Uint8Array>({
    pull(controller) {
      const trecho = trechos.shift()
      if (trecho === undefined) controller.error(new TypeError('connection lost'))
      else controller.enqueue(encoder.encode(trecho))
    },
  }, { highWaterMark: 0 }), { headers: { 'Content-Type': 'text/event-stream' } }))
  falso.fila.push(() => {
    const url = new URL(falso.chamadas.at(-1)!.url)
    const since = Number(url.searchParams.get('since'))
    return respostaSSE(historico.filter((evento) => evento.rev > since).map(quadro))
  })

  let cursor = 40
  const recebidos: typeof historico = []
  const abrir = () => new Promise<unknown>((resolve, reject) => {
    const query = new URLSearchParams({ since: String(cursor), client: 'desktop', spaceId: 'space:fixture', workspaceEpoch: '7' })
    host.stream(`${origin}/events?${query}`, '', (linha) => {
      try {
        const evento: typeof historico[number] = JSON.parse(linha)
        recebidos.push(evento)
        cursor = evento.entry.rev
      } catch (erro) { reject(erro) }
    }, resolve)
  })
  assert.deepEqual(await abrir(), { status: 0, message: 'local hub unreachable' })
  assert.deepEqual(recebidos, historico.slice(0, 1))
  assert.equal(falso.chamadas.length, 2)
  assert.equal(await abrir(), null)
  assert.deepEqual(recebidos, historico)
  assert.deepEqual(falso.chamadas.slice(1).map(({ url }) => url), [
    `${origin}/events?since=40&client=desktop&spaceId=space%3Afixture&workspaceEpoch=7`,
    `${origin}/events?since=41&client=desktop&spaceId=space%3Afixture&workspaceEpoch=7`,
  ])
})

test('a ponte não repete um POST de resultado incerto ao abrir o SSE', { timeout: 2000 }, async () => {
  const origin = 'https://hub.example.invalid'
  const { session, fetch } = await sessaoAutenticada(origin)
  const { host } = createWebBridge({ host: createLocalWebHost(session) })
  fetch.fila.push(
    new Response(new ReadableStream({
      pull(controller) { controller.error(new TypeError('connection lost')) },
    })),
    respostaSSE(['data: {"type":"hello","rev":43}\n\n']),
  )

  const body = {
    botId: 'bot-fixture', clientNonce: 'pedido-fixture', text: 'Confira https://action.example.invalid/',
    attachments: null, spaceId: 'space:fixture', workspaceEpoch: 7,
  }
  await assert.rejects(host.call(origin, '', 'send', body, 1000), { status: 0 })
  assert.equal(session.isAuthenticated, true)
  const linhas: string[] = []
  const fim = await new Promise<unknown>((resolve) => host.stream('/events?since=42', '', (l) => linhas.push(l), resolve))
  assert.equal(fim, null)
  assert.deepEqual(linhas, ['{"type":"hello","rev":43}'])
  assert.deepEqual(fetch.chamadas.filter(({ init }) => init?.method === 'POST').map(({ url, init }) => [url, init?.body]), [
    [`${origin}/api/send`, JSON.stringify(body)],
  ])
})

test('bootstrap sem epoch funciona no POST e no SSE; terminais chegam sem reconexão da ponte', { timeout: 2000 }, async () => {
  const origin = 'https://hub.example.invalid'
  const { session, fetch } = await sessaoAutenticada(origin)
  const { host } = createWebBridge({ host: createLocalWebHost(session) })
  const workspace = { workspaceId: 'personal:unbound', workspaceEpoch: 7 }
  fetch.fila.push(resposta(200, workspace))
  assert.deepEqual(await host.call(origin, '', 'workspaceContext', {}, 1000), workspace)
  assert.equal(fetch.chamadas[1]?.init?.body, '{}')

  for (const type of ['workspace_revoked', 'role_changed']) {
    const hello = { type: 'hello', rev: 43, workspace }
    const terminal = { type, workspaceId: workspace.workspaceId, workspaceEpoch: 8 }
    const esperados = [hello, terminal].map((evento) => JSON.stringify(evento))
    fetch.fila.push(respostaSSE(esperados.map((linha) => `data: ${linha}\n\n`)))
    const linhas: string[] = []
    const fim = await new Promise<unknown>((resolve) => host.stream('/events?since=43', '', (l) => linhas.push(l), resolve))
    assert.equal(fim, null)
    assert.deepEqual(linhas, esperados)
  }
  assert.deepEqual(fetch.chamadas.slice(1).map(({ url }) => url), [
    `${origin}/api/workspaceContext`, `${origin}/events?since=43`, `${origin}/events?since=43`,
  ])
  for (const { init } of fetch.chamadas) assert.equal(new Headers(init?.headers).has('workspaceEpoch'), false)
  assert.equal(session.isAuthenticated, true)
})

test('409 de workspace chega com código estável no POST e no SSE, sem vazar texto nem repetir', { timeout: 2000 }, async () => {
  const origin = 'https://hub.example.invalid'
  const { session, fetch } = await sessaoAutenticada(origin)
  const { host } = createWebBridge({ host: createLocalWebHost(session) })
  const esperado = { status: 409, message: 'Host error 409', code: 'workspace_epoch_mismatch' }
  const erro = { code: esperado.code, error: 'mensagem bruta de erro' }
  fetch.fila.push(resposta(409, erro), resposta(409, erro), resposta(409, { ...erro, code: 'codigo_desconhecido' }))

  await assert.rejects(host.call(origin, '', 'send', { workspaceEpoch: 6 }, 1000), esperado)
  const abrir = () => new Promise<unknown>((resolve, reject) => host.stream('/events?since=43&workspaceEpoch=6', '', reject, resolve))
  assert.deepEqual(await abrir(), esperado)
  assert.deepEqual(await abrir(), { status: 409, message: 'Host error 409' })
  assert.deepEqual(fetch.chamadas.slice(1).map(({ url }) => url), [
    `${origin}/api/send`, `${origin}/events?since=43&workspaceEpoch=6`, `${origin}/events?since=43&workspaceEpoch=6`,
  ])
  for (const { init } of fetch.chamadas) assert.equal(new Headers(init?.headers).has('workspaceEpoch'), false)
  assert.equal(session.isAuthenticated, true)
})

for (const origem401 of ['RPC', 'SSE', 'checkSession']) {
  test(`401 em ${origem401} aborta RPC e SSE em voo e descarta respostas atrasadas`, { timeout: 2000 }, async () => {
    const { session, fetch } = await sessaoAutenticada()
    const host = createLocalWebHost(session)
    const headersPendentes = pendente<Response>()
    const corpoPendente = pendente<string>()
    const lendoCorpo = pendente<void>()
    const sseAberto = pendente<ReadableStreamDefaultController<Uint8Array>>()
    const lendoSse = pendente<void>()
    const fimSse = pendente<unknown>()
    const linhas: string[] = []
    const encoder = new TextEncoder()
    const cancelarCorpo = pendente<void>()
    fetch.fila.push(
      () => headersPendentes.promise,
      Object.assign(resposta(200), { text: () => { lendoCorpo.resolve(); return corpoPendente.promise } }),
      new Response(new ReadableStream<Uint8Array>({ start: sseAberto.resolve, cancel: () => cancelarCorpo.resolve() })),
      resposta(401, FIO),
    )
    const aberto = host.call(ORIGIN, '', 'mailInboxOpenThread', COM_FIO, 5000)
    const lista = host.call(ORIGIN, '', 'mailInboxListThreads', VAZIA, 5000)
    const cancelados = Promise.all([assert.rejects(aberto, SEM_SESSAO), assert.rejects(lista, SEM_SESSAO)])
    host.stream(`${ORIGIN}/events`, '', (linha) => { linhas.push(linha); lendoSse.resolve() }, fimSse.resolve)
    const stream = await sseAberto.promise
    stream.enqueue(encoder.encode('data: hello\n\n'))
    await Promise.all([lendoCorpo.promise, lendoSse.promise])
    const negado = origem401 === 'RPC'
      ? assert.rejects(host.call(ORIGIN, '', 'mailInboxStatus', COM_FIO, 5000), SEM_SESSAO)
      : origem401 === 'SSE'
        ? new Promise<void>((resolve, reject) => host.stream(`${ORIGIN}/events`, '', reject, (erro) => {
          try { assert.deepEqual(erro, SEM_SESSAO); resolve() } catch (e) { reject(e) }
        }))
        : session.checkSession().then((ativa) => assert.equal(ativa, false))
    await negado

    for (const { init } of fetch.chamadas.slice(1, 4)) {
      assert.equal(init?.signal?.aborted, true)
      assert.equal(init?.cache, 'no-store')
    }
    assert.equal(session.isAuthenticated, false)
    assert.deepEqual(await fimSse.promise, SEM_SESSAO)
    await cancelarCorpo.promise
    const total = fetch.chamadas.length
    await assert.rejects(host.call(ORIGIN, '', 'mailInboxStatus', VAZIA, 5000), SEM_SESSAO)
    assert.equal(fetch.chamadas.length, total)

    fetch.fila.push(resposta(200, SESSAO), resposta(200, { threads: [], nextCursor: null }))
    assert.equal(await session.checkSession(), true)
    headersPendentes.resolve(resposta(200, FIO))
    corpoPendente.resolve(JSON.stringify({ threads: [], nextCursor: null }))
    await cancelados
    assert.deepEqual(linhas, ['hello'])
    assert.deepEqual(await host.call(ORIGIN, '', 'mailInboxListThreads', VAZIA, 5000), { threads: [], nextCursor: null })
  })
}

test('401 e SSE atrasados da geração antiga não afetam a sessão restabelecida', { timeout: 2000 }, async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  const rpc = pendente<Response>()
  const sse = pendente<Response>()
  const fim = pendente<unknown>()
  const linhas: string[] = []
  fetch.fila.push(() => rpc.promise, () => sse.promise, resposta(401))
  const erro = assert.rejects(host.call(ORIGIN, '', 'mailInboxOpenThread', COM_FIO, 1000), SEM_SESSAO)
  host.stream(`${ORIGIN}/events`, '', (l) => linhas.push(l), fim.resolve)
  assert.equal(await session.checkSession(), false)
  fetch.fila.push(resposta(200, { ...SESSAO, csrfToken: 'csrf-2' }), new Response('data: evento-novo\n\n'))
  assert.equal(await session.checkSession(), true)
  const fimNovo = await new Promise<unknown>((resolve) => host.stream(`${ORIGIN}/events`, '', (l) => linhas.push(l), resolve))
  assert.equal(fimNovo, null)
  rpc.resolve(resposta(401, FIO))
  sse.resolve(new Response('data: evento-atrasado\n\n'))
  await erro
  assert.deepEqual(await fim.promise, SEM_SESSAO)
  assert.equal(session.isAuthenticated, true)
  assert.deepEqual(linhas, ['evento-novo'])
})

test('ponte local não grava respostas nem registra conteúdo e devolve apenas erros sanitizados', async (t) => {
  const persistencia: string[] = []
  for (const nome of ['caches', 'indexedDB', 'localStorage', 'sessionStorage', 'navigator'] as const) {
    const anterior = Object.getOwnPropertyDescriptor(globalThis, nome)
    Object.defineProperty(globalThis, nome, { configurable: true, get: () => { persistencia.push(nome); return undefined } })
    t.after(() => { if (anterior) Object.defineProperty(globalThis, nome, anterior); else Reflect.deleteProperty(globalThis, nome) })
  }
  const logs = (['log', 'info', 'warn', 'error', 'debug', 'trace'] as const).map((nivel) => t.mock.method(console, nivel, () => {}))
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  const call = () => host.call(ORIGIN, '', 'mailInboxOpenThread', COM_FIO, 1000)
  const bruto = JSON.stringify(FIO)
  fetch.fila.push(resposta(200, FIO), resposta(403, { error: bruto, code: FIO.threadId }), new Response(`invalido ${bruto}`), new Error(bruto))
  assert.deepEqual(await call(), FIO)
  await assert.rejects(call(), { status: 403, message: 'Host error 403' })
  await assert.rejects(call(), { status: 0, message: 'local hub unreachable' })
  await assert.rejects(call(), { status: 0, message: 'local hub unreachable' })
  fetch.fila.push(resposta(500, FIO), new Error(bruto))
  const fim = () => new Promise<unknown>((resolve) => host.stream(`${ORIGIN}/events`, '', assert.fail, resolve))
  assert.deepEqual(await fim(), { status: 500, message: 'Host error 500' })
  assert.deepEqual(await fim(), { status: 0, message: 'local hub unreachable' })
  assert.deepEqual(persistencia, [])
  for (const log of logs) assert.equal(log.mock.callCount(), 0)
})

test('preserva apenas recusas de skills conhecidas no envelope legado', async () => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  fetch.fila.push(resposta(409, { error: 'skill_installation_stale' }), resposta(409, { error: `skill_installation_stale: ${FIO.subject}` }))
  const call = () => host.call(ORIGIN, '', 'updateBot', {}, 1000)
  await assert.rejects(call(), { status: 409, message: 'skill_installation_stale', code: 'skill_installation_stale' })
  await assert.rejects(call(), (erro: unknown) => {
    assert.deepEqual(erro, { status: 409, message: 'Host error 409' })
    return true
  })
})

test('RPC e SSE funcionam sem os combinadores modernos de AbortSignal', async (t) => {
  const { session, fetch } = await sessaoAutenticada()
  const host = createLocalWebHost(session)
  t.mock.method(AbortSignal, 'any', () => assert.fail('AbortSignal.any não está disponível'))
  t.mock.method(AbortSignal, 'timeout', () => assert.fail('AbortSignal.timeout não está disponível'))
  fetch.fila.push(resposta(200, { ok: true }), respostaSSE(['data: hello\n\n']))
  assert.deepEqual(await host.call(ORIGIN, '', 'hello', {}, 1000), { ok: true })
  const linhas: string[] = []
  const fim = await new Promise<unknown>((resolve) => host.stream(`${ORIGIN}/events`, '', (linha) => linhas.push(linha), resolve))
  assert.equal(fim, null)
  assert.deepEqual(linhas, ['hello'])
})
