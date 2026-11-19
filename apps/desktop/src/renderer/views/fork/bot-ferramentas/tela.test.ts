import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { ABAS, navegacaoInicial, navegar, type Navegacao } from './lista.ts'

// MX-95: a tela de verdade (React, sem Hub), renderizada em cada aba, com busca e em cada página. Prova que navegar
// é só visualização: nenhum gesto de gravação sai, o bot não muda e cada interruptor mostra a seleção do bot.
const bundle = await build({
  stdin: {
    contents: `export { PainelDeFerramentas } from './views/fork/bot-ferramentas/FerramentasEmAbas';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }, { name: 'modelo', setup(b) {
    // Sem DOM não há `getServerSnapshot`: o modelo fictício já está no estado a mostrar.
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'modelo', namespace: 'modelo' }))
    b.onLoad({ filter: /.*/, namespace: 'modelo' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }; export const useModel = (m) => m; export const useModels = () => {}; export const useSelector = (m, f) => f(m); export const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i])); export const useSelectMany = (ms, f) => f(); export const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
    // Hooks do app sem `getServerSnapshot` (preferências, movimento reduzido) usam o snapshot do cliente (igual a
    // views/thread/idioma-direcao.test.ts). Só o React importado pelo app passa pelo shim; o react-dom usa o real.
    const real = createRequire(import.meta.url).resolve('react')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) }` }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, location: { search: '' } }
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask })
const m = module.exports as {
  PainelDeFerramentas: typeof import('./FerramentasEmAbas').PainelDeFerramentas
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const connectors = [
  { id: 'conta-stripe-a', name: 'Pagamentos · Loja A', description: 'Cobranças e assinaturas', kind: 'remote', keys: [] },
  { id: 'conta-stripe-b', name: 'Pagamentos · Loja B', description: 'Cobranças e assinaturas', kind: 'remote', keys: [] },
  ...Array.from({ length: 21 }, (_, i) => ({ id: `c${i + 3}`, name: `Conector ${i + 3}`, description: '', kind: 'remote', url: 'https://mcp.example.invalid', keys: [] })),
]
const logins = Array.from({ length: 17 }, (_, i) => ({ id: `l${i + 1}`, site: `site${i + 1}.example.invalid`, username: 'demo', label: `Login ${i + 1}` }))
const skills = Array.from({ length: 20 }, (_, i) => ({ id: `s${i + 1}`, name: `Skill ${i + 1}`, description: 'Uma descrição longa o bastante para passar de duas linhas na janela mínima do app.', path: `s${i + 1}` }))
const plugins = { connectors, logins, skills, indice: null, retry() {} }
// Ligados em páginas diferentes: o da página 2 (c20) e um id que esta lista nem mostra.
const bot = Object.freeze({ id: 'berga', name: 'Berga', connectors: Object.freeze(['conta-stripe-a', 'c20', 'fora-da-lista']), logins: Object.freeze(['l2', 'l16']), skills: Object.freeze(['s1', 's18']) })

const gestos: string[] = []
const render = (o: { inicial?: Navegacao; online?: boolean; plugins?: unknown; disabled?: boolean } = {}) => m.renderToStaticMarkup(m.createElement(m.PainelDeFerramentas, {
  bot: bot as never, feedback: { errors: {} } as never, disabled: o.disabled,
  save: ((...a: unknown[]) => { gestos.push(`save ${JSON.stringify(a)}`) }) as never,
  saveSkill: ((...a: unknown[]) => { gestos.push(`saveSkill ${JSON.stringify(a)}`) }) as never,
  plugins: (o.plugins === undefined ? plugins : o.plugins) as never, online: o.online ?? true, indice: null,
  sobreSkill: (i) => ({ id: i.id, name: i.name, description: i.detail, origem: null }) as never, inicial: o.inicial,
}))

/** O painel visível e, nele, cada interruptor com o texto que o acompanha. */
const visivel = (html: string) => html.split('role="tabpanel"').slice(1).find((p) => !/^[^>]*hidden=""/.test(p))!
const interruptores = (painel: string) => [...painel.matchAll(/<button class="switch"([^>]*)>(.*?)<span class="switch-track/g)].map(([, attrs, dentro]) => ({
  on: /aria-checked="true"/.test(attrs!), dentro: dentro!,
  nome: attrs!.match(/aria-label="([^"]*)"/)?.[1] ?? dentro!.split(/<[^>]+>/).map((x) => x.trim()).find(Boolean)!,
}))
const nomeParaId = new Map<string, string>([...connectors.map((c) => [c.name, c.id] as const), ...logins.map((l) => [l.label, l.id] as const), ...skills.map((s) => [s.name, s.id] as const)])
const selecaoDe = { connectors: bot.connectors, logins: bot.logins, skills: bot.skills }

