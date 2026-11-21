// Renderização (SSR) da seção «Plano do ChatGPT» (Conexões de IA): os quatro estados da conta (sem conta,
// conectado com o plano, conectado sem o plano, precisa entrar de novo), o aviso de saída sem confirmação e a
// seção que só existe com o Hub deste computador e o motor ligado. O fluxo completo (chamar o Hub, esperar o
// retorno, cancelar) é regra pura e está em chatgpt-login.test.ts.
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'
import { build, type Plugin } from 'esbuild'

// Os hooks de estado do app (`useSyncExternalStore` sem snapshot de servidor) ganham, só neste bundle, o `getSnapshot`
// também no servidor; sem DOM, o resto é o código real. (Mesmo arranjo dos testes de render da API direta.)
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'react-servidor' ? undefined : { path: 'react', namespace: 'react-servidor' }))
    b.onLoad({ filter: /.*/, namespace: 'react-servidor' }, () => ({
      contents: `const R = require(${JSON.stringify(createRequire(import.meta.url).resolve('react'))});
        module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: fileURLToPath(new URL('.', import.meta.url)),
    }))
  },
}
const raiz = fileURLToPath(new URL('../../../', import.meta.url))
const bundle = await build({
  stdin: {
    contents: `export { EntrarComChatGPT, PlanoDoChatGPT } from './views/fork/contas-ia/EntrarComChatGPT';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: raiz, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  resolveExtensions: ['.mjs', '.js', '.mts', '.ts', '.jsx', '.tsx', '.json', '.css'],
  alias: { '@shared': fileURLToPath(new URL('../../../../shared', import.meta.url)) },
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
  EntrarComChatGPT: typeof import('./EntrarComChatGPT').EntrarComChatGPT
  PlanoDoChatGPT: typeof import('./EntrarComChatGPT').PlanoDoChatGPT
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

/** A loja de mentira: só o que a seção lê para desenhar; as chamadas ao Hub só acontecem nos cliques. */
const loja = (extra: Record<string, unknown> = {}) => ({
  computer: { id: 'computador-fixture' }, connection: { kind: 'online' }, mismatch: false, isLoopback: true,
  hello: { hostId: 'hub-fixture', backends: [{ id: 'mexerica' }] }, ...extra,
})
const renderiza = (componente: unknown, props: Record<string, unknown> = {}, store: Record<string, unknown> = loja()) =>
  m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never }, m.createElement(componente as never, props)))

const nadaFn = () => {}
const plano = (estado: Record<string, unknown> | null, extra: Record<string, unknown> = {}) =>
  renderiza(m.PlanoDoChatGPT, { estado, ocupado: false, erro: null, avisoDeSair: false, entrar: nadaFn, ativar: nadaFn, sair: nadaFn, cancelar: nadaFn, ...extra })

test('sem conta: só o botão de entrar com o ChatGPT, sem marca de terceiros', () => {
  const html = plano({ conectado: false, planoHabilitado: false, precisaEntrar: false })
  assert.ok(html.includes('Entrar com ChatGPT'), 'o botão de entrar existe')
  assert.ok(!html.includes('Plano habilitado'), 'sem selo de plano')
  assert.ok(!html.includes('Sair'), 'sem sair sem conta')
})

test('conectado com o plano: o rótulo da conta, o selo de plano habilitado e o botão de sair', () => {
  const html = plano({ conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true, precisaEntrar: false })
  assert.ok(html.includes('pessoa@example.invalid'), 'o rótulo da conta aparece')
  assert.ok(html.includes('Plano habilitado'), 'o selo do plano aparece')
  assert.ok(html.includes('Sair'), 'o botão de sair existe')
  assert.ok(!html.includes('Ativar o uso do plano'), 'não oferece ativar o que já está ativo')
})

test('conectado sem o plano: o aviso e o botão de ativar o uso do plano', () => {
  const html = plano({ conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: false, precisaEntrar: false })
  assert.ok(html.includes('O uso do plano não está habilitado neste app.'), 'o aviso aparece')
  assert.ok(html.includes('Ativar o uso do plano'), 'o botão de ativar existe')
  assert.ok(html.includes('Sair'), 'o botão de sair existe')
})

