import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build, type Plugin } from 'esbuild'
import ts from 'typescript'
import { CATALOGOS } from './fork-i18n/index.ts'

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
const source = read('../views/settings/AppearanceSettings.tsx')
const css = read('../views/settings/appearance.css')
const rule = (selector: string) => {
  const start = css.indexOf(`${selector} {`)
  assert.ok(start >= 0, `${selector} existe`)
  return css.slice(start, css.indexOf('}', start))
}

// Mesmo padrão dos testes SSR de idiomas: componente real; só o snapshot dos hooks
// e as fronteiras de navegador são adaptados ao servidor, sem chamar startAppearance.
const require = createRequire(import.meta.url)
const serverReact: Plugin = {
  name: 'appearance-server-react',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => args.namespace === 'shim' ? undefined : { path: 'react', namespace: 'shim' })
    b.onLoad({ filter: /^react$/, namespace: 'shim' }, () => ({
      contents: `const R = require(${JSON.stringify(require.resolve('react'))}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
const bundle = await build({
  stdin: {
    contents: `export { AppearanceSettings } from './views/settings/AppearanceSettings';
      export { createElement } from 'react'; export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [serverReact],
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const saved = new Map([['appLanguage', '"pt-BR"'], ['appearanceAccent', '"folha"'], ['sidebarCompact', 'true']])
const writes: string[] = []
const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { writes.push(key); saved.set(key, value) }, removeItem: (key: string) => { writes.push(key); saved.delete(key) } }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require,
  window: { codync: { platform: 'darwin' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage },
  document: { documentElement: { dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false },
  fetch: () => new Promise(() => {}),
  localStorage: storage, navigator: { platform: 'MacIntel' }, matchMedia: media,
  requestAnimationFrame: () => 0, setTimeout, clearTimeout, queueMicrotask, console, URL, TextEncoder,
  __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const view = module.exports as {
  AppearanceSettings: typeof import('../views/settings/AppearanceSettings').AppearanceSettings
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
const render = () => view.renderToStaticMarkup(view.createElement(view.AppearanceSettings))

test('D529b: prévia e controles são irmãos, e a navegação não ganha direção condicional', () => {
  const html = render()
  assert.match(html, /class="appearance-workspace idioma-superficie" lang="pt-BR" dir="ltr"/)
  assert.match(html, /class="appearance-visualization"><div class="appearance-preview[^\"]*" aria-hidden="true"/)
  assert.match(html, /<\/div><\/div><div class="appearance-controls" role="region"/)
  assert.ok(html.indexOf('appearance-preview') < html.indexOf('appearance-controls'))
  assert.doesNotMatch(html.slice(html.indexOf('appearance-visualization'), html.indexOf('appearance-controls')), /<button|<input|\btabindex=/, 'prévia fora da navegação por teclado')
  const root = /<div className=\{`settings\$\{[^>]+>/.exec(read('../views/settings/SettingsView.tsx'))![0]
  assert.match(root, /page === 'appearance' \? ' settings-appearance' : ''/)
  assert.doesNotMatch(root, /superficieIdioma|idioma-superficie|\bdir=/)
})

test('D529b: só os controles rolam; prévia fixa empilha sem sobreposição e comporta a barra recolhida', () => {
  assert.match(rule('.appearance-workspace'), /display: grid/)
  assert.match(rule('.appearance-workspace'), /grid-template-columns: minmax\(0, 1fr\) minmax\(320px, 400px\)/)
  assert.match(rule('.settings-appearance .settings-page'), /overflow: hidden/)
  assert.match(rule('.appearance-controls'), /overflow-y: auto/)
  assert.match(rule('.appearance-controls .card-form'), /overflow: visible/)
  assert.match(rule('.appearance-preview'), /overflow: hidden/)
  const scrolling = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter((m) => /overflow(?:-y)?:\s*(?:auto|scroll)/.test(m[2]!))
  assert.equal(scrolling.length, 1)
  assert.equal(scrolling[0]![1]!.trim(), '.appearance-controls')
  assert.match(css, /@container \(max-width: 820px\)/)
  const compact = rule('.appearance-workspace:has(.appearance-preview.compact)')
  const floor = Number(/clamp\((\d+)px/.exec(compact)![1])
  // 4 marcas de 14, 4 espaços de 7, ícones 18+8+18, padding da barra e da prévia.
  assert.ok(floor >= 4 * 14 + 4 * 7 + 2 * 18 + 8 + 24 + 24, 'ambos os ícones cabem no mínimo de altura')
})

test('D529b: renderizar a prévia não grava preferências e só expõe leituras', () => {
  const before = [...saved]
  writes.length = 0
  const html = render()
  assert.match(html, /class="appearance-preview compact" aria-hidden="true"/)
  assert.deepEqual(writes, [])
  assert.deepEqual([...saved], before)
  const sf = ts.createSourceFile('AppearanceSettings.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const preview = sf.statements.find((n) => ts.isFunctionDeclaration(n) && n.name?.text === 'AppearancePreview')!
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)) assert.ok(!['set', 'setItem', 'removeItem'].includes(node.expression.name.text), 'prévia não escreve estado ou armazenamento')
    if (ts.isVariableDeclaration(node) && node.initializer && ts.isCallExpression(node.initializer) && node.initializer.expression.getText(sf) === 'usePref') assert.equal((node.name as ts.ArrayBindingPattern).elements.length, 1, 'não recebe setter de preferência')
    ts.forEachChild(node, visit)
  }
  visit(preview)
})

test('D529b: controles têm nome distinto nos seis idiomas, foco rolável e retorno dos menus só nesta página', () => {
  assert.match(render(), /aria-label="Controles de aparência" tabindex="0"/)
  for (const catalog of Object.values(CATALOGOS)) {
    assert.ok(catalog['ajustes.aparencia.controles'].trim())
    assert.notEqual(catalog['ajustes.aparencia.controles'], catalog['ajustes.pagina.aparencia'])
  }
  assert.equal((read('../components/Controls.tsx').match(/restoreFocusOnClose = false/g) ?? []).length, 2)
  assert.equal((source.match(/<ChoicePicker restoreFocusOnClose/g) ?? []).length, 2)
})

// D530/R2: o interruptor acompanha os controles roláveis do layout D529b.
test('D530 R2: Toques Mexerica fica uma única vez na coluna de controles, fora da prévia', () => {
  const html = render()
  const split = html.indexOf('class="appearance-controls"')
  assert.ok(split > 0)
  assert.doesNotMatch(html.slice(0, split), /Toques Mexerica|mx-touch-setting/)
  assert.match(html.slice(split), /mx-touch-setting/)
  assert.equal((html.match(/Toques Mexerica/g) ?? []).length, 1)
  assert.match(html.slice(split), /role="switch"[^>]*aria-checked="true"/)
})
