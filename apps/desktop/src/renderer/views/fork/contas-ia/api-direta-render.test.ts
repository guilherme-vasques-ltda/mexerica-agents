// Renderização (SSR) da linha de API direta com a capacidade falsa: o botão de entrar pelo navegador existe só no
// OpenRouter, ao lado do de colar a chave, e o bloco de resultado mostra a frase de cada falha do login. O fluxo
// completo (chamar o Hub, esperar o retorno, cancelar) é regra pura e está em openrouter-login.test.ts.
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { test } from 'node:test'
import { build, type Plugin } from 'esbuild'

// Os hooks de estado do app (`useSyncExternalStore` sem snapshot de servidor) ganham, só neste bundle, o `getSnapshot`
// também no servidor; sem DOM, o resto é o código real. (Mesmo arranjo dos testes de render de Ajustes.)
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'react-servidor' ? undefined : { path: 'react', namespace: 'react-servidor' }))
    b.onLoad({ filter: /.*/, namespace: 'react-servidor' }, () => ({
      contents: `const R = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))});
        module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
  },
}
const raiz = new URL('../../../', import.meta.url).pathname
const bundle = await build({
  stdin: {
    contents: `export { LinhaDoFornecedor, ResultadoDoTeste } from './views/fork/contas-ia/ApiDireta';
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
const window: Record<string, unknown> = {
  addEventListener: nada, removeEventListener: nada, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800,
  codync: { platform: 'darwin', appScheme: 'donnabot', appVersion: '0.0.0', hostPort: 0, ssh: {}, island: { supported: false } },
}
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty: nada } }, body: { setAttribute: nada }, addEventListener: nada, removeEventListener: nada, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'pt-BR' }, fetch: () => new Promise(() => {}), console, setTimeout, clearTimeout, queueMicrotask,
  structuredClone, TextEncoder, URLSearchParams, URL, Intl,
  __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  LinhaDoFornecedor: typeof import('./ApiDireta').LinhaDoFornecedor
  ResultadoDoTeste: typeof import('./ApiDireta').ResultadoDoTeste
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

/** A loja de mentira: só o que a linha lê para desenhar; as chamadas ao Hub só acontecem nos cliques. */
const store = { computer: { id: 'computador-fixture' }, connection: { kind: 'online' }, isLoopback: true, hello: { hostId: 'hub-fixture', backends: [{ id: 'mexerica' }] } }
const renderiza = (componente: unknown, props: Record<string, unknown> = {}) =>
  m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never }, m.createElement(componente as never, props)))

const fornecedor = (id: string, configurada: boolean) => ({ id, nome: id === 'openrouter' ? 'OpenRouter' : 'Fornecedor Exemplo', configurada, modelos: configurada ? [] : null, conferidoEm: null })

test('o botão de entrar pelo navegador existe só na linha do OpenRouter, ao lado do de colar a chave', () => {
  const entrar = 'Entrar com OpenRouter'
  const semChave = renderiza(m.LinhaDoFornecedor, { fornecedor: fornecedor('openrouter', false), aoMudar: async () => {} })
  assert.ok(semChave.includes(entrar), 'OpenRouter sem chave tem o botão')
  assert.ok(semChave.includes('Adicionar chave'), 'o caminho de colar a chave segue ao lado')
  assert.ok(!renderiza(m.LinhaDoFornecedor, { fornecedor: fornecedor('openai', false), aoMudar: async () => {} }).includes(entrar), 'outro fornecedor não tem')
  const comChave = renderiza(m.LinhaDoFornecedor, { fornecedor: fornecedor('openrouter', true), aoMudar: async () => {} })
  assert.ok(comChave.includes(entrar) && comChave.includes('Trocar'), 'com chave salva o botão segue ao lado de Trocar')
})

test('o bloco de resultado mostra a frase da falha do login e continua igual para as chaves coladas', () => {
  assert.ok(renderiza(m.ResultadoDoTeste, { resultado: { ok: false, frase: 'contas.apiDireta.openrouter.erro.cancelado' } }).includes('O login com o OpenRouter foi cancelado.'))
  assert.ok(renderiza(m.ResultadoDoTeste, { resultado: { ok: false, frase: 'contas.apiDireta.openrouter.erro.tempo' } }).includes('O login com o OpenRouter demorou demais.'))
  // O mesmo bloco das chaves coladas: erro do fornecedor e sucesso seguem com as frases de antes.
  assert.ok(renderiza(m.ResultadoDoTeste, { resultado: { ok: false, erro: 'chave_invalida' } }).includes('O fornecedor recusou a chave.'))
  assert.ok(renderiza(m.ResultadoDoTeste, { resultado: { ok: true, modelos: [{ id: 'x/a', name: 'Modelo A' }] } }).includes('A chave funciona.'))
})