test('precisa entrar de novo: o aviso e o botão de entrar de novo, sem o de sair', () => {
  const html = plano({ conectado: false, planoHabilitado: false, precisaEntrar: true })
  assert.ok(html.includes('A conta precisa entrar de novo.'), 'o aviso aparece')
  assert.ok(html.includes('Entrar de novo'), 'o botão de entrar de novo existe')
  assert.ok(!html.includes('>Sair<'), 'sem conta conectada não há o que sair')
})

test('ocupado: aguardando o navegador e cancelar, sem os botões da conta', () => {
  const html = plano({ conectado: false, planoHabilitado: false, precisaEntrar: false }, { ocupado: true })
  assert.ok(html.includes('Aguardando o navegador…'), 'o aviso de espera aparece')
  assert.ok(html.includes('Cancelar'), 'o botão de cancelar existe')
  assert.ok(!html.includes('Entrar com ChatGPT'), 'o botão de entrar some enquanto espera')
})

test('conectado com outra conta disponível: o botão de entrar com outra conta aparece ao lado do sair', () => {
  const html = plano({ conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true, precisaEntrar: false }, { outraConta: nadaFn })
  assert.ok(html.includes('Entrar com outra conta'), 'o botão de outra conta existe')
})

test('saindo: sem o aviso de aguardar o navegador e sem o botão de cancelar', () => {
  const html = plano({ conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: true, precisaEntrar: false }, { ocupado: true, saindo: true })
  assert.ok(!html.includes('Aguardando o navegador…'), 'sair não mostra espera de navegador')
  assert.ok(!html.includes('Cancelar'), 'sair não oferece cancelar')
})

test('concluindo: a frase de conclusão aparece e o botão de cancelar some', () => {
  const html = plano({ conectado: false, planoHabilitado: false, precisaEntrar: false }, { ocupado: true, concluindo: true })
  assert.ok(html.includes('Concluindo o login…'), 'a frase de conclusão aparece')
  assert.ok(!html.includes('Cancelar'), 'não há o que cancelar depois do retorno')
})

test('conectado fora do Space pessoal: só o «Sair» fica; entrar, ativar e outra conta somem', () => {
  const html = plano({ conectado: true, rotulo: 'pessoa@example.invalid', planoHabilitado: false, precisaEntrar: false }, { foraDoPessoal: true, outraConta: nadaFn })
  assert.ok(html.includes('>Sair<'), 'o «Sair» continua ao alcance')
  assert.ok(!html.includes('Ativar o uso do plano'), 'ativar pede o Space pessoal')
  assert.ok(!html.includes('Entrar com outra conta'), 'trocar de conta pede o Space pessoal')
  assert.ok(html.includes('vale só para os bots do Pessoal'), 'o motivo aparece na tela')
})

test('sair sem confirmação: a frase avisa e aponta as Configurações do ChatGPT', () => {
  const html = plano(null, { avisoDeSair: true })
  assert.ok(html.includes('Saiu deste computador. A OpenAI não confirmou o encerramento; você pode desconectar o app em Configurações do ChatGPT.'))
})

test('a seção existe com o Hub deste computador e o motor ligado, e some nos outros casos', () => {
  const secao = renderiza(m.EntrarComChatGPT)
  assert.ok(secao.includes('Plano do ChatGPT'), 'com Hub local e motor ligado a seção aparece')
  assert.ok(secao.includes('chatgpt.com/settings/usage'), 'o rodapé do limite compartilhado aparece')
  assert.equal(renderiza(m.EntrarComChatGPT, {}, loja({ isLoopback: false })), '', 'Hub de outro computador: sem seção')
  assert.equal(renderiza(m.EntrarComChatGPT, {}, loja({ hello: { hostId: 'hub-fixture', backends: [] } })), '', 'motor desligado: sem seção')
})
