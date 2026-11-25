import assert from 'node:assert/strict'
import { createHmac, randomBytes } from 'node:crypto'
import { createServer, type Server } from 'node:http'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

/**
 * O `/health` do Hub: anônimo para quem não tem o token, com a identidade para quem prova o token.
 * Aqui um servidor de verdade responde, registra cada chamada e confere a prova como o Hub.
 */
const TOKEN = 'token-local-ficticio'
const ROTULO = 'mexerica/health-proof/v1'

const prova = (token: string, nonce: string) => createHmac('sha256', token).update(ROTULO).update(Buffer.from(nonce, 'base64url')).digest('hex')

/** Como o Hub: só a prova certa, com nonce de 32 bytes, destrava a identidade. */
function hubOf(token: string, identity: Record<string, unknown>) {
  const seen: { url: string; authorization: string | undefined }[] = []
  const servers = new Set<Server>()
  const server = createServer((request, response) => {
    seen.push({ url: request.url ?? '', authorization: request.headers.authorization })
    const query = new URL(request.url ?? '/', 'http://127.0.0.1').searchParams
    const nonce = query.get('nonce') ?? ''
    const proved = query.get('proof') === prova(token, nonce) && Buffer.from(nonce, 'base64url').length === 32
    const anonymous = { ok: true, version: '0.0.0-test', environment: 'main', busy: false }
    response.writeHead(query.has('proof') && !proved ? 401 : 200, { 'content-type': 'application/json' })
    response.end(JSON.stringify(query.has('proof') && !proved ? { error: 'invalid token' } : proved ? { ...anonymous, ...identity } : anonymous))
  })
  return {
    seen,
    listen: async () => {
      await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
      servers.add(server)
      const address = server.address()
      assert.ok(address && typeof address === 'object')
      return `http://127.0.0.1:${address.port}`
    },
    close: () => Promise.all([...servers].map((open) => new Promise<void>((resolve) => open.close(() => resolve())))),
  }
}

const load = () => loadMainModule<typeof import('../main/fork-local-health.ts')>('fork-local-health', { 'node:crypto': { createHmac, randomBytes } }, { fetch: (url: string, init?: RequestInit) => fetch(url, init), AbortSignal })

test('anonymous health answers without identity and without anything secret on the wire', async () => {
  const hub = hubOf(TOKEN, { computerId: 'computador-ficticio' })
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

test('the proof of the local token unlocks the identity of the host and of the binary', async () => {
  const identity = { computerId: 'computador-ficticio', binaryPath: '/bin/host-ficticio', binaryHash: 'a'.repeat(64) }
  const hub = hubOf(TOKEN, identity)
  const url = await hub.listen()
  try {
    const { localHealth } = load()
    const health = await localHealth(url, 2000, TOKEN)
    assert.deepEqual(health, { ok: true, version: '0.0.0-test', environment: 'main', busy: false, ...identity })
    assert.equal(hub.seen.length, 1)
    assert.equal(hub.seen[0]!.authorization, undefined, 'the token itself never travels')
    assert.ok(!hub.seen[0]!.url.includes(TOKEN), 'nor appears in the query')
    assert.equal(await localHealth(url, 2000, 'outro-token-ficticio'), null, 'a refused proof reads nothing')
  } finally {
    await hub.close()
  }
})

test('each call carries a fresh nonce', async () => {
  const hub = hubOf(TOKEN, { computerId: 'computador-ficticio' })
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
