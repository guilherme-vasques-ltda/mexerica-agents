import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// R1 do #597 (D454, D70): o recibo de memória na conversa declara o idioma e a direção do app (o corpo da janela segue
// pt-BR/ltr até a última fatia) e isola o conteúdo recebido; a versão vai em LTR isolado.
const bundle = await build({
  stdin: {
    contents: `export { MemoryNotice } from './views/thread/MemoryNotice'
      export { createElement } from 'react'
      export { renderToStaticMarkup } from 'react-dom/server'`,
    resolveDir: new URL('../../..', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  loader: { '.css': 'empty', '.jpg': 'empty', '.webp': 'empty', '.png': 'empty', '.svg': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"' },
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'stubs', setup(b) {
    const stub = (filtro: RegExp, contents: string) => {
      b.onResolve({ filter: filtro }, (a) => ({ path: a.path, namespace: 'stub' }))
      b.onLoad({ filter: filtro, namespace: 'stub' }, () => ({ contents, loader: 'js' }))
    }
    stub(/store\/context$/, "export const useStore = () => globalThis.__host")
    stub(/lib\/observable$/, 'export class Observable {}; export const useModel = (m) => m')
    stub(/store\/memory-store$/, 'export class MemoryNoticeStore { state(entry) { return { entry, unknown: false, busy: false } } undo() {} }')
    stub(/memoria-auto\/placement$/, 'export const memoryResponse = () => true')
    stub(/components\/Controls$/, 'export const Spinner = () => null')
    stub(/components\/Icon$/, 'export const Icon = () => null')
  } }],
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'darwin' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, location: { search: '' } }
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"ar-AE"' : null), setItem() {}, removeItem() {} }
// O `useStore` simulado lê o host deste `globalThis` do contexto.
const host = { ready: async () => ({}), allEntries: () => [], bots: new Map([['b1', { name: 'Lia' }]]) }
const module = { exports: {} as Record<string, any> }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, console, setTimeout, clearTimeout, queueMicrotask, TextEncoder, URL, URLSearchParams, Intl, globalThis: { __host: host } })
const m = module.exports

const recibo = (memory: Record<string, unknown>) => m.renderToStaticMarkup(m.createElement(m.MemoryNotice, { entry: { id: 'e1', botId: 'b1', kind: 'notice', data: { memory } } }))

test('R1-01: o recibo de memória em árabe declara lang/dir do app e isola fato, conflito e versão', () => {
  const html = recibo({ action: 'remember', status: 'done', text: 'Clara is allergic', nature: 'inferred', natureBot: 'b1', conflictText: 'Clara likes peanuts', rev: 3, at: 1793718000000, origin: { schema: 1, kind: 'user', sources: [], external: false } })
  assert.match(html, /^<div class="memory-auto-notice idioma-superficie" lang="ar-AE" dir="rtl"/)
  assert.match(html, /<bdi dir="auto">Clara is allergic<\/bdi>/)
  assert.match(html, /<bdi dir="auto">Clara likes peanuts<\/bdi>/)
  assert.match(html, /<bdi dir="auto">Lia<\/bdi>/)
  assert.match(html, /⁦ ?3⁩/)
  const simples = recibo({ action: 'remember', status: 'done', text: 'fato sem natureza' })
  assert.match(simples, /<bdi dir="auto">fato sem natureza<\/bdi>/)
})
