import assert from 'node:assert/strict'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildSync } from 'esbuild'

const bundle = buildSync({
  entryPoints: [new URL('../views/bot-groups/prefs.ts', import.meta.url).pathname],
  bundle: true, platform: 'node', format: 'cjs', write: false, define: { 'process.env.NODE_ENV': '"production"' },
}).outputFiles[0]!.text

function load(storage: Map<string, string>) {
  const module = { exports: {} }
  const localStorage = { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => { storage.set(key, value) }, removeItem: (key: string) => { storage.delete(key) } }
  runInNewContext(bundle, { module, localStorage, window: { addEventListener() {} } })
  return module.exports as typeof import('../views/bot-groups/prefs')
}

test('pares antigos são migrados ao identificar cada computador, sem perder os outros', () => {
  const first = JSON.stringify(['pc-um', 'grupo-um'])
  const second = JSON.stringify(['pc-dois', 'grupo-dois'])
  const third = JSON.stringify(['pc-um', 'grupo-trabalho'])
  const storage = new Map([['donnabotCollapsedBotSections', JSON.stringify([first, second, third])], ['rosterBotSectionFilter', '"grupo-um"']])
  const prefs = load(storage)
  assert.equal(storage.has('donnabotCollapsedBotSections'), false)
  assert.equal(storage.has('rosterBotSectionFilter'), false)
  const entries = JSON.parse(storage.get('gruposRecolhidos')!) as string[]
  const migrated = prefs.migrarRecolhimento(entries, 'pc-um', 'hub-um', 'pessoal', ['grupo-um'])
  assert.deepEqual(Array.from(migrated), [JSON.stringify(['hub-um', 'pessoal', 'grupo-um']), second, third])
  const another = prefs.migrarRecolhimento(migrated, 'pc-dois', 'hub-dois', 'trabalho', ['grupo-dois'])
  assert.deepEqual(Array.from(another), [JSON.stringify(['hub-um', 'pessoal', 'grupo-um']), JSON.stringify(['hub-dois', 'trabalho', 'grupo-dois']), third])
  const last = prefs.migrarRecolhimento(another, 'pc-um', 'hub-um', 'trabalho', ['grupo-trabalho'])
  assert.deepEqual(Array.from(last), [JSON.stringify(['hub-um', 'pessoal', 'grupo-um']), JSON.stringify(['hub-dois', 'trabalho', 'grupo-dois']), JSON.stringify(['hub-um', 'trabalho', 'grupo-trabalho'])])
  assert.equal(prefs.migrarRecolhimento(last, 'pc-dois', 'hub-dois', 'trabalho', ['grupo-dois']), last)
})

test('recolhimentos anteriores e novos são preservados na leitura única', () => {
  const prior = JSON.stringify(['pc', 'antigo'])
  const current = JSON.stringify(['hub', 'space', 'novo'])
  const storage = new Map([['donnabotCollapsedBotSections', JSON.stringify([prior])], ['gruposRecolhidos', JSON.stringify([current])]])
  load(storage)
  assert.deepEqual(JSON.parse(storage.get('gruposRecolhidos')!), [current, prior])
  assert.equal(storage.has('donnabotCollapsedBotSections'), false)
})
