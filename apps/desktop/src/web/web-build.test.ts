import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { loadConfigFromFile } from 'vite'

test('build Web aceita o manifesto e rejeita service worker e Cache Storage', async (t) => {
  const arquivo = fileURLToPath(new URL('../../vite.web.config.ts', import.meta.url))
  const config = await loadConfigFromFile({ command: 'build', mode: 'production' }, arquivo)
  const plugins = (await Promise.all(config?.config.plugins ?? [])).flat()
  const guarda = plugins.find((plugin) => plugin && 'name' in plugin && plugin.name === 'mexerica-web-sem-cache-pwa')
  assert.ok(guarda && 'writeBundle' in guarda)
  const verificar = guarda.writeBundle
  assert.equal(typeof verificar, 'function')
  const dir = mkdtempSync(join(tmpdir(), 'web-build-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  const rodar = () => Reflect.apply(verificar as Function, undefined, [{ dir }, {}])

  writeFileSync(join(dir, 'manifest.webmanifest'), '{}')
  assert.doesNotThrow(rodar)
  for (const [arquivo, fonte] of [
    ['sw.js', ''],
    ['app.js', 'navigator.serviceWorker.register("/worker.js")'],
    ['app.html', '<script>caches.open("rpc").then(c => c.add("/api/test"))</script>'],
    ['app.mjs', 'const storage = caches; storage.open("events").then(c => c.add("/events"))'],
  ] as const) {
    const caminho = join(dir, arquivo)
    writeFileSync(caminho, fonte)
    assert.throws(rodar, /service worker|Cache Storage/)
    rmSync(caminho)
  }
})
