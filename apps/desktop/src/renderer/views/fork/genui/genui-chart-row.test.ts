import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'
import type { Entry } from '../../../../shared/models.ts'

const bundle = await build({
  stdin: {
    contents: `export { ChatRow } from './views/thread/ChatRows';
      export { GenuiChartMessage } from './views/fork/genui/GenuiChartRow';
      export { genuiChartOf } from './views/fork/genui/chart-entry';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: fileURLToPath(new URL('../../../', import.meta.url)), loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    // Render no servidor não segue modelos (useSyncExternalStore sem snapshot de servidor): leitura direta.
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'observavel', namespace: 'observavel' }))
    b.onLoad({ filter: /.*/, namespace: 'observavel' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }\nexport const useModel = (m) => m\nexport const useModels = () => {}\nexport const useSelector = (m, select) => select(m)\nexport const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]))\nexport const useSelectMany = (ms, f) => f()\nexport const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  // ChatRows puxa módulos com `import.meta.glob`/`env` do Vite: um objeto mínimo no realm.
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
// O texto conferido é o pt-BR: idioma escolhido pt-BR (fatia 2: o cartão segue o idioma do app; D454/D495).
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage }
// DON-903/DON-907 C9a/C4: o ChatRows já puxa a base de Idiomas, que acerta `lang`/`dir` do documento ao carregar.
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr' }, body: { setAttribute() {} } }
const module = { exports: {} }
const realm = { module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage, navigator: { platform: 'Linux', language: 'pt-BR' },
  TextEncoder, URLSearchParams, URL, console, setTimeout, clearTimeout, queueMicrotask, requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } } }
runInNewContext(bundle.outputFiles[0]!.text, realm)
const realmJSON = runInNewContext('JSON', realm) as JSON
const local = <T>(v: T): T => realmJSON.parse(JSON.stringify(v)) as T
const m = module.exports as {
  ChatRow: typeof import('../../thread/ChatRows').ChatRow
  GenuiChartMessage: typeof import('./GenuiChartRow').GenuiChartMessage
  genuiChartOf: typeof import('./chart-entry').genuiChartOf
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const payload = {
  version: 1, component: 'bar_chart', title: 'Vendas do trimestre', summary: 'Valores fictícios informados na conversa.',
  data: { labels: ['Jan', 'Fev', 'Mar'], series: [{ id: 'vendas', label: 'Vendas', values: [10, null, 30.5] }], unit: 'itens', period: 'Primeiro trimestre', sourceLabel: 'Planilha fictícia' },
}
const snapshotOf = (p: unknown = payload, requested = 'bar_chart', effective = 'bar_chart') => JSON.stringify({ requested, effective, payload: p })
const projection = { version: 1, chartId: 'c1', snapshot: snapshotOf() }
const entry = (genuiChart: unknown, extra: Partial<Entry> = {}): Entry => local({
  id: 'e1', seq: 1, botId: 'byte', rev: 1, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, sessionId: 's-a',
  data: { text: 'TEXTO LITERAL DO GRAFICO', genuiChart }, ...extra,
}) as Entry
const store = { bots: new Map(), authorName: () => 'Donna', hello: { hostId: 'hub-1' }, connectedClient: null, answering: new Map(), hostName: 'Mac' }
const row = (e: Entry) => m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never },
  m.createElement(m.ChatRow, { entry: e, groupStart: true, chat: null, openTrace() {} })))

test('entrada real do Hub vira gráfico com resumo, origem informada e Ver dados', () => {
  const html = row(entry(projection))
  assert.match(html, /class="chart-card[ "]/)
  assert.match(html, /Vendas do trimestre/)
  assert.match(html, /Valores fictícios informados na conversa\./)
  assert.match(html, /Informado pelo bot/)
  assert.match(html, /Ver dados/)
  assert.match(html, /aria-expanded="false"/)
  assert.doesNotMatch(html, /TEXTO LITERAL DO GRAFICO/)
  assert.equal(m.genuiChartOf(entry(projection))?.chartId, 'c1')
  const message = m.renderToStaticMarkup(m.createElement(m.GenuiChartMessage, { card: m.genuiChartOf(entry(projection))! }))
  assert.match(message, /genui-chart-message/)
})

test('texto com marcação ou script do modelo aparece como texto, nunca como marcação', () => {
  const attack = '<script>alert(1)</script><img src=x onerror=alert(1)>'
  const hostile = { ...payload, title: attack, summary: '[clique](javascript:alert(1))', data: { ...payload.data, labels: [attack, 'Fev', 'Mar'], sourceLabel: '<b>fonte</b>' } }
  const html = row(entry({ ...projection, snapshot: snapshotOf(hostile) }))
  assert.match(html, /class="chart-card[ "]/)
  assert.doesNotMatch(html, /<script|<img|<b>fonte/)
  assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/)
  const tags = html.replace(/="[^"]*"/g, '=""')
  assert.doesNotMatch(tags, /<a |href=|<[^>]* on\w+=/)
})

test('forma fora do contrato, campo extra ou fora de notice fica no texto literal', () => {
  const withExtra = (key: string) => JSON.stringify({ requested: 'bar_chart', effective: 'bar_chart', payload: { ...payload, [key]: '<b>x</b>' } })
  for (const [why, bad] of [
    ['versão 2', { ...projection, version: 2 }],
    ['chave extra na projeção', { ...projection, html: '<b>x</b>' }],
    ['sem chartId', { version: 1, snapshot: snapshotOf() }],
    ['chartId vazio', { ...projection, chartId: '' }],
    ['snapshot que não é texto', { ...projection, snapshot: { payload } }],
    ['snapshot não é JSON', { ...projection, snapshot: '<svg onload=alert(1)>' }],
    ['html no payload', { ...projection, snapshot: withExtra('html') }],
    ['script no payload', { ...projection, snapshot: withExtra('js') }],
    ['url no payload', { ...projection, snapshot: withExtra('url') }],
    ['pedido e efetivo diferentes', { ...projection, snapshot: snapshotOf(payload, 'line_chart', 'bar_chart') }],
    ['valor em texto', { ...projection, snapshot: snapshotOf({ ...payload, data: { ...payload.data, series: [{ id: 'v', label: 'V', values: ['1', 2, 3] }] } }) }],
    ['matriz irregular', { ...projection, snapshot: snapshotOf({ ...payload, data: { ...payload.data, series: [{ id: 'v', label: 'V', values: [1, 2] }] } }) }],
    ['número fora do limite', { ...projection, snapshot: snapshotOf({ ...payload, data: { ...payload.data, series: [{ id: 'v', label: 'V', values: [1e300, 2, 3] }] } }) }],
    ['componente desconhecido', { ...projection, snapshot: snapshotOf({ ...payload, component: 'table' }, 'table', 'table') }],
    ['nulo', null],
    ['lista', []],
    ['texto', 'genuiChart'],
  ] as const) {
    assert.equal(m.genuiChartOf(entry(bad)), null, why)
    const html = row(entry(bad))
    assert.doesNotMatch(html, /chart-card/, why)
    assert.match(html, /TEXTO LITERAL DO GRAFICO/, why)
  }
  for (const kind of ['user', 'agent', 'permission', 'thought', 'tool', 'NOTICE']) assert.equal(m.genuiChartOf(entry(projection, { kind })), null, kind)
  assert.equal(m.genuiChartOf(entry(projection, { data: { text: 'x', card: 'ownerQuestion', genuiChart: projection } as never })), null)
  const forged = entry(projection, { kind: 'user' })
  assert.doesNotMatch(row(forged), /chart-card/)
})

test('pizza acima de cinco partes aparece em barras e diz isso; nenhum botão executa ação', () => {
  const labels = ['A', 'B', 'C', 'D', 'E', 'F']
  const pie = { ...payload, component: 'pie_chart', data: { labels, series: [{ id: 's', label: 'Partes', values: [1, 2, 3, 4, 5, 6] }], unit: '%', period: 'Hoje', sourceLabel: 'Fictícia' } }
  const html = row(entry({ ...projection, snapshot: snapshotOf(pie, 'pie_chart', 'bar_chart') }))
  assert.match(html, /Pedido como pizza; com mais de 5 partes aparece em barras/)
  assert.deepEqual([...html.matchAll(/<button[^>]*>/g)].map((b) => b[0].includes('chart-toggle')), [true])
})
