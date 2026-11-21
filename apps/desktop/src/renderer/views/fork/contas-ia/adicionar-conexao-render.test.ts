// Renderização (SSR) da tela «Adicionar conexão» com o código real: Planos e Chaves de API na edição completa, e só o
// Codex, sem chaves e sem consulta ao cofre de chaves, na edição Open (a ponte do app diz qual é a edição).
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import test from 'node:test'
import { build, type Plugin } from 'esbuild'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'

const pt = CATALOGOS['pt-BR'] as Record<string, string>
let efeitos: (() => void)[] | undefined

const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'react-servidor' ? undefined : { path: 'react', namespace: 'react-servidor' }))
    b.onLoad({ filter: /.*/, namespace: 'react-servidor' }, () => ({
      contents: `const R = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))});
        module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g), useEffect: agendarEfeito };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
const raiz = new URL('../../../', import.meta.url).pathname
const bundle = await build({
  stdin: {
    contents: `export { AdicionarConexao } from './views/fork/contas-ia/AdicionarConexao';
      export { ApiDireta } from './views/fork/contas-ia/ApiDireta';
      export { NovaConta } from './views/fork/contas-ia/NovaConta';
      export { chavesMudaram } from './views/fork/contas-ia/api-direta';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: raiz, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  resolveExtensions: ['.mjs', '.js', '.mts', '.ts', '.jsx', '.tsx', '.json', '.css'],
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const salvo = new Map([['appLanguage', JSON.stringify('pt-BR')]])
const storage = { getItem: (k: string) => salvo.get(k) ?? null, setItem() {}, removeItem() {} }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const nada = () => {}
const semParar = () => nada
const window: Record<string, unknown> = {
  addEventListener: nada, removeEventListener: nada, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800,
}
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty: nada } }, body: { setAttribute: nada }, addEventListener: nada, removeEventListener: nada, hidden: false }
window.codync = { platform: 'darwin', appScheme: 'donnabot', appVersion: '0.0.0', hostPort: 0, ssh: {}, island: { supported: false } }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  agendarEfeito: (efeito: () => void) => { efeitos?.push(efeito) },
  navigator: { platform: 'MacIntel', language: 'pt-BR' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  structuredClone, crypto: globalThis.crypto, requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  AdicionarConexao: typeof import('./AdicionarConexao').AdicionarConexao
  ApiDireta: typeof import('./ApiDireta').ApiDireta
  NovaConta: typeof import('./NovaConta').NovaConta
  chavesMudaram: typeof import('./api-direta').chavesMudaram
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const observavel = { subscribe: () => semParar, getVersion: () => 0 }
const fornecedores = [{ id: 'anthropic', nome: 'Anthropic', configurada: false, modelos: null, conferidoEm: null }]

/** Um Hub de mentira: registra cada método chamado e responde a lista de chaves sem consultar ninguém. */
function cenario(edicao: string, extra: { isLoopback?: boolean } = {}) {
  window.codync = { platform: 'darwin', appScheme: edicao, appVersion: '0.0.0', hostPort: 0, ssh: {}, island: { supported: false } }
  const chamadas: string[] = []
  const store = {
    ...observavel, computer: { id: 'fixture' }, connection: { kind: 'online' }, mismatch: false, isLoopback: extra.isLoopback ?? true, hostName: 'Fixture',
    hello: { hostId: 'h1', backends: [{ id: 'codex' }, { id: 'claude' }, { id: 'mexerica' }] },
    client: { call: async (metodo: string) => { chamadas.push(metodo); return { fornecedores } } },
  }
  const contas = { ...observavel, accounts: [], loading: false, busy: null }
  const renderiza = (componente: unknown, props: Record<string, unknown> = {}) =>
    m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never }, m.createElement(componente as never, props)))
  const rodaEfeitos = async (componente: unknown, props: Record<string, unknown> = {}) => {
    efeitos = []
    try {
      renderiza(componente, props)
      for (const efeito of efeitos) efeito()
    } finally {
      efeitos = undefined
    }
    await new Promise(setImmediate)
  }
  return { chamadas, renderiza, rodaEfeitos, contas: { ...contas } }
}
const nadaProps = (contas: unknown) => ({ contas, onPlano: nada, onClose: nada })

