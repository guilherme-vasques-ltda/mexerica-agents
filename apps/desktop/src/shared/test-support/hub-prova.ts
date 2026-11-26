import assert from 'node:assert/strict'
import { createHmac } from 'node:crypto'
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { Socket } from 'node:net'

/**
 * Hub de mentira para testes da conexão provada: o `/health` responde como o Hub de verdade
 * (anônimo sem prova, identidade e recibo com a prova do token) e qualquer outra rota exige o
 * bearer — um teste que mande credencial sem prova válida vê o servidor recusar, e as rotas que
 * receberam bearer sem uma prova anterior na mesma conexão ficam registradas como violação.
 */
export const TOKEN_PROVA = 'token-local-ficticio'
const PROVA = 'mexerica/health-proof/v1'
const RECIBO = 'mexerica/health-receipt/v1'
const IDENTIDADE = { hostId: 'host-ficticio', computerId: 'computador-ficticio', binaryPath: '/bin/host-ficticio', binaryHash: 'a'.repeat(64) }

const hmac = (key: string, rotulo: string, nonce: string, extra?: string) => {
  const h = createHmac('sha256', key).update(rotulo).update(Buffer.from(nonce, 'base64url'))
  return (extra === undefined ? h : h.update(extra)).digest('hex')
}

const campos = () => [IDENTIDADE.hostId, IDENTIDADE.computerId, IDENTIDADE.binaryPath, IDENTIDADE.binaryHash].join('\n')

export type HubProva = {
  url: () => string
  /** Requisições protegidas que chegaram com bearer sem prova válida na mesma conexão antes. */
  violacoes: { url: string }[]
  /** Todas as requisições vistas, em ordem. */
  seen: { url: string; authorization: string | undefined }[]
  close: () => Promise<void>
}

/**
 * `rotas` decide o que cada caminho protegido responde; o `/health` é tratado aqui. Sockets que
 * passaram pela prova ficam marcados: bearer num socket nunca provado é violação.
 */
export async function hubProva(rotas: (request: IncomingMessage, response: ServerResponse) => boolean): Promise<HubProva> {
  const seen: { url: string; authorization: string | undefined }[] = []
  const violacoes: { url: string }[] = []
  const provados = new WeakSet<Socket>()
  const server = createServer((request, response) => {
    const caminho = request.url ?? ''
    seen.push({ url: caminho, authorization: request.headers.authorization })
    if (caminho.startsWith('/health')) {
      const query = new URL(caminho, 'http://127.0.0.1').searchParams
      const nonce = query.get('nonce') ?? ''
      const proved = query.get('proof') === hmac(TOKEN_PROVA, PROVA, nonce) && Buffer.from(nonce, 'base64url').length === 32
      const anonymous = { ok: true, version: '0.0.0-test', environment: 'main', busy: false }
      const body = proved ? { ...anonymous, ...IDENTIDADE, receipt: hmac(TOKEN_PROVA, RECIBO, nonce, campos()) } : anonymous
      if (proved) provados.add(request.socket)
      response.writeHead(query.has('proof') && !proved ? 401 : 200, { 'content-type': 'application/json' })
      response.end(JSON.stringify(query.has('proof') && !proved ? { error: 'invalid token' } : body))
      return
    }
    if (request.headers.authorization !== `Bearer ${TOKEN_PROVA}` || !provados.has(request.socket)) {
      violacoes.push({ url: caminho })
      response.writeHead(401, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ error: 'invalid token' }))
      return
    }
    if (!rotas(request, response)) {
      response.writeHead(404)
      response.end()
    }
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    seen,
    violacoes,
    url: () => {
      const address = server.address()
      assert.ok(address && typeof address === 'object')
      return `http://127.0.0.1:${address.port}`
    },
    close: async () => {
      server.closeAllConnections()
      await new Promise<void>((resolve) => server.close(() => resolve()))
    },
  }
}
