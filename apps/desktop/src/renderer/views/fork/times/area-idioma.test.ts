import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// R1-03 do #637 (D507, D70): o motivo de recusa da folha de área é calculado num useMemo; ele guarda a frase (dado) e
// a tela traduz ao desenhar. O TimesAreaSheet real roda com um executor mínimo de ganchos que respeita as dependências:
// com o mesmo grafo, trocar o idioma troca o texto do motivo (antes, o memo guardava o texto e ele ficava velho).
const raiz = new URL('../../../../', import.meta.url).pathname
const bundle = await build({
  stdin: { contents: `export { TimesAreaSheet } from './renderer/views/fork/times/TimesAreaSheet'
    export { renderToStaticMarkup } from 'react-dom/server'`, resolveDir: raiz, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'ganchos', setup(b) {
    const falso = (filter: RegExp, contents: string) => {
      const ns = `falso-${filter.source}`
      b.onResolve({ filter }, () => ({ path: 'x', namespace: ns }))
      b.onLoad({ filter: /.*/, namespace: ns }, () => ({ contents, resolveDir: raiz, loader: 'js' }))
    }
    b.onResolve({ filter: /^react$/ }, (a) => /times\/(TimesAreaSheet|parts)\.tsx$/.test(a.importer) ? { path: 'ganchos', namespace: 'ganchos' } : undefined)
    b.onResolve({ filter: /^react-real$/ }, (a) => b.resolve('react', { resolveDir: a.resolveDir, kind: 'import-statement' }))
    b.onLoad({ filter: /.*/, namespace: 'ganchos' }, () => ({ resolveDir: raiz, loader: 'js', contents: `export * from 'react-real'
      export const useState = (v) => ganchos.useState(v)
      export const useMemo = (f, d) => ganchos.useMemo(f, d)
      export const useId = () => 'id'` }))
    falso(/\/fork-i18n\/estado$/, `import { tradutor, superficieIdioma } from './renderer/lib/fork-i18n/index.ts'
      export { superficieIdioma }
      export const useIdioma = () => ganchos.idioma
      export const useT = () => tradutor(ganchos.idioma)`)
    falso(/\/components\/Controls$/, 'export const Button = (p) => p.children')
    falso(/\/components\/Overlay$/, 'export const ModalHeader = () => null; export const SheetPage = (p) => p.children')
    falso(/\/delegacao\/CampoRegras$/, 'export const CampoRegras = () => null')
    falso(/use-chat-info$/, 'export const useChatInfo = () => null')
    falso(/\/fork\/avatars\/Avatar$/, 'export const DonnabotAvatar = () => null')
    falso(/PanelMenus$/, 'export const ProfileAvatar = () => null')
    falso(/\/components\/Icon$/, 'export const Icon = () => null')
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})

type Slot = { v?: unknown; d?: readonly unknown[] }
const mesmas = (a: readonly unknown[] | undefined, b: readonly unknown[]) => !!a && a.length === b.length && a.every((x, i) => Object.is(x, b[i]))
const ganchos = {
  idioma: 'pt-BR',
  slots: [] as Slot[],
  i: 0,
  slot() { const k = this.i++; return (this.slots[k] ??= {}) },
  useState(inicial: unknown) {
    const s = this.slot()
    if (!('v' in s)) s.v = typeof inicial === 'function' ? (inicial as () => unknown)() : inicial
    return [s.v, (v: unknown) => { s.v = v }]
  },
  useMemo(f: () => unknown, d: readonly unknown[]) { const s = this.slot(); if (!mesmas(s.d, d)) { s.v = f(); s.d = d } return s.v },
}
const module = { exports: {} as Record<string, any> }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), ganchos, console, Intl, URL, TextEncoder, queueMicrotask, setTimeout, clearTimeout, MessageChannel, window: { codync: { platform: 'darwin' } } })
const m = module.exports

test('R1-03: trocar o idioma com a folha aberta troca o motivo da recusa (o memo guarda a frase, não o texto)', () => {
  // A área editada aponta para uma área de cima que não existe mais: a conferência local recusa.
  const graph = { nodes: [{ id: 'a1', name: 'Vendas', parentId: 'sumiu', managerBotId: null, position: 0 }], members: [], routes: [] }
  const props = { graph, bots: new Map(), primaryBotId: null, nodeId: 'a1', parentId: null, newId: () => 'n', onSubmit() {}, onCancel() {}, regras: null }
  const desenhar = () => { ganchos.i = 0; return m.renderToStaticMarkup(m.TimesAreaSheet(props)) as string }
  assert.match(desenhar(), /A área de cima não existe mais\./)
  ganchos.idioma = 'en'
  const en = desenhar()
  assert.match(en, /The area above no longer exists\./)
  assert.doesNotMatch(en, /A área de cima/)
})
