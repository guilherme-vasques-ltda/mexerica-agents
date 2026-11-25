import assert from 'node:assert/strict'
import { createServer, type Server } from 'node:http'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

/**
 * O `/health` do Hub: anônimo para quem não tem o token, com a identidade para o bearer deste
 * computador. Aqui um servidor de verdade responde e registra o header de cada chamada.
 */
const TOKEN = 'token-local-ficticio'

async function hub(reply: (authorization: string | undefined) => { status: number; body: unknown }) {
  const seen: (string | undefined)[] = []
  const server: Server = createServer((request, response) => {
    const authorization = request.headers.authorization
    seen.push(authorization)
    const { status, body } = reply(authorization)
    response.writeHead(status, { 'content-type': 'application/json' })
    response.end(JSON.stringify(body))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert.ok(address && typeof address === 'object')
  return {
    seen,
    url: `http://127.0.0.1:${address.port}`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  }
}

const load = () => loadMainModule<typeof import('../main/fork-local-health.ts')>('fork-local-health', {}, { fetch: (url: string, init?: RequestInit) => fetch(url, init) })

test('anonymous health answers without identity and without a token on the wire', async () => {
  const local = await hub(() => ({ status: 200, body: { ok: true, version: '0.0.0-test', environment: 'main', busy: false } }))
  try {
    const { localHealth } = load()
    const health = await localHealth(local.url, 2000)
    assert.deepEqual(health, { ok: true, version: '0.0.0-test', environment: 'main', busy: false })
    assert.deepEqual(local.seen, [undefined])
  } finally {
    await local.close()
  }
})

test('the local bearer unlocks the identity of the host and of the binary', async () => {
  const identity = { computerId: 'computador-ficticio', binaryPath: '/bin/host-ficticio', binaryHash: 'a'.repeat(64) }
  const local = await hub((authorization) => ({
    status: authorization === `Bearer ${TOKEN}` ? 200 : 401,
    body: authorization === `Bearer ${TOKEN}` ? { ok: true, environment: 'main', busy: true, ...identity } : { error: 'invalid token' },
  }))
  try {
    const { localHealth } = load()
    const health = await localHealth(local.url, 2000, TOKEN)
    assert.deepEqual(health, { ok: true, environment: 'main', busy: true, ...identity })
    assert.deepEqual(local.seen, [`Bearer ${TOKEN}`], 'the token leaves as a Bearer header, once')
    assert.equal(await localHealth(local.url, 2000, 'outro-token-ficticio'), null, 'a refused token reads nothing')
    assert.deepEqual(local.seen.slice(1), ['Bearer outro-token-ficticio'])
  } finally {
    await local.close()
  }
})

test('a host that is down or answers an error reads as nothing', async () => {
  const { localHealth } = load()
  const local = await hub(() => ({ status: 500, body: { ok: false } }))
  try {
    assert.equal(await localHealth(local.url, 2000, TOKEN), null)
  } finally {
    await local.close()
  }
  assert.equal(await localHealth('http://127.0.0.1:1', 500), null, 'nothing listening: null, not a throw')
})
