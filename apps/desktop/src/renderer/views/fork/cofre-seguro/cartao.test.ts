import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import { ehPedidoSeguro } from './estado.ts'

// MX-209: as telas de verdade (React, sem Hub) renderizadas em cada estado. Prova que o cartão da conversa NÃO tem
// campo de valor (pendente, guardado, cancelado) e que o valor só tem campo na janela segura (type=password).
const bundle = await build({
  stdin: {
    contents: `export { PedidoSeguro } from './views/fork/cofre-seguro/PedidoSeguro';
      export { CartaoSeguro, CaixasDeCodigo } from './views/fork/cofre-seguro/JanelaSegura';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon(p) { return null }', loader: 'js' }))
  } }, { name: 'loja', setup(b) {
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'modelo', namespace: 'modelo' }))
    b.onLoad({ filter: /.*/, namespace: 'modelo' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }; export const useModel = (m) => m; export const useModels = () => {}; export const useSelector = (m, f) => f(m); export const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i])); export const useSelectMany = (ms, f) => f(); export const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
    const real = createRequire(import.meta.url).resolve('react')
    b.onResolve({ filter: /^react$/ }, (a) => (a.importer.includes('/node_modules/') || a.namespace === 'react-shim' ? { path: real } : { path: 'react', namespace: 'react-shim' }))
    b.onLoad({ filter: /.*/, namespace: 'react-shim' }, () => ({ resolveDir: '/', loader: 'js', contents: `const R = require(${JSON.stringify(real)}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) }` }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const loja = { bots: new Map([['berga', { name: 'Berga' }]]), client: null, connection: { kind: 'online' }, isLoopback: true, computer: { id: 'pc-1' } }
const sandbox = {
  module: { exports: {} as Record<string, unknown> }, exports: {}, require: createRequire(import.meta.url),
  window: { codync: { platform: 'linux', forkCofre: { onEstado: () => () => {} } }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, location: { search: '' } },
  matchMedia: media, localStorage: { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} },
  TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask, Intl,
}
runInNewContext(bundle.outputFiles[0]!.text, sandbox)
const m = sandbox.module.exports as {
  PedidoSeguro: typeof import('./PedidoSeguro').PedidoSeguro
  CartaoSeguro: typeof import('./JanelaSegura').CartaoSeguro
  CaixasDeCodigo: typeof import('./JanelaSegura').CaixasDeCodigo
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const entrada = (kind: 'login' | 'secret' | 'app' | 'code', status: 'pending' | 'ready' | 'cancelled') => ({
  id: 'e-1', botId: 'berga', updatedAt: 1_700_000_000_000,
  data: { text: 'Preciso entrar para continuar.', connectionRequest: {
    kind, status, title: kind === 'login' ? 'github.com' : kind === 'code' ? 'GitHub conta A' : 'Pagamentos', ...(kind === 'login' ? { site: 'github.com' } : kind === 'code' ? { site: 'github.com', loginId: 'github-com-abc123' } : kind === 'secret' ? { field: 'API_KEY', location: 'env', connectorId: 'c1' } : { toolkit: 'stripe' }),
  } },
})
const cartao = (kind: 'login' | 'secret' | 'app' | 'code', status: 'pending' | 'ready' | 'cancelled') =>
  m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: loja as never }, m.createElement(m.PedidoSeguro, { entry: entrada(kind, status) as never })))
const semCampo = (html: string) => {
  assert.equal(/<input/i.test(html), false, 'o cartão da conversa não tem <input>')
  assert.equal(/<textarea/i.test(html), false)
  assert.equal(/type="password"/i.test(html), false)
  assert.equal(/copiar|copy/i.test(html), false, 'nunca há botão Copiar')
}

test('pedido pendente de login e de chave: sem campo de valor; botões e rodapé da proposta', () => {
  for (const kind of ['login', 'secret'] as const) {
    const html = cartao(kind, 'pending')
    semCampo(html)
    assert.match(html, /Esperando você/)
    assert.match(html, kind === 'login' ? /Digitar a senha com segurança/ : /Digitar a chave com segurança/)
    assert.match(html, /Agora não/)
    assert.match(html, /No cofre da Mexerica, só para o Berga/)
    assert.match(html, /nunca aparece na conversa/i)
  }
})

test('guardado: ✓ verde, nome da senha/chave, sem campo, sem valor, sem Copiar', () => {
  for (const kind of ['login', 'secret'] as const) {
    const html = cartao(kind, 'ready')
    semCampo(html)
    assert.match(html, /Guardado no cofre/)
    assert.match(html, kind === 'login' ? /github\.com/ : /API_KEY/)
  }
})

test('cancelado: sem campo e sem botão de digitar', () => {
  for (const kind of ['login', 'secret'] as const) {
    const html = cartao(kind, 'cancelled')
    semCampo(html)
    assert.equal(/Digitar a/.test(html), false)
  }
})

test('só login e chave viram pedido seguro; agente, app e conector seguem pelo cartão de antes', () => {
  assert.equal(ehPedidoSeguro(entrada('login', 'pending').data.connectionRequest as never), true)
  assert.equal(ehPedidoSeguro(entrada('secret', 'pending').data.connectionRequest as never), true)
  assert.equal(ehPedidoSeguro(entrada('code', 'pending').data.connectionRequest as never), true, 'o código de login usa o mesmo pedido seguro')
  assert.equal(ehPedidoSeguro(entrada('app', 'pending').data.connectionRequest as never), false)
  assert.equal(ehPedidoSeguro({ kind: 'agent' } as never), false)
  assert.equal(ehPedidoSeguro({ kind: 'connector' } as never), false)
})

const pedidoJanela = (tipo: 'login' | 'secret') => ({ pedido: { tipo, titulo: tipo === 'login' ? 'github.com' : 'Pagamentos', site: tipo === 'login' ? 'github.com' : null, campo: tipo === 'login' ? null : 'API_KEY', local: tipo === 'login' ? null : 'env', botNome: 'Berga' } })

test('janela segura: o valor entra num <input type=password>; olho fechado; avisos e botões da proposta', () => {
  for (const tipo of ['login', 'secret'] as const) {
    const html = m.renderToStaticMarkup(m.createElement(m.CartaoSeguro, { dados: pedidoJanela(tipo) }))
    assert.equal((html.match(/type="password"/g) ?? []).length, 1)
    assert.equal(/type="text"[^>]*autofocus/i.test(html), false)
    assert.match(html, /Nunca aparece na conversa\./)
    assert.match(html, /Ligada a/)
    assert.match(html, /Cancelar/)
    assert.match(html, /Salvar/)
    assert.match(html, /Mostrar/)
    assert.match(html, /O bot recebe só o nome/)
    // Usuário e rótulo só no login.
    assert.equal(/Usuário|usuário/.test(html), tipo === 'login')
  }
})

test('MX-277: a janela do login novo abre VAZIA, com site, usuário, senha e nome; sem "ligada a" nem pedido do bot', () => {
  const dados = { pedido: { tipo: 'loginNovo', titulo: '', site: null, campo: null, local: null, botNome: '' } }
  const html = m.renderToStaticMarkup(m.createElement(m.CartaoSeguro, { dados: dados as never }))
  assert.equal((html.match(/type="password"/g) ?? []).length, 1, 'a senha só tem <input type=password>')
  assert.match(html, /Guardar uma senha no cofre/)
  assert.match(html, /Cole aqui a senha que você colou na conversa/)
  assert.match(html, /Site/)
  assert.match(html, /Usuário/)
  assert.match(html, /Nome \(opcional\)/)
  assert.equal(/Ligada a/.test(html), false, 'não há bot nem conta ligada ainda')
  assert.equal(/pediu/.test(html), false)
  assert.match(html, /Nunca aparece na conversa\./)
  assert.match(html, /você escolhe quais bots podem usá-la/)
  // Nenhum campo vem preenchido: o trecho escondido nunca chegou aqui.
  for (const campo of html.match(/<input[^>]*>/g) ?? []) assert.equal(/value="[^"]+"/.test(campo), false, campo)
})

test('MX-286: cartão do código de login — sem campo, com título do login, botão "Digitar o código com segurança" e validade', () => {
  const html = cartao('code', 'pending')
  semCampo(html)
  assert.match(html, /Esperando você/)
  assert.match(html, /O Berga precisa de um código de login/)
  assert.match(html, /GitHub conta A/, 'o título do login vem do Hub')
  assert.match(html, /github\.com/)
  assert.match(html, /Digitar o código com segurança/)
  assert.match(html, /Agora não/)
  assert.match(html, /Uma vez só, por 5 minutos\. Não fica guardado\./)
  assert.match(html, /O código não aparece na conversa\./)
  assert.equal(/Fica guardada|No cofre da Mexerica/.test(html), false, 'o código não fica guardado')
  assert.equal(/nunca (vê|ve)|never sees|sem ver/i.test(html), false, 'não promete que o bot nunca vê o código')
})

test('MX-286: "Código entregue" e cancelado do código de login — sem campo, sem "Ver no cofre", sem valor', () => {
  const pronto = cartao('code', 'ready')
  semCampo(pronto)
  assert.match(pronto, /Código entregue/)
  assert.match(pronto, /vale uma vez só, por 5 minutos, e não ficou guardado/)
  assert.equal(/Ver no cofre|Senha de /.test(pronto), false, 'não há nome guardado nem atalho para o cofre')
  assert.equal(/\d{6}/.test(pronto), false, 'nenhum código aparece')
  const cancelado = cartao('code', 'cancelled')
  semCampo(cancelado)
  assert.match(cancelado, /Cancelado/)
  assert.match(cancelado, /precisa de um código de login/)
})

test('MX-286: janela segura em modo código — 6 caixinhas, um só <input> numérico, "Usar código", validade, sem olho nem "ligada a"', () => {
  const dados = { pedido: { tipo: 'code', titulo: 'GitHub conta A', site: 'github.com', campo: null, local: null, botNome: 'Berga' } }
  const html = m.renderToStaticMarkup(m.createElement(m.CartaoSeguro, { dados: dados as never }))
  assert.equal((html.match(/class="cofre-digito[ "]/g) ?? []).length, 6, 'seis caixinhas vazias')
  assert.equal((html.match(/<input/g) ?? []).length, 1, 'um só campo de verdade, por cima das caixas')
  assert.match(html, /inputMode="numeric"|inputmode="numeric"/)
  assert.match(html, /autoComplete="one-time-code"|autocomplete="one-time-code"/)
  assert.equal(/type="password"/.test(html), false, 'o código é um número curto: sem campo de senha')
  assert.equal(/Mostrar|Esconder/.test(html), false, 'sem olho')
  assert.match(html, /Código de login de github\.com/)
  assert.match(html, /O Berga pediu o código de login de github\.com/)
  assert.match(html, /Usar código/)
  assert.match(html, /Vale uma vez só, por 5 minutos\./)
  assert.match(html, /Não fica guardado\./)
  assert.equal(/Ligada a|Usuário|Nome \(opcional\)/.test(html), false)
  assert.equal(/nunca (vê|ve)|never sees|sem ver/i.test(html), false, 'não promete que o bot nunca vê o código')
  assert.match(html, /<button[^>]*disabled[^>]*>[^<]*Usar código/, 'sem código, "Usar código" fica desligado')
})

test('MX-286: as caixinhas desenham o que está no campo e crescem até o tamanho do código colado (4 a 10)', () => {
  const caixas = (valor: string) => m.renderToStaticMarkup(m.createElement(m.CaixasDeCodigo, { valor, aoMudar() {}, rotulo: 'Código', digito: (n: number, total: number) => `${n}/${total}` }))
  const conta = (html: string) => (html.match(/class="cofre-digito[ "]/g) ?? []).length
  assert.equal(conta(caixas('')), 6)
  assert.equal(conta(caixas('1234')), 6, 'um código de 4 dígitos cabe nas 6 caixas')
  assert.equal(conta(caixas('48291350')), 8, 'colar 8 dígitos abre 8 caixas')
  assert.equal(conta(caixas('1234567890')), 10)
  const cheio = caixas('481')
  assert.equal((cheio.match(/cofre-digito cheia/g) ?? []).length, 3)
  assert.equal((cheio.match(/cofre-digito atual/g) ?? []).length, 1, 'a próxima caixa é a atual')
  assert.equal((caixas('1234567890').match(/cofre-digito atual/g) ?? []).length, 0, 'com 10 dígitos não há próxima')
  for (const d of '481') assert.ok(cheio.includes(`>${d}</span>`))
})
