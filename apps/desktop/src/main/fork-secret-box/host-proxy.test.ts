import assert from 'node:assert/strict'
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
import test from 'node:test'
import type { WebContents } from 'electron'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

/** Espera de verdade: o Hub de mentira é I/O real, microtarefas não bastam. */
const esperar = async (condicao: () => boolean, ms = 5_000) => {
  const limite = Date.now() + ms
  while (!condicao() && Date.now() < limite) await new Promise((resolve) => setTimeout(resolve, 10))
}
import { hubProva, TOKEN_PROVA } from '../../shared/test-support/hub-prova.ts'
import * as contexts from './contexts.ts'

/**
 * O proxy do Hub pelo processo principal, agora sobre a conexão provada de verdade: os testes
 * sobem um Hub de mentira no loopback (hub-prova) e o transporte real — prova e credencial na
 * mesma conexão — é o que liga o handler à resposta.
 */

function fixture() {
  type Handler = (event: { sender: WebContents }, ...args: any[]) => any
  const handlers = new Map<string, Handler>()
  const sent: unknown[][] = []
  const sender = { isDestroyed: () => false, send: (...args: unknown[]) => sent.push(args) } as unknown as WebContents
  const proxy = loadMainModule<typeof import('../host-proxy.ts')>('host-proxy', {
    electron: { ipcMain: {
      handle: (channel: string, handler: Handler) => handlers.set(channel, handler),
      on: (channel: string, handler: Handler) => handlers.set(channel, handler),
    } },
    './analytics': { noteAnalyticsSetting: () => {} },
    './fork-secret-box/contexts': contexts,
    'node:crypto': { createHmac, randomBytes, timingSafeEqual },
  }, { URL, AbortController, setTimeout, clearTimeout, TextDecoder, Buffer })
  proxy.registerHostProxy()
  return {
    sender, sent,
    invoke: (channel: string, ...args: unknown[]) => handlers.get(channel)!({ sender }, ...args),
    invokeFrom: (sender: WebContents, channel: string, ...args: unknown[]) => handlers.get(channel)!({ sender }, ...args),
  }
}

test('handlers reais recusam privado antes de leitura, fetch e eco', async () => {
  const f = fixture()
  contexts.markPrivateContext(f.sender)
  const poison = new Proxy({}, { get: () => { throw new Error('argumento lido') } })
  const result = await f.invoke('host:call', poison, poison, poison, poison, poison)
  assert.equal(JSON.stringify(result), JSON.stringify({ ok: false, error: { status: 0, message: 'Contexto indisponível.' } }))
  f.invoke('host:stream:open', poison, poison, poison)
  f.invoke('host:stream:close', poison)
  assert.deepEqual(f.sent, [])
})

test('privado não substitui/fecha stream geral; fluxo geral preserva call e close', async () => {
  let aberto = 0
  let fechado = 0
  const hub = await hubProva((request, response) => {
    if (request.url === '/api/ping') {
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ value: 'fixture' }))
      return true
    }
    if (request.url === '/events') {
      aberto += 1
      response.writeHead(200, { 'content-type': 'text/event-stream' })
      // Como o Hub de verdade, o stream já escreve algo ao abrir (o keep-alive): sem o primeiro
      // byte, o cliente não recebe nem os cabeçalhos.
      response.write(': open\n\n')
      response.socket!.on('close', () => { fechado += 1 })
      return true
    }
    return false
  })
  const f = fixture()
  const result = await f.invoke('host:call', hub.url(), TOKEN_PROVA, 'ping', {}, 1000)
  assert.equal(result.ok, true)
  assert.equal(result.value.value, 'fixture')
  f.invoke('host:stream:open', 'shared-id', `${hub.url()}/events`, TOKEN_PROVA)
  await esperar(() => aberto === 1)
  assert.equal(aberto, 1, 'o stream abriu no servidor')
  const privateSender = { isDestroyed: () => false, send: () => assert.fail('eco privado') } as unknown as WebContents
  contexts.markPrivateContext(privateSender)
  f.invokeFrom(privateSender, 'host:stream:open', 'shared-id', `${hub.url()}/events`, TOKEN_PROVA)
  f.invokeFrom(privateSender, 'host:stream:close', 'shared-id')
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.equal(fechado, 0, 'o close privado não derrubou o stream geral')
  f.invoke('host:stream:close', 'shared-id')
  await esperar(() => fechado === 1)
  assert.equal(fechado, 1, 'o close geral derrubou o stream')
  assert.deepEqual(hub.violacoes, [], 'nenhuma credencial atravessou sem prova')
  assert.deepEqual(f.sent, [])
  await hub.close()
})

