import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build, type Plugin } from 'esbuild'
import { market, MCP_REGISTRY_UNAVAILABLE } from '../views/marketplace/market-models.ts'
import { CATALOGOS, traduzir } from './fork-i18n/index.ts'
import { codigoOuDescricao, detalheDe, selosDe } from '../views/fork/meus-mcp/estado.ts'
import { cru, frase, textoDe } from './fork-i18n/frase.ts'

// D454 (fatia 8a): Extensões no idioma escolhido, com as lições das R1 do #565, #568 e #572: código e caminho da
// esquerda para a direita e isolados; lang/dir na tela e na raiz dos portais; conteúdo do usuário em dir="auto";
// cancelar traduzido; erro escrito pela interface guardado como chave e traduzido ao mostrar.

test('o registro MCP retirado devolve a mensagem fixa em inglês', () => {
  assert.match(MCP_REGISTRY_UNAVAILABLE, /^mcp_registry_unavailable: The MCP Registry was retired\./)
})

test('nenhum texto de extensoes.* traz comando, caminho ou URL dentro da frase do catálogo', () => {
  const codigo = /~\/|\/etc\/|`|https?:\/\/|npx /
  const achados = Object.entries(CATALOGOS).flatMap(([idioma, catalogo]) =>
    Object.entries(catalogo).filter(([k, v]) => k.startsWith('extensoes.') && codigo.test(String(v))).map(([k]) => `${idioma}: ${k}`))
  assert.deepEqual(achados, [])
})

test('R1 do #572: o erro que a interface escreve fica como chave (+ parâmetros), nunca como frase pronta', () => {
  const fonte = (a: string) => readFileSync(new URL(`../views/marketplace/${a}`, import.meta.url), 'utf8')
  for (const arquivo of ['ConnectorSheets.tsx', 'ConnectionRequestCard.tsx']) {
    const s = fonte(arquivo)
    // D507: setError só recebe frase(chave, params) ou cru(errorText(e)) (texto do Hub), nunca texto montado aqui.
    assert.doesNotMatch(s, /setError\(\s*\{/, arquivo)
    assert.doesNotMatch(s, /setError\(\s*[`'"]/, arquivo)
    assert.doesNotMatch(s, /setError\(\s*t\(/, arquivo)
    assert.doesNotMatch(s, /throw new Error\(\s*[`'"]/, arquivo)
  }
})

// Renderização no servidor das folhas reais, em árabe. Só neste bundle: hooks de estado com snapshot de servidor, o
// portal renderiza no lugar e o painel de categorias já abre (não há DOM no Node).
const reactNoServidor: Plugin = {
  name: 'react-no-servidor',
  setup(b) {
    const real = (m: string) => JSON.stringify(createRequire(import.meta.url).resolve(m))
    b.onResolve({ filter: /^react$/ }, (args) => (args.namespace === 'shim' ? undefined : { path: 'react', namespace: 'shim' }))
    b.onResolve({ filter: /^react-dom$/ }, (args) => (args.namespace === 'shim' ? undefined : { path: 'react-dom', namespace: 'shim' }))
    b.onLoad({ filter: /^react$/, namespace: 'shim' }, () => ({
      contents: `const R = require(${real('react')}); module.exports = { ...R, useSyncExternalStore: (s, g, gs) => R.useSyncExternalStore(s, g, gs ?? g) };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
    b.onLoad({ filter: /^react-dom$/, namespace: 'shim' }, () => ({
      contents: `const D = require(${real('react-dom')}); module.exports = { ...D, createPortal: (filhos) => filhos };`,
      loader: 'js', resolveDir: new URL('.', import.meta.url).pathname,
    }))
    b.onLoad({ filter: /views[\\/]bots[\\/]parts\.tsx$/ }, (args) => {
      const fonte = readFileSync(args.path, 'utf8')
      const de = 'const [rect, setRect] = useState<DOMRect | null>(null)'
      if (!fonte.includes(de)) throw new Error('teste desatualizado: AnchoredPanel')
      return { contents: fonte.replace(de, 'const [rect, setRect] = useState<DOMRect | null>({ top: 0, left: 0, bottom: 20, right: 100, width: 100, height: 20 } as DOMRect)'), loader: 'tsx' }
    })
  },
}
const bundle = await build({
  stdin: {
    contents: `export { CustomConnectorSheet } from './views/marketplace/ConnectorSheets';
      export { AnchoredPanel } from './views/bots/parts';
      export { ItemRow } from './views/marketplace/pieces';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
  alias: { '@shared': new URL('../../shared', import.meta.url).pathname },
  loader: { '.css': 'empty', '.png': 'empty', '.jpg': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const salvo = new Map([['appLanguage', JSON.stringify('ar-AE')]])
const storage = { getItem: (k: string) => salvo.get(k) ?? null, setItem() {}, removeItem() {} }
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'darwin', app: {} }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage, innerWidth: 1200, innerHeight: 800 }
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr', dataset: {}, style: { setProperty() {} } }, body: { setAttribute() {} }, addEventListener() {}, removeEventListener() {}, hidden: false }
const module = { exports: {} }
runInNewContext(bundle.outputFiles[0]!.text, {
  module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage,
  navigator: { platform: 'MacIntel', language: 'ar-AE' }, fetch: () => new Promise(() => {}), TextEncoder, URLSearchParams, URL, Intl, console, setTimeout, clearTimeout, queueMicrotask,
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  CustomConnectorSheet: typeof import('../views/marketplace/ConnectorSheets').CustomConnectorSheet
  AnchoredPanel: typeof import('../views/bots/parts').AnchoredPanel
  ItemRow: typeof import('../views/marketplace/pieces').ItemRow
  StoreContext: typeof import('../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}
const store = {
  subscribe: () => () => {}, getVersion: () => 0, client: null, connectedClient: null, hostName: 'مكتب سارة',
  hello: null, principalEpoch: 0, ready: () => new Promise(() => {}),
}
const comStore = (filho: unknown) => m.createElement(m.StoreContext.Provider, { value: store as never }, filho as never)

test('Adicionar MCP em árabe: exemplos de código e campo de comando da esquerda para a direita; computador isolado', () => {
  const html = m.renderToStaticMarkup(comStore(m.createElement(m.CustomConnectorSheet)))
  assert.match(html, /class="idioma-superficie"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  assert.match(html, /placeholder="npx -y @scope\/server"[^>]*dir="ltr"|dir="ltr"[^>]*placeholder="npx -y @scope\/server"/)
  assert.match(html, /placeholder="API_KEY=…"[^>]*dir="ltr"|dir="ltr"[^>]*placeholder="API_KEY=…"/)
  // O nome do MCP é do usuário: vazio, o campo herda o RTL da tela (o placeholder em árabe fica certo); com texto, dir="auto".
  assert.match(html, /<input(?![^>]*dir=)[^>]*placeholder="الاسم"/)
  assert.match(html, /يعمل على [\u2068]+مكتب سارة[\u2069]+/, 'o computador entra isolado na frase do rodapé')
  assert.match(html, /title="إغلاق"/)
  assert.doesNotMatch(html, /title="Fechar"/)
})

test('R1-02 do #577: selo e orientação de Meus MCP são chaves traduzidas ao desenhar; comando e URL ficam como vieram', () => {
  const manual = { id: 'm', name: 'X', description: 'Desc', command: null, url: null, kind: 'stdio', keys: [] } as never
  const remoto = { id: 'r', name: 'Y', description: '', command: null, url: 'https://mcp.example/x', kind: 'remote', auth: 'signedOut', keys: [] } as never
  assert.deepEqual(selosDe(manual).map((s) => s.chave), ['extensoes.mcp.selo.manual'])
  assert.deepEqual(detalheDe(manual), { chave: 'extensoes.mcp.detalhe.manual' })
  assert.deepEqual(detalheDe(remoto), { codigo: 'https://mcp.example/x' })
  for (const idioma of ['en', 'ar-AE'] as const) {
    for (const k of ['extensoes.mcp.selo.manual', 'extensoes.mcp.selo.entrar', 'extensoes.mcp.detalhe.manual'] as const) {
      assert.notEqual(traduzir(idioma, k), traduzir('pt-BR', k), `${idioma}: ${k}`)
    }
  }
  const fonte = readFileSync(new URL('../views/marketplace/MarketplaceView.tsx', import.meta.url), 'utf8')
  assert.match(fonte, /label=\{t\(s\.chave\)\}/)
  assert.doesNotMatch(fonte, /label=\{s\.rotulo\}/)
})

test('R2-01 do #577: comando e URL salvos saem LTR isolados, mesmo começando em árabe; descrição em dir="auto"', () => {
  const comando = 'مساعد --porta 8080'
  assert.deepEqual(codigoOuDescricao({ command: comando, url: null, description: 'وصف' } as never), { codigo: comando })
  assert.deepEqual(codigoOuDescricao({ command: null, url: null, description: 'وصف' } as never), { dado: 'وصف' })
  const linha = (subtitleCodigo: boolean, subtitle: string) => m.renderToStaticMarkup(m.createElement(m.ItemRow, { title: 'X', subtitle, subtitleCodigo, icon: null }))
  assert.match(linha(true, comando), new RegExp(`class="[^"]*bidi-isolado[^"]*"[^>]* dir="ltr" title="${comando}">${comando}<`))
  assert.match(linha(false, 'وصف'), /dir="auto" title="وصف">وصف</)
  assert.doesNotMatch(linha(false, 'وصف'), /dir="ltr"/)
})

test('R1 do #572 (b): o painel ancorado abre num portal com lang/dir do idioma na raiz; sem language, legado', () => {
  const props = { open: true, onClose: () => {}, anchor: { current: null }, children: 'x' }
  const traduzido = m.renderToStaticMarkup(m.createElement(m.AnchoredPanel, { ...props, language: { lang: 'ar-AE', dir: 'rtl' } }))
  assert.match(traduzido, /^<div[^>]*class="anchored[^"]*idioma-superficie"[^>]*lang="ar-AE"[^>]*dir="rtl"/)
  assert.doesNotMatch(m.renderToStaticMarkup(m.createElement(m.AnchoredPanel, props)), /lang=|dir=/)
  // E a chamada real passa o idioma: o menu de troca de computador.
  const fonte = (a: string) => readFileSync(new URL(`../views/marketplace/${a}`, import.meta.url), 'utf8')
  assert.match(fonte('MarketplaceView.tsx'), /<DropdownMenu[\s\S]{0,200}language=\{superficieIdioma\(idioma\)\}/)
  // As duas confirmações de remover passam o cancelar traduzido e o idioma.
  const dialogos = fonte('MarketplaceView.tsx').split('<Dialog').slice(1).map((d) => d.slice(0, d.indexOf('/>')))
  assert.equal(dialogos.length, 2)
  for (const d of dialogos) {
    assert.match(d, /cancel=\{t\('extensoes\.cancelar'\)\}/)
    assert.match(d, /language=\{superficieIdioma\(idioma\)\}/)
  }
})

test('R1 do #572 (a) e D507: o erro guardado é Frase (dado); sai no idioma da tela e o texto cru do Hub fica como veio', () => {
  const erro = frase('extensoes.pedido.abrirParaEntrar', { titulo: 'Gmail' })
  assert.deepEqual(structuredClone(erro), erro)
  assert.match(textoDe(erro, 'ar-AE'), /^افتح [⁨]+Gmail[⁩]+ في «الإضافات» لتسجيل الدخول\.$/)
  assert.match(textoDe(erro, 'en'), /Gmail/)
  assert.equal(textoDe(cru('boom'), 'ar-AE'), 'boom')
  assert.equal(textoDe(null, 'ar-AE'), undefined)
})


test('OpenCode Go: slot global não recebe spaceId e cabe no limite do relay', async () => {
  const calls: { method: string; timeout: number | undefined; body: unknown }[] = []
  const call = async (method: string, body: unknown, timeout?: number) => {
    calls.push({ method, body, timeout })
    assert.ok(timeout !== undefined && timeout >= 1000 && timeout <= 120_000)
    return { signedIn: null, methods: [], savedEnv: [] }
  }
  const client = {
    boundHubId: 'fixture-hub',
    spaceId: 'fixture-space',
    call: (method: string, body: object, timeout?: number) => call(method, { ...body, spaceId: 'fixture-space' }, timeout),
    transport: { call },
  } as unknown as import('../client/host-client').HostClient
  await market.agentAuth(client, 'opencode')
  await market.setAgentEnv(client, 'opencode', { OPENCODE_API_KEY: 'fixture' })
  await market.setAgentEnv(client, 'opencode', { OPENCODE_API_KEY: '' })
  assert.deepEqual(calls.map((c) => c.method), ['agentAuth', 'engineGoKeySave', 'engineGoKeySave'])
  for (const { body } of calls.slice(1)) {
    assert.equal((body as Record<string, unknown>).spaceId, undefined)
    assert.equal((body as Record<string, unknown>).hubId, 'fixture-hub')
  }
  await market.setAgentEnv(client, 'opencode', { OUTRA_CHAVE: 'fixture' })
  assert.equal(calls.at(-1)?.method, 'setAgentEnv')
})
