// DON-917: sem idioma salvo, a tela sai em inglês (D454); os textos conferidos aqui são os do catálogo `en`.
import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

// MB-1b R3 (B1, D70): o fluxo real do Board. O TorrePage de verdade é renderizado ao vivo, depois sem conexão
// (o item da pergunta desmonta) e ao vivo de novo, com o MESMO store de rascunhos: o campo volta com o texto e a
// falha do envio, e Tentar de novo usa o MESMO nonce. Depois que o Hub registra e a pergunta sai da lista, o
// rascunho some.
const bundle = await build({
  stdin: {
    contents: `export { TorrePage } from './views/fork/torre/TorreView';
      export { QuestionDrafts, submitDraft, keepDraftsFor } from './views/fork/torre/rascunhos-pergunta';
      export { responderOf } from './views/fork/torre/responder';
      export { parseTowerSnapshot } from '../shared/fork-torre';
      export { originDraft } from '../shared/guard-hub';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  alias: { '@shared': new URL('../../../../shared', import.meta.url).pathname },
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }, { name: 'rosto', setup(b) {
    // O rosto lê preferências por store externo (sem snapshot de servidor): fica um marcador vazio.
    b.onResolve({ filter: /\/fork\/avatars\/Avatar$/ }, () => ({ path: 'rosto', namespace: 'rosto' }))
    b.onLoad({ filter: /.*/, namespace: 'rosto' }, () => ({ contents: 'export function DonnabotAvatar() { return null }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
const storage = { getItem: () => null, setItem() {}, removeItem() {} }
const module = { exports: {} as Record<string, any> }
const realm = { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, crypto, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask }
runInNewContext(bundle.outputFiles[0]!.text, realm)
const m = module.exports

const U = { availability: 'unavailable' }
const question = { entryId: 'q1', conversation: { conversationId: 'rota', revision: 3 }, text: 'Reservo o hotel do centro?', askedAt: 5 }
const snapshot = (questions: unknown[]) => m.parseTowerSnapshot({
  version: 1, hubId: 'h', snapshotId: 's', collectedAt: 1, spaceId: 'p',
  bots: [{ botId: 'rota', name: 'Rota', runtime: { availability: 'available', source: 's', coverage: 'c', value: { status: 'needsInput' } },
    pending: U, queue: U, usage: {}, open: null,
    questions: { availability: 'available', source: 'hub.runtime.pendingQuestions', coverage: 'c', value: questions } }],
}, { hubId: 'h', spaceId: 'p' })

// Como o BotStore: o send grava a entrada local `sending` com o nonce; a queda/erro a marca `failed`.
const entries: any[] = []
const sends: { text: string; nonce: string }[] = []
const store = {
  allEntries: () => entries, respond() {}, answering: new Map(), hostName: 'Servidor',
  send: (text: string, chat: string, opts: { nonce: string }) => {
    sends.push({ text, nonce: opts.nonce })
    entries.push({ id: `local-${opts.nonce}`, botId: chat, kind: 'user', data: { text, status: 'sending', clientNonce: opts.nonce } })
  },
  discard: (e: unknown) => { entries.splice(entries.indexOf(e), 1) },
  subscribe: () => () => {},
}
const responder = m.responderOf(store, () => 'aparelho')
const drafts = new m.QuestionDrafts()
const render = (snap: unknown, stale: boolean) => m.renderToStaticMarkup(m.createElement(m.TorrePage, {
  session: { phase: { kind: 'ready' }, snapshot: snap, link: null, events: stale ? 'paused' : 'live', offlineSince: null, notice: null, stale },
  spaceName: 'Pessoal', computerName: 'Servidor', personalSpaceId: 'p',
  onNavigate() {}, onRetry() {}, onTimes() {}, onUsage() {}, responder, drafts,
})) as string

test('rascunho sobrevive à desmontagem e à reconexão; retry com o mesmo nonce; some quando a pergunta sai', () => {
  const live = snapshot([question])
  // O dono digita (o mesmo set do onChange) e envia (o mesmo submit do formulário).
  drafts.set('q1', { botId: 'rota', text: 'Reserva o do centro.', origin: m.originDraft('Reserva o do centro.', 'typed') })
  assert.match(render(live, false), /Reserva o do centro\./)
  m.submitDraft(responder, drafts, 'q1', 'rota')
  const nonce = sends[0]!.nonce
  assert.match(render(live, false), /Sending…/)

  // Sem conexão: o item da pergunta desmonta. O envio falha nesse meio tempo.
  const offline = render(live, true)
  assert.doesNotMatch(offline, /torre-question/)
  entries[0].data.status = 'failed'

  // Volta a conexão com a pergunta ainda pendente: mesmo texto, o aviso da falha e Tentar de novo.
  const back = render(live, false)
  assert.match(back, /<textarea[^>]*>Reserva o do centro\.<\/textarea>/)
  assert.match(back, /It wasn’t sent/)
  assert.match(back, /Try again/)
  m.submitDraft(responder, drafts, 'q1', 'rota')
  assert.equal(sends.length, 2)
  assert.equal(sends[1]!.nonce, nonce, 'o MESMO nonce: o Hub reconhece a mensagem se a primeira tentativa chegou')
  assert.equal(entries.length, 1, 'uma entrada só no chat')

  // O Hub registra e tira a pergunta da lista: o rascunho é limpo.
  entries[0].data.status = 'queued'
  m.keepDraftsFor(drafts, snapshot([]))
  assert.deepEqual({ ...drafts.get('q1') }, { botId: null, text: '', origin: undefined, nonce: null })
})

// D341 (DON-896, D70): o Board real não lista bot parado nem sem estado, não conta Livre e, sem nada listado,
// mostra só o vazio calmo, sem painel e sem contadores; sem conexão, o mesmo filtro.
test('Board sem bots parados: fora da lista e dos números; vazio calmo', () => {
  const rt = (status: string) => ({ availability: 'available', source: 's', coverage: 'c', value: { status } })
  const card = (botId: string, name: string, runtime: unknown) => ({ botId, name, runtime, pending: U, queue: U, usage: {}, open: null })
  const snap = (bots: unknown[]) => m.parseTowerSnapshot({ version: 1, hubId: 'h', snapshotId: 's', collectedAt: 1, spaceId: 'p', bots,
    routineCreations: { availability: 'available', source: 'hub', coverage: 'space', value: { items: [], blocked: [] } },
    attention: { availability: 'available', source: 'hub', coverage: 'space', value: { version: 1, hubId: 'h', spaceId: 'p', collectedAt: 1, items: [], resolvedToday: [], coverageGaps: [] } },
  }, { hubId: 'h', spaceId: 'p' })
  const parados = [card('livre', 'Parado', rt('idle')), card('mudo', 'Mudo', U)]
  const comErro = snap([...parados, card('erro', 'Planejador', rt('error'))])
  for (const stale of [false, true]) {
    const html = render(comErro, stale)
    assert.match(html, /Planejador/)
    assert.doesNotMatch(html, /Stopped|Muted|Free|No information/)
    assert.match(html, /torre-counts/)
    assert.doesNotMatch(html, /<b>0<\/b>/)
  }
  assert.match(render(comErro, false), /didn’t report the state of 1 bot/)
  for (const stale of [false, true]) {
    const html = render(snap(parados), stale)
    assert.match(html, stale ? /Its state is unconfirmed\./ : /Nothing is waiting for you right now\./)
    assert.doesNotMatch(html, /torre-counts|torre-detail|Stopped|Muted|Free/)
  }
})