test('stream geral entrega data e end pelo handler real', async () => {
  const hub = await hubProva((request, response) => {
    if (request.url === '/events') {
      response.writeHead(200, { 'content-type': 'text/event-stream' })
      response.end('data: fixture\n')
      return true
    }
    return false
  })
  const f = fixture()
  f.invoke('host:stream:open', 'general-id', `${hub.url()}/events`, TOKEN_PROVA)
  await esperar(() => f.sent.some((m) => m[0] === 'host:stream:end'))
  assert.equal(JSON.stringify(f.sent), JSON.stringify([
    ['host:stream:data', 'general-id', ['fixture']],
    ['host:stream:end', 'general-id', null],
  ]))
  assert.deepEqual(hub.violacoes, [])
  await hub.close()
})

const ateOFim = async (f: ReturnType<typeof fixture>) => {
  await esperar(() => f.sent.some((m) => m[0] === 'host:stream:end'))
}
const lotes = (f: ReturnType<typeof fixture>) => f.sent.filter((m) => m[0] === 'host:stream:data').map((m) => m[2])

test('stream em lote: ordem preservada e nenhum evento perdido entre trechos', async () => {
  const hub = await hubProva((request, response) => {
    if (request.url === '/events') {
      response.writeHead(200, { 'content-type': 'text/event-stream' })
      // Os trechos saem com um intervalo entre eles: cada um é um trecho do stream por si, que é
      // o que o teste de lote mede — a ordem e a integridade, não o acaso da junção no TCP.
      response.write('data: a1\ndata: a2\n: ping\nevent: tick\n\n')
      setTimeout(() => {
        response.write('data: b')
        setTimeout(() => {
          response.write('1\n')
          setTimeout(() => response.end('data: c1\r\ndata: c2\n'), 20)
        }, 20)
      }, 20)
      return true
    }
    return false
  })
  const f = fixture()
  f.invoke('host:stream:open', 'batch-id', `${hub.url()}/events`, TOKEN_PROVA)
  await ateOFim(f)
  assert.equal(JSON.stringify(lotes(f)), JSON.stringify([['a1', 'a2'], ['b1'], ['c1', 'c2']]))
  assert.deepEqual(f.sent.at(-1), ['host:stream:end', 'batch-id', null])
  assert.deepEqual(hub.violacoes, [])
  await hub.close()
})

test('stream: lote já lido chega antes do end mesmo com erro no trecho seguinte', async () => {
  const hub = await hubProva((request, response) => {
    if (request.url === '/events') {
      response.writeHead(200, { 'content-type': 'text/event-stream' })
      response.write('data: x1\ndata: x2\n')
      setTimeout(() => response.socket?.destroy(), 10)
      return true
    }
    return false
  })
  const f = fixture()
  f.invoke('host:stream:open', 'err-id', `${hub.url()}/events`, TOKEN_PROVA)
  await ateOFim(f)
  assert.equal(JSON.stringify(lotes(f)), JSON.stringify([['x1', 'x2']]))
  assert.equal(f.sent.at(-1)?.[0], 'host:stream:end')
  await hub.close()
})

test('erro name_taken do HTTP atravessa o handler real de host:call', async () => {
  const hub = await hubProva((request, response) => {
    if (request.url === '/api/botGroupsEdit') {
      response.writeHead(409, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ error: 'Nome já usado', code: 'name_taken' }))
      return true
    }
    return false
  })
  const f = fixture()
  const result = await f.invoke('host:call', hub.url(), TOKEN_PROVA, 'botGroupsEdit', { op: 'add', name: 'Grupo' }, 1000)
  assert.equal(JSON.stringify(result), JSON.stringify({ ok: false, error: { status: 409, message: 'Nome já usado', code: 'name_taken' } }))
  assert.deepEqual(hub.violacoes, [])
  await hub.close()
})
