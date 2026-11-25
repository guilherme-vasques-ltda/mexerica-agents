import assert from 'node:assert/strict'
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto'
import { createServer, type Server } from 'node:http'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

/**
 * O `/health` do Hub: anônimo para quem não tem o token, com a identidade para quem prova o token,
 * e a resposta só vale com o recibo. Aqui servidores de verdade respondem como o Hub.
 */
const TOKEN = 'token-local-ficticio'
const PROVA = 'mexerica/health-proof/v1'
const RECIBO = 'mexerica/health-receipt/v1'
const IDENTIDADE = { hostId: 'host-ficticio', computerId: 'computador-ficticio', binaryPath: '/bin/host-ficticio', binaryHash: 'a'.repeat(64) }

const hmac = (key: string, rotulo: string, nonce: string, extra?: string) => {
  const h = createHmac('sha256', key).update(rotulo).update(Buffer.from(nonce, 'base64url'))
  return (extra === undefined ? h : h.update(extra)).digest('hex')
}

const campos = (identity: Record<string, string>) =>
  [identity.hostId ?? '', identity.computerId ?? '', identity.binaryPath ?? '', identity.binaryHash ?? ''].join('\n')

/** Como o Hub: a prova certa destrava a identidade, e a resposta leva o recibo do token. */
function hubOf(token: string, identity: Record<string, string>) {
  const seen: { url: string; authorization: string | undefined }[] = []
  const servidores = new Set<Server>()
  const server = createServer((request, response) => {
    seen.push({ url: request.url ?? '', authorization: request.headers.authorization })
    const query = new URL(request.url ?? '/', 'http://127.0.0.1').searchParams
    const nonce = query.get('nonce') ?? ''
    const proved = query.get('proof') === hmac(token, PROVA, nonce) && Buffer.from(nonce, 'base64url').length === 32
    const anonymous = { ok: true, version: '0.0.0-test', environment: 'main', busy: false }
    const body = proved
      ? { ...anonymous, ...identity, receipt: hmac(token, RECIBO, nonce, campos(identity)) }
      : anonymous
    response.writeHead(query.has('proof') && !proved ? 401 : 200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(query.has('proof') && !proved ? { error: 'invalid token' } : body))
  })
  return {
    seen,
    listen: async () => {
      await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
      servidores.add(server)
      const address = server.address()
      assert.ok(address && typeof address === 'object')
      return `http://127.0.0.1:${address.port}`
    },
    close: () => Promise.all([...servidores].map((open) => new Promise<void>((resolve) => open.close(() => resolve())))),
  }
}

const load = () =>
  loadMainModule<typeof import('../main/fork-local-health.ts')>(
    'fork-local-health',
    { 'node:crypto': { createHmac, randomBytes, timingSafeEqual } },
    { Buffer, fetch: (url: string, init?: RequestInit) => fetch(url, init), AbortSignal },
  )

test('anonymous health answers without identity and without anything secret on the wire', async () => {
  const hub = hubOf(TOKEN, IDENTIDADE)
  const url = await hub.listen()
  try {
    const { localHealth } = load()
    const health = await localHealth(url, 2000)
    assert.deepEqual(health, { ok: true, version: '0.0.0-test', environment: 'main', busy: false })
    assert.equal(hub.seen.length, 1)
    assert.equal(hub.seen[0]!.url, '/health', 'no query, so no proof')
    assert.equal(hub.seen[0]!.authorization, undefined)
    assert.ok(!hub.seen[0]!.url.includes(TOKEN))
  } finally {
    await hub.close()
  }
})

test('the proof of the local token unlocks the identity, accepted only with the receipt on it', async () => {
  const hub = hubOf(TOKEN, IDENTIDADE)
  const url = await hub.listen()
  try {
    const { localHealth } = load()
    const health = await localHealth(url, 2000, TOKEN)
    assert.deepEqual({ ...health, receipt: undefined }, { ok: true, version: '0.0.0-test', environment: 'main', busy: false, ...IDENTIDADE, receipt: undefined })
    assert.ok(health?.receipt, 'the answer carries the receipt')
    assert.equal(hub.seen[0]!.authorization, undefined, 'the token itself never travels')
    assert.ok(!hub.seen[0]!.url.includes(TOKEN), 'nor appears in the query')
    assert.equal(await localHealth(url, 2000, 'outro-token-ficticio'), null, 'a refused proof reads nothing')
  } finally {
    await hub.close()
  }
})

test('each call carries a fresh nonce', async () => {
  const hub = hubOf(TOKEN, IDENTIDADE)
  const url = await hub.listen()
  try {
    const { localHealth } = load()
    await localHealth(url, 2000, TOKEN)
    await localHealth(url, 2000, TOKEN)
    const nonces = hub.seen.map((call) => new URL(call.url, 'http://127.0.0.1').searchParams.get('nonce'))
    assert.equal(nonces.length, 2)
    assert.notEqual(nonces[0], nonces[1], 'a repeated nonce would let a proof be replayed')
  } finally {
    await hub.close()
  }
})

test('a listener that answers with the public identity but without the receipt is refused', async () => {
  // Um impostor que conhece os campos públicos (os que já circularam) não tem o token para o recibo.
  const impostor = hubOf('outro-token-ficticio', IDENTIDADE)
  const url = await impostor.listen()
  try {
    const { localHealth } = load()
    assert.equal(await localHealth(url, 2000, TOKEN), null, 'a forged identity is not accepted')
  } finally {
    await impostor.close()
  }
  // E o mesmo servidor com o token certo responde: a recusa acima é do recibo, não do endereço.
  const real = hubOf(TOKEN, IDENTIDADE)
  const realURL = await real.listen()
  try {
    const { localHealth } = load()
    assert.ok(await localHealth(realURL, 2000, TOKEN))
  } finally {
    await real.close()
  }
})

test('a receipt bound to other values is refused', async () => {
  const server = createServer((request, response) => {
    const query = new URL(request.url ?? '/', 'http://127.0.0.1').searchParams
    const nonce = query.get('nonce') ?? ''
    // Recibo calculado sobre uma identidade diferente da que o corpo traz.
    const receipt = hmac(TOKEN, RECIBO, nonce, campos({ ...IDENTIDADE, computerId: 'outro-computador' }))
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ ok: true, environment: 'main', busy: false, ...IDENTIDADE, receipt }))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert.ok(address && typeof address === 'object')
  try {
    const { localHealth } = load()
    assert.equal(await localHealth(`http://127.0.0.1:${address.port}`, 2000, TOKEN), null)
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
})

test('a host that answers an error or is down reads as nothing', async () => {
  const { localHealth } = load()
  const server = createServer((_request, response) => {
    response.writeHead(500, { 'content-type': 'application/json' })
    response.end('{"ok":false}')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert.ok(address && typeof address === 'object')
  try {
    assert.equal(await localHealth(`http://127.0.0.1:${address.port}`, 2000, TOKEN), null)
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
  assert.equal(await localHealth('http://127.0.0.1:1', 500), null, 'nothing listening: null, not a throw')
})
