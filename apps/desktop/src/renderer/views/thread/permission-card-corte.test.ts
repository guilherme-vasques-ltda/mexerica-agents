import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// Cartão renderizado com detalhe maior que o corte da tela (2000): o texto cortado aparece com o aviso
// de que o que não aparece não pode ser conferido e nenhuma opção de aprovar; detalhe curto segue com
// aprovação disponível.
const bundle = await build({
  stdin: {
    contents: `export { idiomaPref } from './lib/fork-i18n/estado';
      export { PermissionCard } from './views/thread/PermissionCard';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: fileURLToPath(new URL('../../', import.meta.url)), loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': fileURLToPath(new URL('../../../shared', import.meta.url)) },
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'observavel', namespace: 'observavel' }))
    b.onLoad({ filter: /.*/, namespace: 'observavel' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }\nexport const useModel = (m) => m\nexport const useModels = () => {}\nexport const useSelector = (m, select) => select(m)\nexport const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]))\nexport const useSelectMany = (ms, f) => f()\nexport const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
    const real = createRequire(import.meta.url).resolve('react')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) }` }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
  logLevel: 'silent',
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: () => '"pt-BR"', setItem() {}, removeItem() {} }
const window = { codync: { platform: 'linux', speech: { available: false }, app: {} }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, location: { search: '' } }
const no = () => {}
const element = () => ({ setAttribute: no, removeAttribute: no, getAttribute: () => null, style: {}, classList: { add: no, remove: no, toggle: no, contains: () => false }, addEventListener: no, removeEventListener: no, appendChild: no, querySelector: () => null })
const document = { documentElement: { ...element(), lang: '', dir: '' }, body: element(), head: element(), createElement: element, addEventListener: no, removeEventListener: no, querySelector: () => null, querySelectorAll: () => [], visibilityState: 'visible', hasFocus: () => true }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { document, module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage,
  navigator: { platform: 'Linux', language: 'pt-BR', languages: ['pt-BR'] }, TextEncoder, TextDecoder, URL, URLSearchParams, console, setTimeout, clearTimeout, setInterval, clearInterval, queueMicrotask, crypto, requestAnimationFrame: () => 0,
  __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } } })
/* eslint-disable @typescript-eslint/no-explicit-any */
const m = module.exports as any
const html = (el: unknown) => m.renderToStaticMarkup(el) as string
const h = m.createElement
const store = { bots: new Map() }

const entrada = (detail: string) => ({ id: 'p1', seq: 1, botId: 'b', rev: 1, kind: 'permission', turn: 1, createdAt: 0, updatedAt: 0, data: {
  status: 'pending', toolKind: 'other', title: 'Autorizar usar ferramenta não identificada: alvo não informado', cwd: '/projetos/app',
  detail, options: [{ optionId: 'allow-once', name: 'Permitir uma vez', kind: 'allow_once' }, { optionId: 'reject', name: 'Recusar', kind: 'reject_once' }] } })

test('comando local mantém a recusa disponível mas exige abrir os detalhes para aprovar', () => {
  const entry = entrada('{"argv":["/usr/bin/true"],"cwd":"/projetos/app"}')
  entry.data.toolKind = 'open_local_command'
  const card = html(h(m.StoreContext.Provider, { value: store }, h(m.PermissionCard, { entry, hostName: 'Mac', answering: null, respond() {} })))
  assert.doesNotMatch(card, /hold-button/)
  assert.match(card, /Detalhes/)
  assert.match(card, /Recusar/)
})

test('detalhe maior que o corte da tela: aviso visível e nenhuma opção de aprovar', () => {
  const card = html(h(m.StoreContext.Provider, { value: store }, h(m.PermissionCard, { entry: entrada('x'.repeat(2500)), hostName: 'Mac', answering: null, respond() {} })))
  assert.match(card, /Texto cortado; o que não aparece não pode ser conferido\./)
  assert.doesNotMatch(card, /hold-button/, 'sem botão de aprovar com o detalhe cortado')
  assert.match(card, /Recusar/, 'negar continua disponível')
})

test('detalhe curto: aprovação disponível e sem aviso de corte', () => {
  const card = html(h(m.StoreContext.Provider, { value: store }, h(m.PermissionCard, { entry: entrada('argumentos do pedido 7'), hostName: 'Mac', answering: null, respond() {} })))
  assert.match(card, /hold-button/, 'aprovar disponível com o detalhe inteiro')
  assert.doesNotMatch(card, /Texto cortado/)
  assert.match(card, /Recusar/)
})