test('edição completa: busca, Planos com Codex e Claude Code, e a lista de chaves vem do Hub', async () => {
  const c = cenario('donnabot')
  const html = c.renderiza(m.AdicionarConexao, nadaProps(c.contas))
  assert.ok(html.includes(pt['contas.conexoes.titulo']!))
  assert.ok(html.includes(pt['contas.conexoes.buscar']!))
  assert.ok(html.includes(pt['contas.conexoes.planos']!))
  assert.match(html, /Codex/)
  assert.match(html, /Claude Code/)
  assert.ok(html.includes(pt['contas.conexoes.conectar']!))
  assert.ok(html.includes(pt['contas.conexoes.chaves']!), 'o grupo de chaves já aparece (carregando) antes da lista chegar')
  await c.rodaEfeitos(m.AdicionarConexao, nadaProps(c.contas))
  assert.deepEqual(c.chamadas, ['motorChaves'])
})

test('Open: só o Codex, sem grupo de chaves e sem consultar o cofre de chaves, mesmo com o motor Mexerica no Hub', async () => {
  const c = cenario('mexerica-open')
  const html = c.renderiza(m.AdicionarConexao, nadaProps(c.contas))
  assert.match(html, /Codex/)
  assert.doesNotMatch(html, /Claude/)
  assert.ok(!html.includes(pt['contas.conexoes.chaves']!), 'sem o grupo Chaves de API')
  await c.rodaEfeitos(m.AdicionarConexao, nadaProps(c.contas))
  await c.rodaEfeitos(m.ApiDireta)
  assert.deepEqual(c.chamadas, [], 'a Open não pergunta ao Hub por chaves')
  assert.ok(!c.renderiza(m.ApiDireta).includes(pt['contas.apiDireta.titulo']!), 'a seção de chaves salvas não aparece na Open')
})

test('Hub remoto: planos sim, chaves não (o cadastro de chave é só deste computador)', async () => {
  const c = cenario('donnabot', { isLoopback: false })
  await c.rodaEfeitos(m.AdicionarConexao, nadaProps(c.contas))
  assert.deepEqual(c.chamadas, [])
  assert.match(c.renderiza(m.AdicionarConexao, nadaProps(c.contas)), /Codex/)
})

test('Open: o passo seguinte, «Nova conta», também só oferece o Codex e nenhuma chave de API', () => {
  const props = (c: ReturnType<typeof cenario>) => ({ contas: c.contas, initialEngine: 'claude', onCreated: nada, onClose: nada })
  const open = cenario('mexerica-open')
  const html = open.renderiza(m.NovaConta, props(open))
  assert.match(html, /Codex/)
  assert.doesNotMatch(html, /Claude/)
  assert.ok(!html.includes(pt['contas.porChave']!), 'sem a opção de chave de API')
  const completa = cenario('donnabot')
  const todos = completa.renderiza(m.NovaConta, props(completa))
  assert.match(todos, /Claude Code/)
  assert.ok(todos.includes(pt['contas.porChave']!), 'a edição completa mantém a chave de API')
})

test('uma chave salva numa tela avisa a outra: a seção da página relê o Hub', async () => {
  const c = cenario('donnabot')
  await c.rodaEfeitos(m.ApiDireta)
  assert.deepEqual(c.chamadas, ['motorChaves'])
  m.chavesMudaram()
  await new Promise(setImmediate)
  assert.deepEqual(c.chamadas, ['motorChaves', 'motorChaves'])
})

test('a seção de chaves salvas aparece na edição completa', () => {
  const c = cenario('donnabot')
  assert.ok(c.renderiza(m.ApiDireta).includes(pt['contas.apiDireta.titulo']!))
})
