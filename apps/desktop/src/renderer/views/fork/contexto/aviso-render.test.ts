import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { CATALOGOS, tradutor } from '../../../lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'

const bundle = await build({
  stdin: {
    contents: "export { AvisoBalao } from './AvisoBalao'; export { createElement } from 'react'; export { renderToStaticMarkup } from 'react-dom/server';",
    resolveDir: new URL('.', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }],
})
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), process, console, TextEncoder, setTimeout, clearTimeout, queueMicrotask })
const m = module.exports as { AvisoBalao: (p: unknown) => unknown; createElement: typeof import('react').createElement; renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup }

for (const idioma of Object.keys(CATALOGOS) as AppLanguage[]) {
  test(`aviso de sucesso renderiza medidas em ${idioma}`, () => {
    const aviso = { id: 'feito', tom: 'ok', icone: 'ok', vars: { antes: '60 mil', depois: '22 mil', de: '13%', para: '5%' } }
    const html = m.renderToStaticMarkup(m.createElement(m.AvisoBalao as never, { aviso, t: tradutor(idioma) }))
    assert.match(html, /60 mil/)
    assert.match(html, /22 mil/)
    assert.doesNotMatch(html, /\{\w+\}/)
  })
}
