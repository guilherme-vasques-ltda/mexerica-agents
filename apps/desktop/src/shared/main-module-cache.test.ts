// MEX-1431: guarda do custo do `loadMainModule`. Cada rig de teste do main (fork-dock-ciclo carrega o index.ts real com
// os catálogos de texto uma vez por teste) refazia a transpilação inteira e passava de 4 minutos, estourando o tempo do
// job. O cache de transpilação (e um contexto V8 por carga, não por módulo) mantém isso em segundos, sem juntar estado
// entre cargas.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { loadMainModule, transpileStats } from './test-support/main-module.ts'

interface DockModule { applyDockVisibility(): void }

const load = () => {
  const app = { dock: {}, on() { return app }, removeListener() { return app } }
  const prefs = { get: () => false, set() {} }
  return loadMainModule<DockModule & Record<string, unknown>>('dock', { electron: { app }, './host-controller': { prefs } }, { process: { platform: 'darwin', env: {} } })
}

test('MEX-1431: a segunda carga do mesmo módulo reaproveita a transpilação (não chama o TypeScript de novo)', () => {
  load()
  const before = transpileStats.calls
  load()
  load()
  assert.equal(transpileStats.calls, before, 'carregar o mesmo módulo de novo não pode transpilar de novo')
})

test('MEX-1431: o cache guarda só o JS; cada carga tem módulos próprios, sem estado em comum', () => {
  const first = load()
  const second = load()
  assert.notEqual(first, second, 'cada carga devolve um exports novo')
  assert.notEqual(first.applyDockVisibility, second.applyDockVisibility, 'as funções vêm de uma avaliação nova')
})

test('MEX-1431: uma árvore com dependências locais (auth.ts) também carrega toda do cache na segunda vez', () => {
  // Conta transpilações em vez de medir tempo (tempo é frágil sob carga): depois da primeira carga, nenhum arquivo transpila.
  const stub = new Proxy({}, { get: () => () => {} })
  const loadAuth = () => loadMainModule('auth', {
    electron: { app: stub, ipcMain: stub }, './environment': { identity: { port: 0, scheme: 'x', appId: 'x' } },
    './navigation': { openExternalURL: async () => true },
  }, { process: { platform: 'darwin', env: {} } })
  loadAuth()
  const before = transpileStats.calls
  loadAuth()
  assert.equal(transpileStats.calls, before)
})
