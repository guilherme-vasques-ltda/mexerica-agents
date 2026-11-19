import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// MX-4 (C11), revisão do #793: testes do congelamento por camada e do cache do chatView.
// Sem DOM no runner: o shim de `useSyncExternalStore` captura `subscribe`/`getSnapshot` por
// componente — dá para mutar a loja, disparar o listener e conferir o snapshot congelado.

/* eslint-disable @typescript-eslint/no-explicit-any */
type Captura = { s: ((cb: () => void) => () => void) | undefined; g: () => unknown; fired: number; off?: () => void }

async function bundleTeste(stdin: string, stubs: Record<string, string> = {}, extra: Record<string, unknown> = {}) {
  const real = createRequire(import.meta.url).resolve('react')
  const bundle = await build({
    stdin: { contents: stdin, resolveDir: new URL('..', import.meta.url).pathname, loader: 'ts' },
    bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
    alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
    plugins: [{
      name: 'teste-pausa',
      setup(b) {
        b.onResolve({ filter: /^react$/ }, (a) =>
          a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' })
        b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({
          resolveDir: '/', loader: 'js',
          contents: `const R = require(${JSON.stringify(real)});
            module.exports = { ...R, useSyncExternalStore: (s, g) => {
              const rec = { s, g, fired: 0, off: undefined }
              globalThis.__caps.push(rec)
              if (s) rec.off = s(() => { rec.fired++ })
              return g()
            } }`,
        }))
        for (const [path, contents] of Object.entries(stubs)) {
          const name = `stub:${path}`
          // `*x` casa por sufixo (o mesmo módulo pode vir por caminhos relativos diferentes).
          const casa = path.startsWith('*') ? (a: { path: string }) => a.path.endsWith(path.slice(1)) : (a: { path: string }) => a.path === path
          b.onResolve({ filter: /.*/ }, (a) => (casa(a) ? { path: name, namespace: 'stub' } : undefined))
          b.onLoad({ filter: /.*/, namespace: 'stub' }, (a) => (a.path === name ? { contents, loader: 'js' } : undefined))
        }
      },
    }],
    loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
    define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
    logLevel: 'silent',
  })
  const module = { exports: {} }
  const caps: Captura[] = []
  runInNewContext(bundle.outputFiles[0]!.text, {
    module, exports: module.exports, require: createRequire(import.meta.url),
    console, queueMicrotask, setTimeout, clearTimeout, TextEncoder, TextDecoder, URL,
    window: { addEventListener() {}, removeEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }), location: { search: '' } },
    document: { addEventListener() {}, removeEventListener() {}, documentElement: { lang: '', dir: '', setAttribute() {}, removeAttribute() {}, getAttribute: () => null, style: {} } },
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    navigator: { language: 'pt-BR', languages: ['pt-BR'], platform: 'Linux' },
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
    __caps: caps,
    __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
    ...extra,
  })
  return { m: module.exports as any, caps }
}

const tick = () => new Promise((r) => setTimeout(r, 0))

test('C11: pausado, o snapshot fica congelado mesmo com a loja suja; ao voltar, ressincroniza', async () => {
  const { m, caps } = await bundleTeste(`
    export { Observable, useSelector, useSelectMany, useModel, SubscriptionPaused } from './lib/observable'
    export { createElement } from 'react'
    export { renderToStaticMarkup } from 'react-dom/server'`)
  const { Observable, SubscriptionPaused, createElement: h, renderToStaticMarkup } = m

  class Modelo extends Observable {
    campo = 'a'
    bump() { (this as any).changed() }
  }
  const modelo = new Modelo()
  const sonda = () => {
    const v1 = m.useSelector(modelo, (x: any) => x.campo)
    const v2 = m.useSelectMany([modelo], () => [modelo.campo])
    const vm = m.useModel(modelo)
    return h('p', null, `${v1}|${v2[0]}|${vm.campo}`)
  }

  caps.length = 0
  const pausado = renderToStaticMarkup(h(SubscriptionPaused.Provider, { value: true }, h(sonda)))
  assert.equal(pausado, '<p>a|a|a</p>')
  const [gSel, gMany, gModel] = caps.map((c) => c.g) as (() => any)[]
  const versao = modelo.getVersion()

  // A loja muda atrás da tela que cobre: o listener dispara (loja suja), mas o snapshot segue congelado.
  modelo.campo = 'b'
  modelo.bump()
  await tick()
  assert.ok(caps.every((c) => c.fired === 1), 'listeners dispararam')
  assert.equal(gSel(), 'a')
  assert.deepEqual(gMany(), ['a'])
  assert.equal(gModel(), versao)

  // Ao voltar, o snapshot novo enxerga o valor fresco — e despausado segue a loja normalmente.
  caps.length = 0
  const solto = renderToStaticMarkup(h(SubscriptionPaused.Provider, { value: false }, h(sonda)))
  assert.equal(solto, '<p>b|b|b</p>')
  modelo.campo = 'c'
  modelo.bump()
  await tick()
  assert.equal(caps[0]!.g(), 'c')
})