test('MX-95: trocar de aba, buscar e paginar não grava nada e cada interruptor mostra a seleção do bot', () => {
  const antes = JSON.stringify(bot)
  const cenas: Navegacao[] = []
  for (const aba of ABAS) {
    let nav = navegar(navegacaoInicial(), { tipo: 'aba', aba })
    cenas.push(nav, navegar(nav, { tipo: 'pagina', pagina: 2 }))
    for (const texto of ['1', 'loja b', 'nada disso']) cenas.push(nav = navegar(nav, { tipo: 'busca', texto }))
  }
  let vistos = 0
  for (const nav of cenas) {
    const painel = visivel(render({ inicial: nav }))
    for (const s of interruptores(painel)) {
      const id = nomeParaId.get(s.nome)
      assert.ok(id, `interruptor sem item conhecido: ${s.nome}`)
      assert.equal(s.on, selecaoDe[nav.aba].includes(id), `${nav.aba} ${JSON.stringify(nav.busca)} p${nav.pagina[nav.aba]}: ${s.nome}`)
      vistos++
    }
  }
  assert.ok(vistos > 60, `poucos interruptores conferidos (${vistos})`)
  assert.deepEqual(gestos, [], 'navegar não chama save nem saveSkill')
  assert.equal(JSON.stringify(bot), antes, 'o bot não muda')
})

test('MX-95: busca sempre à mostra (até com lista vazia); paginação no rodapé com faixa e números', () => {
  const p1 = visivel(render())
  assert.match(p1, /aria-label="Buscar conectores"/)
  assert.match(p1, /1–15 de 23/)
  assert.match(p1, /aria-current="page"[^>]*>1<\/button>/)
  assert.match(p1, /aria-label="Página 2"[^>]*>2<\/button>/)
  assert.match(p1, /aria-label="Página anterior" disabled=""|disabled=""[^>]*aria-label="Página anterior"/)
  assert.ok(p1.indexOf('class="card-section"') < p1.indexOf('class="bf-paginas"'), 'as páginas ficam depois da lista')
  const p2 = visivel(render({ inicial: navegar(navegacaoInicial(), { tipo: 'pagina', pagina: 2 }) }))
  assert.match(p2, /16–23 de 23/)
  const poucos = visivel(render({ plugins: { ...plugins, connectors: connectors.slice(0, 3) } }))
  assert.match(poucos, /aria-label="Buscar conectores"/)
  assert.doesNotMatch(poucos, /bf-paginas/)
  const vazio = visivel(render({ plugins: { ...plugins, connectors: [] } }))
  assert.match(vazio, /aria-label="Buscar conectores"/)
  assert.match(vazio, /class="bf-vazio" role="status"/)
  const semResultado = visivel(render({ inicial: navegar(navegacaoInicial(), { tipo: 'busca', texto: 'planilha fiscal' }) }))
  assert.match(semResultado, /Nada encontrado para “planilha fiscal”/)
})

test('MX-95: duas contas do mesmo conector são duas linhas; "Loja B" acha só a segunda', () => {
  const html = visivel(render({ inicial: navegar(navegacaoInicial(), { tipo: 'busca', texto: 'pagamentos' }) }))
  assert.deepEqual(interruptores(html).map((s) => [s.nome, s.on]), [['Pagamentos · Loja A', true], ['Pagamentos · Loja B', false]])
  const b = visivel(render({ inicial: navegar(navegacaoInicial(), { tipo: 'busca', texto: 'Loja B' }) }))
  assert.deepEqual(interruptores(b).map((s) => s.nome), ['Pagamentos · Loja B'])
})

test('MX-95: abas com tablist, aria-selected, tabpanel e só a escolhida no Tab', () => {
  const html = render({ inicial: navegar(navegacaoInicial(), { tipo: 'aba', aba: 'logins' }) })
  assert.match(html, /role="tablist"/)
  assert.equal(html.match(/role="tab"/g)!.length, 3)
  assert.equal(html.match(/role="tabpanel"/g)!.length, 3)
  assert.match(html, /id="bot-ferramentas-berga-aba-logins" aria-controls="bot-ferramentas-berga-painel-logins" aria-selected="true" tabindex="0"/)
  assert.match(html, /id="bot-ferramentas-berga-aba-connectors" aria-controls="bot-ferramentas-berga-painel-connectors" aria-selected="false" tabindex="-1"/)
  assert.match(visivel(html), /aria-label="Buscar logins"/)
})

test('MX-95: carregando, sem conexão antes de ler e sem conexão com a lista já lida', () => {
  assert.match(render({ plugins: null }), /role="status"[^>]*>Carregando/)
  assert.match(render({ plugins: null, online: false }), /Sem conexão com o Hub\./)
  const offline = render({ online: false, disabled: true })
  assert.match(offline, /Sem conexão: dá para ver e buscar/)
  const painel = visivel(offline)
  assert.match(painel, /aria-label="Buscar conectores"/)
  assert.ok(painel.match(/class="switch"[^>]*disabled=""/g)!.length >= 15, 'só os interruptores desligam')
  assert.doesNotMatch(painel, /class="bf-num[^"]*" (?:aria-current="page" )?aria-label="Página 2" disabled/, 'as páginas seguem')
})

test('MX-95: skill tem descrição em bloco próprio (cortado no CSS) e o interruptor fora do texto', () => {
  const html = visivel(render({ inicial: navegar(navegacaoInicial(), { tipo: 'aba', aba: 'skills' }) }))
  assert.match(html, /<p id="[^"]+-d" class="bf-desc">Uma descrição longa/)
  assert.match(html, /<button class="switch" role="switch" aria-checked="true" aria-label="Skill 1"/)
  const sws = interruptores(html)
  assert.equal(sws.length, 15)
  assert.ok(sws.every((s) => s.dentro === ''), 'nem a descrição nem o "ver mais" ficam dentro do interruptor')
})
