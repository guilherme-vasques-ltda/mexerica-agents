import assert from 'node:assert/strict'
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
import { once } from 'node:events'
import { mkdtempSync, rmSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { deferred, loadMainModule } from './test-support/main-module.ts'

/**
 * A conexão provada (fork-proved-loopback): a prova do token e a credencial passam pela MESMA
 * conexão TCP. Um listener que ocupou o lugar do Hub entre as duas nunca lê a credencial,
 * porque um pedido numa conexão que não é a provada é destruído antes de qualquer byte sair.
 */
const TOKEN = 'token-local-ficticio'
const PROVA = 'mexerica/health-proof/v1'
const RECIBO = 'mexerica/health-receipt/v1'
const IDENTIDADE = { hostId: 'host-ficticio', computerId: 'computador-ficticio', binaryPath: '/bin/host-ficticio', binaryHash: 'a'.repeat(64) }

const hmac = (key: string, rotulo: string, nonce: string, extra?: string) => {
  const h = createHmac('sha256', key).update(rotulo).update(Buffer.from(nonce, 'base64url'))
  return (extra === undefined ? h : h.update(extra)).digest('hex')
}

const campos = () => [IDENTIDADE.hostId, IDENTIDADE.computerId, IDENTIDADE.binaryPath, IDENTIDADE.binaryHash].join('\n')

/** Hub de mentira que registra cada requisição com a conexão que a atendeu. */
function hubOf(token: string, options: { mataAposProva?: boolean; recusaProva?: boolean; corpoAtrasado?: Promise<void>; corpoPingando?: boolean; pausarProva?: () => void } = {}) {
  const seen: { url: string; authorization: string | undefined; socketId: number }[] = []
  const sockets = new Map<number, import('node:net').Socket>()
  let proximo = 1
  const servidores = new Set<Server>()
  const server = createServer((request, response) => {
    const socketId = [...sockets.entries()].find(([, s]) => s === request.socket)?.[0] ?? 0
    seen.push({ url: request.url ?? '', authorization: request.headers.authorization, socketId })
    if (request.url?.startsWith('/api/')) {
      let body = ''
      request.on('data', (parte) => (body += parte))
      request.on('end', () => {
        response.writeHead(200, { 'content-type': 'application/json' })
        if (options.corpoPingando) {
          response.write(' ')
          const timer = setInterval(() => response.write(' '), 20)
          response.once('close', () => clearInterval(timer))
        } else if (options.corpoAtrasado) {
          response.write('{"ok":')
          void options.corpoAtrasado.then(() => response.end('true}'))
        } else {
          response.end(JSON.stringify({ ok: true }))
        }
      })
      return
    }
    if (request.url?.startsWith('/stream')) {
      response.writeHead(200, { 'content-type': 'text/event-stream' })
      response.write('data: {}\n\n')
      return
    }
    const query = new URL(request.url ?? '/', 'http://127.0.0.1').searchParams
    const nonce = query.get('nonce') ?? ''
    const proved = query.get('proof') === hmac(token, PROVA, nonce) && Buffer.from(nonce, 'base64url').length === 32
    const anonymous = { ok: true, version: '0.0.0-test', environment: 'main', busy: false }
    const body = proved ? { ...anonymous, ...IDENTIDADE, receipt: hmac(token, RECIBO, nonce, campos()) } : anonymous
    const recusa = query.has('proof') && (!proved || options.recusaProva)
    if (options.pausarProva) {
      options.pausarProva()
      return
    }
    response.writeHead(recusa ? 401 : 200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(recusa ? { error: 'invalid token' } : body))
    // Mata a conexão depois da resposta da prova: o pedido com a credencial vai nascer numa
    // conexão NOVA e a guarda tem que destruí-lo antes de qualquer byte sair.
    if (proved && options.mataAposProva) response.socket?.destroy()
  })
  server.on('connection', (socket) => {
    const id = proximo++
    sockets.set(id, socket)
    socket.on('close', () => sockets.delete(id))
  })
  return {
    seen,
    listen: async (host = '127.0.0.1') => {
      await once(server.listen(0, host), 'listening')
      servidores.add(server)
      const address = server.address()
      assert.ok(address && typeof address === 'object')
      return `http://${host === '::1' ? '[::1]' : host}:${address.port}`
    },
    listenUnix: async () => {
      const dir = mkdtempSync(join(tmpdir(), 'mx-test-'))
      const socketPath = join(dir, 's')
      try {
        await once(server.listen(socketPath), 'listening')
        servidores.add(server)
      } finally {
        server.once('close', () => rmSync(dir, { recursive: true, force: true }))
      }
      return { socketPath }
    },
    close: () => Promise.all([...servidores].map((open) => new Promise<void>((resolve) => open.close(() => resolve())))),
  }
}

const load = () =>
  loadMainModule<typeof import('../main/fork-proved-loopback.ts')>('fork-proved-loopback', {
    'node:crypto': { createHmac, randomBytes, timingSafeEqual },
  }, { Buffer, AbortSignal, URL })

test('Unix socket: the request carries the bearer only on the connection that proved it', async (t) => {
  if (process.platform === 'win32') return t.skip('Unix socket required')
  const { postProvado, provarSocket, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN)
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  const target = await hub.listenUnix()
  assert.equal((await provarSocket(target.socketPath, TOKEN, AbortSignal.timeout(2_000))).computerId, IDENTIDADE.computerId)
  const response = await postProvado(target, '/api/snapshot', TOKEN, '{}', 2_000)
  const end = once(response, 'end')
  response.resume()
  await end
  assert.equal(response.statusCode, 200)
  assert.deepEqual(hub.seen.map((entry) => entry.authorization), [undefined, undefined, `Bearer ${TOKEN}`])
  assert.equal(hub.seen[1]?.socketId, hub.seen[2]?.socketId)
})

test('Unix socket: loss of the proved connection does not send the bearer to a new one', async (t) => {
  if (process.platform === 'win32') return t.skip('Unix socket required')
  const { postProvado, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN, { mataAposProva: true })
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  await assert.rejects(postProvado(await hub.listenUnix(), '/api/snapshot', TOKEN, '{}', 2_000))
  assert.equal(hub.seen.length, 1)
  assert.equal(hub.seen[0]?.authorization, undefined)
})

test('the proof and credential share a connection and the full response survives delayed chunks', async (t) => {
  const { postProvado, fecharProvedLoopback } = load()
  const corpo = deferred()
  const hub = hubOf(TOKEN, { corpoAtrasado: corpo.promise })
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  const url = await hub.listen()
  const res = await postProvado(url, '/api/snapshot', TOKEN, '{}', 5_000)
  assert.equal(res.statusCode, 200)
  corpo.resolve()
  let texto = ''
  res.setEncoding('utf8')
  for await (const parte of res) texto += parte
  assert.deepEqual(JSON.parse(texto), { ok: true })
  const saude = hub.seen.find((call) => call.url.startsWith('/health'))
  const api = hub.seen.find((call) => call.url.startsWith('/api/'))
  assert.ok(saude && api, JSON.stringify(hub.seen))
  assert.equal(saude.authorization, undefined, 'a prova não carrega credencial')
  assert.equal(api.authorization, `Bearer ${TOKEN}`, 'a chamada carrega o bearer')
  assert.equal(api.socketId, saude.socketId, 'prova e credencial na MESMA conexão')
})

test('IPv6 loopback also sends the proof and bearer on the same connection', async (t) => {
  const { postProvado, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN)
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  let url: string
  try {
    url = await hub.listen('::1')
  } catch (erro) {
    if (['EAFNOSUPPORT', 'EADDRNOTAVAIL'].includes((erro as NodeJS.ErrnoException).code ?? '')) {
      return t.skip('IPv6 loopback unavailable')
    }
    throw erro
  }
  const res = await postProvado(url, '/api/snapshot', TOKEN, '{}', 5_000)
  assert.equal(res.statusCode, 200)
  const fim = once(res, 'end')
  res.resume()
  await fim
  assert.equal(hub.seen.length, 2)
  assert.equal(hub.seen[0]?.authorization, undefined)
  assert.equal(hub.seen[1]?.authorization, `Bearer ${TOKEN}`)
  assert.equal(hub.seen[0]?.socketId, hub.seen[1]?.socketId)
})

test('a POST with an active but unfinished body expires at the total deadline', { timeout: 5_000 }, async (t) => {
  const { postProvado, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN, { corpoPingando: true })
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  const res = await postProvado(await hub.listen(), '/api/snapshot', TOKEN, '{}', 700)
  assert.equal(res.statusCode, 200)
  await assert.rejects(async () => { for await (const _ of res) { /* consume */ } }, { code: 'ECONNRESET' })
})

test('a stream cancelled before or during proof never sends the bearer', async (t) => {
  const { streamProvado, fecharProvedLoopback } = load()
  const prova = deferred()
  const hub = hubOf(TOKEN, { pausarProva: prova.resolve })
  t.after(async () => { await fecharProvedLoopback(); await hub.close() })
  const url = await hub.listen()
  await assert.rejects(streamProvado(url, '/stream', TOKEN, AbortSignal.abort(), 5_000), { name: 'AbortError' })
  assert.equal(hub.seen.length, 0)
  const controller = new AbortController()
  const stream = streamProvado(url, '/stream', TOKEN, controller.signal, 5_000)
  await prova.promise
  controller.abort()
  await assert.rejects(stream, { name: 'AbortError' })
  assert.equal(hub.seen.length, 1)
  assert.equal(hub.seen[0]?.authorization, undefined)
})

test('when the proved connection dies, the credential never reaches the port', async () => {
  const { postProvado, ProvaRecusada, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN, { mataAposProva: true })
  const url = await hub.listen()
  await assert.rejects(postProvado(url, '/api/snapshot', TOKEN, '{}', 5_000), ProvaRecusada)
  assert.deepEqual(hub.seen.filter((call) => call.url.startsWith('/api/')), [], 'nenhum byte da credencial chegou ao servidor')
  assert.deepEqual(hub.seen.filter((call) => call.authorization !== undefined), [], 'e nada de bearer')
  await fecharProvedLoopback()
  await hub.close()
})

test('a refused proof never sends the credential at all', async () => {
  const { postProvado, ProvaRecusada, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN, { recusaProva: true })
  const url = await hub.listen()
  await assert.rejects(postProvado(url, '/api/snapshot', TOKEN, '{}', 5_000), ProvaRecusada)
  assert.deepEqual(hub.seen.filter((call) => call.authorization !== undefined), [], 'o bearer nunca saiu')
  await fecharProvedLoopback()
  await hub.close()
})

test('an open stream does not block the queue: later calls open their own proved round', async () => {
  const { postProvado, streamProvado, fecharProvedLoopback } = load()
  const hub = hubOf(TOKEN)
  const url = await hub.listen()
  const controller = new AbortController()
  const stream = await streamProvado(url, '/stream', TOKEN, controller.signal, 5_000)
  assert.equal(stream.statusCode, 200)
  // Com o stream vivo ocupando a fila, a chamada seguinte ainda sai: prova e credencial numa
  // conexão nova, as duas juntas — nunca o bearer atravessando para uma conexão sem prova.
  const res = await postProvado(url, '/api/snapshot', TOKEN, '{}', 5_000)
  assert.equal(res.statusCode, 200)
  res.resume()
  await new Promise((resolve) => res.on('end', resolve))
  const api = hub.seen.find((call) => call.url.startsWith('/api/'))
  assert.ok(api, 'a chamada atrás do stream chegou')
  assert.equal(api!.authorization, `Bearer ${TOKEN}`)
  const provas = hub.seen.filter((call) => new URL(call.url, 'http://127.0.0.1').searchParams.has('proof'))
  const antes = provas.filter((call) => call.socketId === api!.socketId)
  assert.ok(antes.length > 0, 'a credencial saiu numa conexão que a prova acabou de validar')
  controller.abort()
  stream.destroy()
  await fecharProvedLoopback()
  await hub.close()
})