test('C11: camadas aninhadas — qualquer tela coberta pausa a árvore do chat (P2-1)', async () => {
  const flags = { settings: false, extensoes: false, rotinas: false, times: false, torre: false }
  const { m } = await bundleTeste(
    `export { WithRoutinesScreen } from './views/rotinas-central/camada'
     export { WithTimesScreen } from './views/fork/times/camada'
     export { WithTorreScreen } from './views/fork/torre/camada'
     export { SubscriptionPaused } from './lib/observable'
     export { createElement, useContext } from 'react'
     export { renderToStaticMarkup } from 'react-dom/server'`,
    {
      '*sob-demanda/telas': `export const AjustesTela = () => null
        export const ExtensoesTela = () => null
        export const RoutinesCentralView = () => null
        export const TimesScreenView = () => null
        export const TorreScreenView = () => null`,
      '../fork/ajustes/lugar-atual': `export const settingsScreen = { get open() { return !!globalThis.__camadas?.settings } }`,
      '../fork/extensoes/lugar-atual': `export const extensoesScreen = { get open() { return !!globalThis.__camadas?.extensoes } }`,
      './navigation': `export const routinesScreen = { get open() { return !!globalThis.__camadas?.rotinas } }
        export const timesScreen = { get open() { return !!globalThis.__camadas?.times } }`,
      './lugar-atual': `export const torreScreen = { get open() { return !!globalThis.__camadas?.torre } }`,
    },
    { __camadas: flags },
  )
  const { WithRoutinesScreen, WithTimesScreen, WithTorreScreen, SubscriptionPaused, createElement: h, useContext, renderToStaticMarkup } = m
  const probe = () => h('p', null, String(useContext(SubscriptionPaused)))
  const janela = h(WithRoutinesScreen, null, h(WithTimesScreen, null, h(WithTorreScreen, null, h(probe))))

  for (const cenario of Object.keys(flags) as (keyof typeof flags)[]) {
    for (const k of Object.keys(flags)) flags[k as keyof typeof flags] = false
    flags[cenario] = true
    assert.match(renderToStaticMarkup(janela), />true</, `${cenario} aberta pausa o chat`)
  }
  for (const k of Object.keys(flags)) flags[k as keyof typeof flags] = false
  assert.match(renderToStaticMarkup(janela), />false</, 'nada aberto, nada pausado')
})

test('C10: chatView cacheia por lista e a chave separa conversa, sessão e sessões conhecidas', async () => {
  const { m } = await bundleTeste(`export { chatView } from './views/thread/chat-items'`)
  const entrada = (id: string, sessionId: string) => ({ id, seq: 1, botId: 'b', rev: 1, kind: 'markdown', turn: 1,
    createdAt: 0, updatedAt: 0, sessionId, data: { text: 'oi' } }) as any
  const lista = [entrada('e1', 's1'), entrada('e2', 's1')]

  // Mesma lista + mesmos argumentos: mesmo objeto cacheado (a remontagem não refaz o pipeline).
  const v1 = m.chatView(lista, 'b', 's1', new Set(['s1']))
  assert.equal(m.chatView(lista, 'b', 's1', new Set(['s1'])), v1)
  // Lista nova (a loja troca o array a cada mudança): view nova.
  assert.notEqual(m.chatView([...lista], 'b', 's1', new Set(['s1'])), v1)
  // Sessão e conjunto de conhecidas fazem parte da chave.
  assert.notEqual(m.chatView(lista, 'b', null, new Set(['s1'])), v1)
  assert.notEqual(m.chatView(lista, 'b', 's1', new Set(['s1', 's2'])), v1)
  // A chave não pode colapsar campos: "a b"/"c" ≠ "a"/"b c" (P3 da revisão do #793).
  assert.notEqual(m.chatView(lista, 'a b', 'c', new Set()), m.chatView(lista, 'a', 'b c', new Set()))
})
