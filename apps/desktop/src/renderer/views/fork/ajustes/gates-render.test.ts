// Renderização (SSR) dos componentes reais de Ajustes com a capacidade falsa e verdadeira: remover o gate de
// qualquer um deles faz estes testes falharem (o predicado puro está em gates-capacidades.test.ts). O app Web é
// `web: true`; o Desktop, `web` ausente com plataforma conhecida (shared/fork-capacidades.ts).
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import test from 'node:test'
import { build, type Plugin } from 'esbuild'
import { CATALOGOS } from '../../../lib/fork-i18n/index.ts'
import { islandLabel } from '../../../../shared/island.ts'

const pt = CATALOGOS['pt-BR'] as Record<string, string>
let efeitos: (() => void)[] | undefined

// Os hooks de estado do app (`useSyncExternalStore` sem snapshot de servidor) ganham, só neste bundle, o `getSnapshot`
// também no servidor; sem DOM, o resto é o código real.
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
    contents: `export { AccountSettingsCard } from './views/fork/account/AccountSettingsCard';
      export { SettingsView, GeneralSettings } from './views/settings/SettingsView';
      export { ComputersView } from './views/settings/ComputersView';
      export { VozPage } from './views/fork/voz/VozPage';
      export { AppContext } from './store/context';
      export { account } from './store/account';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: raiz, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic', logLevel: 'silent', plugins: [reactNoServidor],
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
  requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } },
})
const m = module.exports as {
  AccountSettingsCard: typeof import('../account/AccountSettingsCard').AccountSettingsCard
  SettingsView: typeof import('../../settings/SettingsView').SettingsView
  GeneralSettings: typeof import('../../settings/SettingsView').GeneralSettings
  ComputersView: typeof import('../../settings/ComputersView').ComputersView
  VozPage: typeof import('../voz/VozPage').VozPage
  AppContext: typeof import('../../../store/context').AppContext
  account: typeof import('../../../store/account').account
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

type Aparelho = 'web' | 'desktop'
/** A ponte do aparelho: o Web diz `web: true` (e plataforma `linux`); o Desktop não tem o marcador. */
function aparelho(tipo: Aparelho, extra: Record<string, unknown> = {}) {
  window.codync = {
    platform: tipo === 'web' ? 'linux' : 'darwin', ...(tipo === 'web' ? { web: true } : {}),
    appScheme: 'donnabot', appVersion: '0.0.0', hostPort: 0,
    app: { showInDock: async () => true, launchAtLogin: async () => false },
    island: { supported: false, enabled: async () => false },
    speech: { available: false, locales: () => [], onDeviceUnavailableReason: () => null },
    ssh: {}, accountProfile: {},
    voz: { onChange: semParar, estado: async () => ({}) },
    ...extra,
  }
}

const observavel = { subscribe: () => semParar, getVersion: () => 0 }
const app = (extra: Record<string, unknown> = {}) => ({
  ...observavel, managedStores: [], computers: [], approvals: [], local: null, selectedStore: null, cloud: { ...observavel, computers: [], deviceRegistration: 'idle', isOlderCopy: () => false },
  host: { ...observavel, state: { kind: 'running' } }, spaceMirrors: { active: null, root: () => undefined },
  ...extra,
})
const renderiza = (componente: unknown, props: Record<string, unknown> = {}, contexto: Record<string, unknown> = {}) =>
  m.renderToStaticMarkup(m.createElement(m.AppContext.Provider, { value: app(contexto) as never }, m.createElement(componente as never, props)))

test('Perfil: com a conta entrada, o bloco aparece no Desktop e não no Web (nem a chave crua)', () => {
  m.account.user = { userId: 'u1', email: 'a@b.c' } as never
  m.account.isReady = true
  try {
    aparelho('desktop')
    const desktop = renderiza(m.AccountSettingsCard)
    assert.match(desktop, /account-profile-row/, 'Desktop mostra o perfil')
    aparelho('web')
    const web = renderiza(m.AccountSettingsCard)
    assert.match(web, /account-card/, 'a linha da conta segue no Web')
    assert.doesNotMatch(web, /account-profile-row|webUnavailable/)
    assert.ok(!web.includes(pt['conta.perfil.titulo']!) , 'sem o título Perfil')
  } finally { m.account.user = null }
})

test('Ícone do app: a página só aparece na lateral do Desktop', () => {
  const pagina = pt['aparencia.icone.pagina']!
  aparelho('desktop')
  assert.ok(renderiza(m.SettingsView, { page: 'general', aba: null, onPage: nada, onAba: nada }).includes(pagina), 'Desktop lista a página')
  aparelho('web')
  assert.ok(!renderiza(m.SettingsView, { page: 'general', aba: null, onPage: nada, onAba: nada }).includes(pagina), 'Web não lista')
  // Abrir direto a página no Web cai em Geral (nunca numa página nativa).
  const web = renderiza(m.SettingsView, { page: 'app-icon', aba: null, onPage: nada, onAba: nada })
  assert.match(web, /<h2[^>]*>Geral<\/h2>/)
})

test('Voz: a aba Chaves some no Web sem Hub, e a página abre na primeira (não vazia)', () => {
  const props = { page: 'voice', aba: 'chaves', onPage: nada, onAba: nada }
  aparelho('desktop')
  assert.ok(renderiza(m.SettingsView, props).includes(pt['ajustes.aba.chaves']!), 'Desktop tem a aba Chaves')
  aparelho('web')
  const web = renderiza(m.SettingsView, props)
  assert.ok(!web.includes(pt['ajustes.aba.chaves']!), 'Web não tem a aba Chaves')
  assert.ok(web.includes(pt['ajustes.aba.ouvir']!), 'as demais abas seguem')
})

test('Voz > chaves do computador: o cartão só existe com o cofre do aparelho', () => {
  aparelho('desktop')
  assert.match(renderiza(m.VozPage, { reserva: null, aba: 'chaves' }), /Chaves deste/, 'Desktop mostra as chaves do computador')
  aparelho('web')
  assert.doesNotMatch(renderiza(m.VozPage, { reserva: null, aba: 'chaves' }), /Chaves deste/)
  assert.match(renderiza(m.VozPage, { reserva: null, aba: 'ouvir' }), new RegExp(pt['voz.ajustes.vozParaTexto']!), 'o resto de Voz segue no Web')
})

test('Computadores > Por SSH: a seção e o editor só existem com SSH no aparelho', () => {
  aparelho('desktop')
  const desktop = renderiza(m.ComputersView)
  assert.ok(desktop.includes(pt['ajustes.comp.porSsh']!), 'Desktop mostra Por SSH')
  assert.ok(desktop.includes(pt['ajustes.comp.adicionarSsh']!), 'e o botão de adicionar')
  aparelho('web')
  const web = renderiza(m.ComputersView)
  assert.ok(!web.includes(pt['ajustes.comp.porSsh']!), 'Web não mostra Por SSH')
  assert.ok(!web.includes(pt['ajustes.comp.adicionarSsh']!))
})

test('Abrir ao iniciar a sessão e cartão Sistema: Desktop mostra; Web esconde o item e o cartão vazio', () => {
  const aoIniciar = pt['ajustes.geral.aoIniciar']!, sistema = pt['ajustes.geral.sistema']!
  aparelho('desktop')
  const desktop = renderiza(m.GeneralSettings, { ativa: true })
  assert.ok(desktop.includes(aoIniciar) && desktop.includes(sistema), 'Desktop mostra o item e o cartão')
  aparelho('web')
  const web = renderiza(m.GeneralSettings, { ativa: true })
  assert.ok(!web.includes(aoIniciar), 'Web sem o item')
  assert.ok(!web.includes(sistema), 'Web sem o cartão Sistema vazio')
  // Com outro item do cartão (a ilha), o cartão fica, mas o item nativo continua fora.
  aparelho('web', { island: { supported: true, enabled: async () => false } })
  const comIlha = renderiza(m.GeneralSettings, { ativa: true })
  assert.ok(comIlha.includes(sistema) && !comIlha.includes(aoIniciar))
})

test('Ajustes Gerais não consulta a ilha indisponível e segue consultando quando suportada', () => {
  const rotulo = pt[islandLabel('darwin')]!
  for (const supported of [false, true]) {
    let consultas = 0
    aparelho('desktop', { island: { supported, enabled: async () => { consultas++; return false } } })
    efeitos = []
    try {
      const html = renderiza(m.GeneralSettings)
      for (const efeito of efeitos) efeito()
      assert.equal(consultas, supported ? 1 : 0)
      assert.equal(html.includes(rotulo), supported)
    } finally { efeitos = undefined }
  }
})

test('Open: Dock, início de sessão e ícone não aparecem nem consultam a ponte', () => {
  let consultas = 0
  const app = {
    showInDock: async () => { consultas++; return true },
    launchAtLogin: async () => { consultas++; return false },
  }
  aparelho('desktop', { appScheme: 'mexerica-open', app })
  efeitos = []
  try {
    const geral = renderiza(m.GeneralSettings)
    for (const efeito of efeitos) efeito()
    assert.equal(consultas, 0)
    assert.ok(!geral.includes(pt['ajustes.geral.dock']!))
    assert.ok(!geral.includes(pt['ajustes.geral.aoIniciar']!))
    assert.ok(!geral.includes(pt['ajustes.geral.sistema']!))
    const direto = renderiza(m.SettingsView, { page: 'app-icon', aba: null, onPage: nada, onAba: nada })
    assert.ok(!direto.includes(pt['aparencia.icone.pagina']!))
    assert.match(direto, /<h2[^>]*>Geral<\/h2>/)
  } finally { efeitos = undefined }
})
