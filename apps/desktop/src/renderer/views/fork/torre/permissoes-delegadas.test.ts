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
const card = { entryId: 'e1', sessionRef: { id: 's1', chatId: 'b2' }, kind: 'permission', status: 'pending', conversation: { conversationId: 'b2', revision: 2 } }
const item = { key: 'k1', requesterBotId: 'b1', targetBotId: 'b2', entryId: 'e1', conversation: { conversationId: 'b2', revision: 2 }, mode: 'wait', requestedAt: 1_000 }
const bots = (pending: unknown) => [
  { botId: 'b1', name: 'Jardineira', usage: {}, runtime: observed({ status: 'idle' }), pending: observed([]) },
  { botId: 'b2', name: 'Cozinheira', usage: {}, runtime: observed({ status: 'idle' }), pending },
]
const render = (delegated: unknown, opts: { stale?: boolean; language?: string; pending?: unknown } = {}) => {
  m.idiomaPref.set(opts.language ?? 'pt-BR')
  const snapshot = m.parseTowerSnapshot({ version: 1, hubId: 'h', spaceId: 's', snapshotId: 'ss', collectedAt: 120_000,
    bots: bots(opts.pending ?? observed([card])), delegatedPermissions: delegated }, { hubId: 'h', spaceId: 's' })
  return m.renderToStaticMarkup(m.createElement(m.TorrePage, {
    session: { snapshot, phase: { kind: 'ready' }, link: 'live', events: 'live', stale: opts.stale ?? false, notice: null, offlineSince: null },
    spaceName: 'Jardim', computerName: 'Computador', personalSpaceId: 's', onNavigate() {}, onRetry() {}, onTimes() {}, onUsage() {},
  })) as string
}
const semIsolados = (s: string) => s.replace(/[\u2066-\u2069]/g, '')
const bloco = (html: string) => semIsolados(/data-permissao="k1"/.test(html) ? html.slice(html.indexOf('data-permissao="k1"'), html.indexOf('</section>', html.indexOf('data-permissao="k1"'))) : '')

test('permissão delegada aparece em Pontas soltas com só Abrir, sem aprovar nem texto do pedido', () => {
  const html = render(observed({ items: [item], omitted: 0 }))
  const b = bloco(html)
  assert.match(html, /Permissões pedidas por outros bots/)
  assert.match(b, /Jardineira pediu a Cozinheira/)
  assert.match(b, /Espera sua permissão/)
  assert.match(b, /esperou resposta · há 1 min/)
  assert.match(b, /title="Abrir a conversa de Cozinheira"/)
  // Nesta linha o único botão é Abrir; o cartão original continua só em "Esperam você".
  assert.equal((b.match(/<button/g) ?? []).length, 1)
  assert.doesNotMatch(b, /Permitir|Negar|Aprovar|Retomar|Responder/)
  assert.match(html, /Nada é aprovado, negado ou retomado daqui/)
})

test('sem o campo, indisponível, vazio, offline ou cartão que já saiu: a seção não aparece', () => {
  const nada = /Permissões pedidas por outros bots/
  assert.doesNotMatch(render(undefined), nada)
  assert.doesNotMatch(render({ availability: 'unavailable' }), nada)
  assert.doesNotMatch(render(observed({ items: [], omitted: 0 })), nada)
  assert.doesNotMatch(render(observed({ items: [item], omitted: 0 }), { stale: true }), nada)
  assert.doesNotMatch(render(observed({ items: [item], omitted: 0 }), { pending: observed([]) }), nada)
  assert.doesNotMatch(render(observed({ items: [item], omitted: 0 }), { pending: { availability: 'unavailable' } }), nada)
  // `omitted` positivo sem nenhuma linha confirmada pela fotografia: nada de "e mais N" solto.
  assert.doesNotMatch(render(observed({ items: [], omitted: 3 })), nada)
  assert.doesNotMatch(render(observed({ items: [item], omitted: 3 }), { pending: observed([]) }), nada)
  // Cartão de outro tipo, já respondido ou de outra revisão não confirma a linha.
  for (const outro of [{ kind: 'question' }, { status: 'answered' }, { conversation: { conversationId: 'b2', revision: 9 } }]) {
    assert.doesNotMatch(render(observed({ items: [item], omitted: 0 }), { pending: observed([{ ...card, ...outro }]) }), nada)
  }
})

test('mais itens que o Hub listou viram "e mais N", em todos os idiomas, sem chave crua', () => {
  const titulos: Record<string, RegExp> = {
    'pt-BR': /Permissões pedidas por outros bots/, en: /Permissions requested by other bots/, es: /Permisos pedidos por otros bots/,
    fr: /Autorisations demandées par d’autres bots/, 'zh-CN': /其他机器人请求的权限/, 'ar-AE': /الأذونات التي طلبتها بوتات أخرى/,
  }
  for (const [language, titulo] of Object.entries(titulos)) {
    const html = render(observed({ items: [item], omitted: 2 }), { language })
    assert.match(html, titulo, language)
    assert.doesNotMatch(bloco(html), /torre\./, language)
    assert.doesNotMatch(html, /torre\.permDeleg/, language)
  }
  assert.match(render(observed({ items: [item], omitted: 2 })), /e mais 2 permissões/)
  assert.match(render(observed({ items: [item], omitted: 1 })), /e mais 1 permissão</)
})
