import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// Renderer real, só o rosto/ícone são substituídos como nas provas MB-1b existentes.
const bundle = await build({
  stdin: { contents: `export { TorrePage } from './views/fork/torre/TorreView';
    export { parseTowerSnapshot } from '../shared/fork-torre';
    export { idiomaPref } from './lib/fork-i18n/estado';
    export { createElement } from 'react'; export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'marcadores', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icon', namespace: 'marker' }))
    b.onResolve({ filter: /\/fork\/avatars\/Avatar$/ }, () => ({ path: 'avatar', namespace: 'marker' }))
    b.onLoad({ filter: /.*/, namespace: 'marker' }, () => ({ contents: 'export function Icon() { return null }; export function DonnabotAvatar() { return null }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const module = { exports: {} as Record<string, any> }
runInNewContext(bundle.outputFiles[0]!.text, { module, exports: module.exports, require: createRequire(import.meta.url),
  window: { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 },
  matchMedia: media, localStorage: { getItem: () => null, setItem() {}, removeItem() {} }, crypto, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask })
const m = module.exports
const observed = (value: unknown) => ({ availability: 'available', source: 'hub', coverage: 'space', value })
const item = { entryId: 'n1', entryRev: 3, routineId: 'r1', botId: 'b1', botName: 'Jardineira', name: 'Cuidar das plantas',
  createdAt: 1_700_000_000_000, when: 'Toda sexta às 9h', nextRunAt: null, status: 'active' }
const block = { botId: 'b1', botName: 'Jardineira', limit: 10, used: 10, retryAt: 1_700_086_400_000,
  reason: 'As dez criações via MCP foram usadas.', howToIncrease: 'Trabalho que ninguém pediu permitirá aumentar este limite quando estiver disponível.', settingsAvailable: false }
const render = (value: unknown, opts: { stale?: boolean; callback?: boolean; link?: string; phase?: string; language?: string } = {}) => {
  m.idiomaPref.set(opts.language ?? 'pt-BR')
  const snapshot = m.parseTowerSnapshot({ version: 1, hubId: 'h', spaceId: 's', snapshotId: 'ss', collectedAt: 1,
    bots: [{ botId: 'b1', name: 'Jardineira', usage: {}, runtime: observed({ status: 'idle' }) }], routineCreations: observed(value) }, { hubId: 'h', spaceId: 's' })
  return m.renderToStaticMarkup(m.createElement(m.TorrePage, {
    session: { snapshot, phase: { kind: opts.phase ?? 'ready' }, link: opts.link ?? 'live', events: 'live', stale: opts.stale ?? false, notice: null, offlineSince: null },
    spaceName: 'Jardim', computerName: 'Computador', personalSpaceId: 's', onNavigate() {}, onRetry() {}, onTimes() {}, onUsage() {},
    onReviewRoutine: opts.callback === false ? null : async () => ({}),
  })) as string
}

test('criação de bot livre e diagnóstico de cota impedem o vazio enganoso do Board', () => {
  const html = render({ items: [item], blocked: [] })
  assert.match(html, /Rotinas criadas pelos bots/)
  assert.match(html, /Cuidar das plantas/)
  assert.match(html, /Já está ativa\. Sem resposta, continua\./)
  assert.doesNotMatch(html, /Nada precisa de você agora/)
  assert.doesNotMatch(html, /disabled=""[^>]*>Manter/)
  const quota = render({ items: [], blocked: [block] })
  assert.match(quota, /As dez criações via MCP foram usadas/)
  assert.match(quota, /Trabalho que ninguém pediu permitirá/)
  assert.doesNotMatch(quota, /Nada precisa de você agora|>Manter<|>Cancelar<|href=/)
})

test('fotografia antiga, offline ou callback ausente não oferecem ações utilizáveis', () => {
  for (const opts of [{ stale: true }, { link: 'offline' }, { callback: false }]) {
    const html = render({ items: [item], blocked: [] }, opts)
    assert.match(html, /disabled=""[^>]*>Manter/)
    assert.match(html, /disabled=""[^>]*>Cancelar/)
  }
  assert.doesNotMatch(render({ items: [item], blocked: [] }, { phase: 'moved' }), /Cuidar das plantas|>Manter</)
  assert.match(render({ items: [{ ...item, status: 'paused' }], blocked: [] }), /Manter não reativa/)
  assert.match(render({ items: [], blocked: [] }), /Nada precisa de você agora/)
  const malformed = render({ items: [{ ...item, status: 'desconhecido' }], blocked: [] })
  assert.match(malformed, /O computador não informou as rotinas/)
  assert.doesNotMatch(malformed, /Nada precisa de você agora/)
})

test('rotina externa pendente oferece Ligar e permanece indisponível offline', () => {
  const value = { items: [{ ...item, status: 'paused', externalReleasePending: true, routineRevision: 'a'.repeat(64) }], blocked: [] }
  const html = render(value)
  assert.match(html, />Ligar</)
  assert.doesNotMatch(html, />Manter</)
  assert.match(html, /sem executar agora/)
  assert.match(render(value, { link: 'offline' }), /disabled=""[^>]*>Ligar/)
})

// Limite zero é bloqueio configurado; retryAt não significa que a criação será liberada.
test('limite zero não promete nova tentativa em uma data', () => {
  const html = render({ items: [], blocked: [{ ...block, limit: 0, used: 0 }] })
  assert.match(html, /Criações pelo bot estão bloqueadas/)
  assert.doesNotMatch(html, /Nova tentativa:/)
  assert.match(render({ items: [], blocked: [block] }), /Nova tentativa:/)
})

// Usa useT e a preferência reais: trocar o idioma não deixa a seção presa ao português.
test('seção traduzida nos seis idiomas, incluindo direção árabe', () => {
  const titles = {
    'pt-BR': 'Rotinas criadas pelos bots', en: 'Routines created by bots', es: 'Rutinas creadas por los bots',
    fr: 'Routines créées par les bots', 'zh-CN': '机器人创建的例程', 'ar-AE': 'الروتينات التي أنشأتها الروبوتات',
  }
  for (const [language, title] of Object.entries(titles)) {
    const html = render({ items: [item], blocked: [block] }, { language })
    assert.ok(html.includes(title), language)
    assert.ok(html.includes(`lang="${language}" dir="${language === 'ar-AE' ? 'rtl' : 'ltr'}"`), language)
    if (language !== 'pt-BR') assert.doesNotMatch(html, /Já está ativa|criações pelo bot nas últimas/)
  }
  assert.match(render({ items: [], blocked: [block] }), /criações pelo bot nas últimas 24 horas/)
})
