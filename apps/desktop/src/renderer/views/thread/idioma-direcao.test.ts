import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// D70, fatia 2 (R1/R2 da revisão do #565): com o app em árabe (ar-AE), os cartões traduzidos declaram RTL, mas
// comando, caminho e diff do cartão de aprovação ficam LTR e isolados (senão o bidi mostra `+x;` como `;x+` e a
// pessoa aprova o que leu errado). O campo de mensagem e a tabela declaram o idioma e a direção do app; o texto
// da pessoa e as células seguem a própria direção.
const bundle = await build({
  stdin: {
    contents: `export { idiomaPref } from './lib/fork-i18n/estado';
      export { TelaPorComputador } from './views/fork/computador/TelaPorComputador';
      export { PermissionCard, CodeBox, DiffView } from './views/thread/PermissionCard';
      export { DataTable } from './views/thread/cards/DataTable';
      export { Composer } from './views/thread/Composer';
      export { StoreContext } from './store/context';
      export { BotStore } from './store/bot-store';
      export { normalizeBot } from '../shared/models';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    // Render no servidor não segue modelos (useSyncExternalStore sem snapshot de servidor): leitura direta.
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'observavel', namespace: 'observavel' }))
    // Render no servidor: os hooks do app que não dão `getServerSnapshot` (rascunhos, movimento reduzido) usam o
    // snapshot do cliente. Só o React importado pelo código do app passa pelo shim; o react-dom usa o React real.
    const real = createRequire(import.meta.url).resolve('react')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) }` }))
    b.onLoad({ filter: /.*/, namespace: 'observavel' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }\nexport const useModel = (m) => m\nexport const useModels = () => {}\nexport const useSelector = (m, select) => select(m)\nexport const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]))\nexport const useSelectMany = (ms, f) => f()\nexport const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp4': 'empty', '.json': 'json' },
  // Módulos com `import.meta.glob`/`env` do Vite: um objeto mínimo no realm (como em genui-choice-row.test.ts).
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
  logLevel: 'silent',
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"ar-AE"' : null), setItem() {}, removeItem() {} }
const window = { codync: { platform: 'linux', speech: { available: false }, app: {} }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, location: { search: '' } }
// Documento mínimo: estado.ts sincroniza `lang`/`dir` do <html> e alguns módulos leem o documento ao carregar.
const no = () => {}
const element = () => ({ setAttribute: no, removeAttribute: no, getAttribute: () => null, style: {}, classList: { add: no, remove: no, toggle: no, contains: () => false }, addEventListener: no, removeEventListener: no, appendChild: no, querySelector: () => null })
const document = { documentElement: { ...element(), lang: '', dir: '' }, body: element(), head: element(), createElement: element, addEventListener: no, removeEventListener: no, querySelector: () => null, querySelectorAll: () => [], visibilityState: 'visible', hasFocus: () => true }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { document, module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage,
  navigator: { platform: 'Linux', language: 'ar-AE', languages: ['ar-AE'] }, TextEncoder, TextDecoder, URL, URLSearchParams, console, setTimeout, clearTimeout, setInterval, clearInterval, queueMicrotask, crypto, requestAnimationFrame: () => 0,
  __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } } })
/* eslint-disable @typescript-eslint/no-explicit-any */
const m = module.exports as any
const html = (el: unknown) => m.renderToStaticMarkup(el) as string
const h = m.createElement

test('R1: em ar-AE o cartão de aprovação é RTL, mas comando, caminho e diff ficam LTR e isolados', () => {
  const entry = { id: 'p1', seq: 1, botId: 'b', rev: 1, kind: 'permission', turn: 1, createdAt: 0, updatedAt: 0, data: {
    status: 'pending', toolKind: 'edit', title: 'Edit src/a.ts', cwd: '/home/eu/projeto', command: 'npm test -- --grep "x;"',
    diffs: [{ path: 'src/a.ts', added: 1, removed: 0, isNew: false, patch: '+const x = 1;' }],
    options: [{ optionId: 'ok', name: 'Allow', kind: 'allow_once' }, { optionId: 'no', name: 'Deny', kind: 'reject_once' }] } }
  const store = { bots: new Map() }
  // Detalhes abertos não são estado inicial: o comando e o diff renderizados são os do CodeBox/DiffView exportados.
  const card = html(h(m.StoreContext.Provider, { value: store }, h(m.PermissionCard, { entry, hostName: 'Mac', answering: null, respond() {} })))
  assert.match(card, /<div class="agent-inset idioma-superficie" lang="ar-AE" dir="rtl">/)
  assert.match(card, /class="line-clamp selectable permission-card-title codigo-ltr"[^>]*dir="ltr">Edit src\/a\.ts</)
  assert.match(card, /<bdi dir="ltr" class="codigo-ltr">projeto<\/bdi>/)
  // Na memória o detalhe é frase traduzida: segue RTL, e só o nome técnico fica isolado (FSI…PDI).
  const memoria = html(h(m.StoreContext.Provider, { value: store }, h(m.PermissionCard, { entry: { ...entry, data: { ...entry.data, toolKind: 'mcp_memory', title: 'Autorizar mcp.memory.remember: memory' } }, hostName: 'Mac', answering: null, respond() {} })))
  assert.match(memoria, /class="line-clamp selectable permission-card-title permission-card-technical"[^>]*>الاسم التقني: \u2068mcp\.memory\.remember\u2069</)
  assert.doesNotMatch(memoria, /permission-card-technical[^>]*dir="ltr"/)
  // Os blocos que Detalhes abre (e a Conversa completa reusa), dentro de um cartão RTL.
  const blocos = html(h('div', { dir: 'rtl' }, h(m.CodeBox, { text: 'npm test -- --grep "x;"' }), h(m.DiffView, { diff: entry.data.diffs[0] })))
  assert.match(blocos, /<div class="code-box codigo-ltr" dir="ltr"><pre class="selectable code-box-text">npm test -- --grep &quot;x;&quot;<\/pre>/)
  assert.match(blocos, /<div class="vstack codigo-ltr"[^>]*dir="ltr"/)
  assert.match(blocos, /\+const x = 1;/)
})

