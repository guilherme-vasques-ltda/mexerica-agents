import assert from 'node:assert/strict'
import { once } from 'node:events'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { serveRemoteWeb } from './serve-remote.ts'

test('serves the gate only as top-level and the renderer only from the same origin', async (t) => {
  const dist = await mkdtemp(join(tmpdir(), 'remote-web-'))
  await writeFile(join(dist, 'index.html'), 'gate')
  await writeFile(join(dist, 'renderer.html'), 'renderer')
  const server = serveRemoteWeb(dist).listen(0, '127.0.0.1')
  t.after(async () => {
    await new Promise<void>(resolve => server.close(() => resolve()))
    await rm(dist, { recursive: true, force: true })
  })
  await once(server, 'listening')
  const address = server.address()
  assert.ok(address && typeof address === 'object')
  const base = `http://127.0.0.1:${address.port}`
  for (const path of ['/', '/index.html?view=1']) {
    const gate = await fetch(base + path)
    assert.equal(await gate.text(), 'gate')
    assert.equal(gate.headers.get('content-security-policy'), "frame-ancestors 'none'")
    assert.equal(gate.headers.get('x-frame-options'), 'DENY')
    assert.equal(gate.headers.get('cache-control'), 'no-store')
  }
  const renderer = await fetch(base + '/renderer.html')
  assert.equal(await renderer.text(), 'renderer')
  assert.equal(renderer.headers.get('content-security-policy'), "frame-ancestors 'self'")
  assert.equal(renderer.headers.get('x-frame-options'), 'SAMEORIGIN')
  for (const [path, policy] of [['/', "frame-ancestors 'none'"], ['/renderer.html', "frame-ancestors 'self'"]]) {
    const head = await fetch(base + path, { method: 'HEAD' })
    assert.equal(head.status, 200)
    assert.equal(head.headers.get('content-security-policy'), policy)
    assert.equal(await head.text(), '')
  }
  assert.equal((await fetch(base + '/unknown')).status, 404)
  assert.equal((await fetch(base + '/assets/%2findex.html')).status, 404)
  assert.equal((await fetch(base + '/', { method: 'POST' })).status, 405)
})
