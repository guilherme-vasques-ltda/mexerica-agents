import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// D70, R1 do #572 (item 4): o ChoicePicker leva o idioma da tela migrada até a raiz do menu (portal no body); sem a
// prop, nada muda. O menu real abre em portal: aqui o AnchoredMenu é trocado por um marcador que mostra o que recebeu.
const bundle = await build({
  stdin: {
    contents: `export { ChoicePicker } from './components/Controls';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent',
  plugins: [{ name: 'portal', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$|^\.\/Icon$/ }, () => ({ path: 'icone', namespace: 'stub' }))
    b.onResolve({ filter: /^\.\/Overlay$/ }, () => ({ path: 'overlay', namespace: 'stub' }))
    b.onLoad({ filter: /^icone$/, namespace: 'stub' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    b.onLoad({ filter: /^overlay$/, namespace: 'stub' }, () => ({ loader: 'js', resolveDir: '/', contents: `
      const { createElement } = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))})
      export function AnchoredMenu({ language }) { return createElement('div', { 'data-menu': '', ...(language ?? {}) }) }
      export function IconButtonBase() { return null }
      export function useEscape() {}
      export function usePresence() { return { mounted: false, shown: false } }
      export const anchorPlacement = () => ({ style: {}, width: 0, room: 0 })` }))
  } }],
  loader: { '.css': 'empty', '.svg': 'empty', '.png': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage,
  console, setTimeout, clearTimeout, queueMicrotask, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false } } })
/* eslint-disable @typescript-eslint/no-explicit-any */
const m = module.exports as any
const options = [{ id: 'ask', label: 'اسأل' }, { id: 'auto', label: 'اسمح' }]

test('ChoicePicker repassa o idioma à raiz do menu; sem a prop, o menu não declara nada', () => {
  const comIdioma = m.renderToStaticMarkup(m.createElement(m.ChoicePicker, { selection: 'ask', options, onChange() {}, language: { lang: 'ar-AE', dir: 'rtl' } }))
  assert.match(comIdioma, /<div data-menu="" lang="ar-AE" dir="rtl"><\/div>/)
  const semIdioma = m.renderToStaticMarkup(m.createElement(m.ChoicePicker, { selection: 'ask', options, onChange() {} }))
  assert.match(semIdioma, /<div data-menu=""><\/div>/)
})
