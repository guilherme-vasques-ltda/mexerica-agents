import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { configDaFrutinha } from '../../../../shared/fork-frutinhas.ts'

// R1-1 (#555): o rosto tem o tamanho pedido. Renderiza o real (fork/avatars/Avatar.tsx) no servidor, com o
// `import.meta.glob` do Vite trocado por um mapa mínimo; não há layout no Node, então o teste prova os atributos.
const bundle = await build({
  stdin: {
    contents: `export { RostoParado, CharacterAvatar } from './fork/avatars/Avatar';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent',
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const artes = Object.fromEntries(['developer', 'mexerica'].flatMap((a) => [128, 256, 512].map((t) => [`./frutinhas/${a}-${t}.webp`, `/a/${a}-${t}.webp`])))
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage }
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'Linux', language: 'pt-BR' }, TextEncoder, URLSearchParams, URL, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => artes, env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  CharacterAvatar: typeof import('../../../fork/avatars/Avatar').CharacterAvatar
  RostoParado: typeof import('../../../fork/avatars/Avatar').RostoParado
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
const html = (size: number) => m.renderToStaticMarkup(m.createElement(m.RostoParado, { config: configDaFrutinha('developer'), size }))

test('2a: no Novo bot (30px), o legado recebe uma Mexeriquinha SVG no tamanho pedido', () => {
  const h = html(30)
  assert.match(h, /<svg[^>]*class="mxq mxq--livre(?: mxq--compacto)?"/)
  assert.match(h, /width="30"/)
  assert.match(h, /height="30"/)
  assert.match(h, /data-mexeriquinha="simples"/)
  assert.match(h, /style="display:inline-flex;flex:none;width:30px;height:30px"/)
  assert.doesNotMatch(h, /<img|dbf-root/)
})

test('2a: a prévia grande mantém SVG e dimensão própria, sem rasterizar', () => {
  const h = html(150)
  assert.match(h, /<svg[^>]*width="150"[^>]*height="150"/)
  assert.match(h, /width:150px;height:150px/)
  assert.doesNotMatch(h, /<img/)
})

test('2a: mascote sem identidade de bot conserva o desenho público do provedor', () => {
  const h = m.renderToStaticMarkup(m.createElement(m.CharacterAvatar, { shape: 'blob', color: 'blue', size: 24 }))
  assert.match(h, /dbf-root/)
  assert.doesNotMatch(h, /data-mexeriquinha|class="mxq /)
})