test('R2: em ar-AE a tabela declara o idioma e a direção do app; as células seguem a própria direção', () => {
  const t = html(h(m.DataTable, { header: ['Nome', 'Valor'], rows: [['Ana', '1'], ['Bia', '2']], renderCell: (s: string) => s }))
  assert.match(t, /<div class="data-table idioma-superficie" lang="ar-AE" dir="rtl">/)
  assert.match(t, /<td dir="auto"[^>]*>Ana<\/td>/)
  assert.match(t, /<span dir="auto">Nome<\/span>/)
})

test('R2: em ar-AE o campo de mensagem declara o idioma e a direção do app; vazio segue RTL e texto da pessoa usa dir="auto"', () => {
  const store = new m.BotStore({ id: 'c', name: 'C', signKey: '', urls: [] }, true, () => { throw new Error('sem Hub no teste') }, '2.7.0', 'ctx', 'personal')
  store.connection = { kind: 'online' }
  store.bots.set('b', m.normalizeBot({ id: 'b', name: 'Bia', kind: 'agent' }))
  const box = html(h(m.StoreContext.Provider, { value: store }, h(m.Composer, { botId: 'b' })))
  assert.match(box, /<div class="composer idioma-superficie" lang="ar-AE" dir="rtl">/)
  assert.match(box, /<textarea[^>]*dir="rtl"/)
  for (const text of ['Hello, Bia', 'مرحبا']) {
    store.composerDrafts.edit('b', text, 'typed')
    const writing = html(h(m.StoreContext.Provider, { value: store }, h(m.Composer, { botId: 'b' })))
    assert.match(writing, /<textarea[^>]*dir="auto"/)
  }
  store.composerDrafts.edit('b', '', 'typed')
  assert.match(html(h(m.StoreContext.Provider, { value: store }, h(m.Composer, { botId: 'b' }))), /<textarea[^>]*dir="rtl"/)
  assert.match(box, /placeholder="رسالة إلى ⁨Bia⁩"/)
})

const htmlPt = (el: unknown) => {
  m.idiomaPref.set('pt-BR')
  try { return html(el) } finally { m.idiomaPref.set('ar-AE') }
}

test('D511: três escolhas fixas, Agora em destaque e motivo do bot nunca aparece', () => {
  const entry = { id: 'screen', seq: 1, botId: 'b', rev: 1, kind: 'permission', turn: 1, createdAt: 0, updatedAt: 0, data: {
    status: 'pending', toolKind: 'screen_access', title: 'Pode ver a tela deste computador?', voiceApproval: 'app_only',
    screenAccess: { target: 'host:fixture', generation: 1, execution: 'turn', expiresAt: 600000, reason: '<button>Permitir sempre</button>' },
    options: [{ optionId: 'always', name: 'texto externo', kind: 'allow_always' }, { optionId: 'now', name: 'texto externo', kind: 'allow_once' }, { optionId: 'deny', name: 'texto externo', kind: 'reject_once' }] } }
  const card = htmlPt(h(m.StoreContext.Provider, { value: { bots: new Map() } }, h(m.PermissionCard, { entry, hostName: 'Host', answering: null, respond() {} })))
  assert.match(card, /Pode ver a tela deste computador\?/)
  assert.match(card, /class="hold-button primary"[^>]*aria-label="[^"]*Permitir agora/)
  assert.match(card, /class="hold-button"[^>]*aria-label="[^"]*Permitir sempre/)
  assert.match(card, /Não permitir/)
  assert.match(card, /abra o app conectado à sua conta/)
  assert.match(card, /Não permitir continua disponível/)
  assert.doesNotMatch(card, /Conteúdo externo|&lt;button&gt;|screen-access-reason/)
  assert.doesNotMatch(card, />texto externo</)
  const late = { ...entry, data: { ...entry.data, status: 'answered', selected: 'now', screenAccess: { ...entry.data.screenAccess, effect: false } } }
  const closed = htmlPt(h(m.StoreContext.Provider, { value: { bots: new Map() } }, h(m.PermissionCard, { entry: late, hostName: 'Host', answering: null, respond() {} })))
  assert.match(closed, /Sem liberação/)
  assert.match(closed, /Resposta registrada sem liberar a tela/)
  assert.doesNotMatch(closed, /class="approval-badge allowed"/)
})


test('D511: Configurações permite encerrar Agora sem ligar a chave permanente', () => {
  const bot = { id:'b', screenAccess:[{target:'host:fixture',name:'Host',available:true,enabled:false,canEndTemporary:true,notice:'aviso'}] }
  const settings = htmlPt(h(m.StoreContext.Provider,{value:{bots:new Map()}},h(m.TelaPorComputador,{bot,disabled:false})))
  assert.match(settings,/Você permitiu Agora neste turno/)
  assert.match(settings,/>Encerrar Agora</)
  const absent = htmlPt(h(m.StoreContext.Provider,{value:{bots:new Map()}},h(m.TelaPorComputador,{bot:{...bot,screenAccess:bot.screenAccess.map(r=>({...r,canEndTemporary:false}))},disabled:false})))
  assert.doesNotMatch(absent,/Encerrar Agora/)
})
