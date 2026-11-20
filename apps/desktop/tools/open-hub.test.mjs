import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { openApiPath, openStreamPath, requestOpenHub } from '../src/main/fork-open/hub-transport.ts'

test('a ponte só aceita métodos e streams do Hub Open', () => {
  assert.equal(openApiPath('createSpace'), '/api/createSpace')
  for (const name of ['../health', 'foo?bar', 'a/b', 'a%2fb', 'createSpace\n', 7, null]) {
    assert.equal(openApiPath(name), null)
  }
  const origin = 'http://127.0.0.1:19524'
  assert.equal(openStreamPath(origin, origin + '/events?rev=1'), '/events?rev=1')
  assert.equal(openStreamPath(origin, origin + '/term/setup-123'), '/term/setup-123')
  for (const url of [
    'http://127.0.0.1:19422/events',
    'http://127.0.0.1:19423/events',
    'http://user@127.0.0.1:19524/events',
    origin + '/events#frag',
    origin + '/events\n',
    origin + '/term/../events',
    origin + '/term/a%2Fb',
    origin + '/health',
    'https://example.invalid/events',
    null,
  ]) assert.equal(openStreamPath(origin, url), null)
})

test('o transporte usa o socket privado e não segue redirecionamento', async (t) => {
  const dir = mkdtempSync(join(realpathSync(tmpdir()), 'open-hub-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  const privateDir = join(dir, 's')
  mkdirSync(privateDir, { mode: 0o700 })
  const socketPath = join(privateDir, 'h')
  const server = createServer((req, res) => {
    assert.equal(req.method, 'POST')
    assert.equal(req.url, '/api/createSpace')
    assert.equal(req.headers.authorization, 'Bearer synthetic-fixture')
    res.writeHead(302, { Location: 'https://example.invalid/' }).end()
  })
  await new Promise((resolve, reject) => server.once('error', reject).listen(socketPath, resolve))
  t.after(() => new Promise((resolve) => server.close(resolve)))
  const response = await requestOpenHub(socketPath, '/api/createSpace', 'synthetic-fixture', '{}', AbortSignal.timeout(3000))
  assert.equal(response.statusCode, 302)
  response.resume()
})
