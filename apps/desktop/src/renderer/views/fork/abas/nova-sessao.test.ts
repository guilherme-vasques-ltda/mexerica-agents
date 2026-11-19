import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// D527 (D70: só o fluxo alterado): o "+" de nova sessão fica logo depois da aba do bot, com as sessões mostradas ou
// recolhidas, e só existe com `onNew` (o Hub anuncia `create`+`send` numa conversa de bot do Pessoal). O clique real
// (cria para o bot aberto, abre a aba e põe o cursor na caixa) é provado no Chromium: docs/evidence/d527-pilula-nova-sessao.py.
const bundle = await build({
  stdin: {
    contents: `export { ChatTabs, mainWidth } from './views/fork/abas/ChatTabs';
      export { extrasAllowed } from './views/fork/sessoes/ganchos';
      export { panelPrefs } from './views/fork/painel-direito/prefs';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  // O ícone lê o manifesto do Mac por store externo: vira um marcador com o nome, para conferir qual é.
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: `import { createElement } from 'react'; export function Icon(p) { return createElement('i', { 'data-icon': p.name }) } export function BotAvatar() { return createElement('b') }`, loader: 'js', resolveDir: new URL('../../../', import.meta.url).pathname }))
    // O rosto do bot também lê store externo (humor do avatar): fica um marcador.
    b.onResolve({ filter: /\/components\/Avatar$/ }, () => ({ path: 'rosto', namespace: 'icone' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
const prefs = new Map<string, string>([['appLanguage', '"pt-BR"']])
const storage = { getItem: (k: string) => prefs.get(k) ?? null, setItem: (k: string, v: string) => void prefs.set(k, v), removeItem: (k: string) => void prefs.delete(k) }
const module = { exports: {} }
const realm = { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask }
runInNewContext(bundle.outputFiles[0]!.text, realm)
const m = module.exports as {
  ChatTabs: typeof import('./ChatTabs').ChatTabs
  mainWidth: typeof import('./ChatTabs').mainWidth
  extrasAllowed: typeof import('../sessoes/ganchos').extrasAllowed
  panelPrefs: typeof import('../painel-direito/prefs').panelPrefs
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const bot = { id: 'byte', name: 'Byte', kind: 'agent' } as never
const session = { id: 's1', title: 'Guarda de data', state: 'open', at: 1, revision: 1 } as never
const sessions = { open: [session], archived: [], known: new Set(['s1']), state: () => 'idle', main: 'idle', principal: null, combined: 'idle' } as never
const noop = () => {}
const render = (onNew: { run: () => void; busy: boolean } | null) => m.renderToStaticMarkup(m.createElement(m.ChatTabs, {
  chat: bot, channel: false, sessions, openId: null, closed: new Set<string>(), onOpen: noop, onCloseTab: noop, onTogglePin: noop, onMore: noop, onNew,
}))
// Os filhos diretos do cartão, na ordem: o rascunho de medida fica de fora.
const order = (html: string) => [...html.matchAll(/<(?:button|span) class="(ctab main[^"]*|ctab-add|csep|ctab(?: on)?|tsep|ctab-more|ctab-toggle)"/g)].map((x) => x[1]!.split(' ')[0] === 'ctab' && x[1]!.includes('main') ? 'main' : x[1]!)

for (const [aberta, open] of [['mostradas', true], ['recolhidas', false]] as const) {
  test(`com as sessões ${aberta}, o "+" vem logo depois da aba do bot`, () => {
    m.panelPrefs.tabsOpen.set(open)
    const html = render({ run: noop, busy: false })
    assert.match(html, open ? /^<div class="ctabs"/ : /^<div class="ctabs collapsed"/)
    const kids = order(html)
    assert.deepEqual(kids.slice(0, 2), ['main', 'ctab-add'])
    assert.equal(kids.filter((k) => k === 'ctab-add').length, 1)
    assert.match(html, /<button class="ctab-add" title="Nova sessão com Byte" aria-label="Nova sessão"><i data-icon="plus"><\/i><\/button>/)
  })
}

test('criando: o "+" fica desabilitado e diz que está criando', () => {
  m.panelPrefs.tabsOpen.set(false)
  assert.match(render({ run: noop, busy: true }), /<button class="ctab-add" title="Nova sessão com Byte" aria-label="Criando sessão…" disabled="" aria-busy="true">/)
})

test('sem bot ou sem poder criar, não há "+"', () => {
  for (const open of [true, false]) {
    m.panelPrefs.tabsOpen.set(open)
    assert.doesNotMatch(render(null), /ctab-add/)
  }
  // Sem conversa aberta, ou em canal, `useNewSession` devolve null: o ChatTabs nem recebe `onNew`.
  const store = { spaceId: null, spaces: null } as never
  assert.equal(m.extrasAllowed(store, null), false)
  assert.equal(m.extrasAllowed(store, { id: 'c', name: 'c', kind: 'group', members: [] } as never), false)
})

// R2 do #603 (achado 1): o espaço da aba do bot com o "+" entra no encaixe das abas e na largura da pílula. Em árabe
// (dir rtl) o "+" fica à esquerda do chip; medir `add.right - main.left` dava negativo e cabiam abas demais.
const caixa = (left: number, width: number, visivel = true) => ({
  offsetWidth: width,
  getBoundingClientRect: () => ({ left, right: left + width }),
  getClientRects: () => (visivel ? [{}] : []),
})
const cartao = (main: ReturnType<typeof caixa>, add: ReturnType<typeof caixa> | null) =>
  ({ querySelector: (s: string) => (s === '.ctab.main' ? main : s === '.ctab-add' ? add : null) }) as never

test('a largura do chip com o "+" é a mesma em português e em árabe', () => {
  // Chip de 80 px em 100; "+" de 30 px a 4 px dele, à direita (ltr) ou à esquerda (rtl).
  assert.equal(m.mainWidth(cartao(caixa(100, 80), caixa(184, 30))), 114)
  assert.equal(m.mainWidth(cartao(caixa(100, 80), caixa(66, 30))), 114)
})

test('o "+" escondido (barra flutuando por cima) ou ausente não conta na largura', () => {
  assert.equal(m.mainWidth(cartao(caixa(100, 80), caixa(0, 0, false))), 80)
  assert.equal(m.mainWidth(cartao(caixa(100, 80), null)), 80)
})
