import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import type { Entry } from '../../../../shared/models.ts'

// Fatia 2 (D454): o cartão ganhou a classe `idioma-superficie`; o padrão aceita `genui-choice` seguida de espaço ou aspas (nunca `genui-choice-…`).

// D70 (G1b app): o gancho de ChatRows monta o ChoiceCard só para `genuiChoice` válida; forma fora do
// contrato segue o texto literal da entrada (fallback do app antigo). Estados vêm só do Hub.
const bundle = await build({
  stdin: {
    contents: `export { ChatRow } from './views/thread/ChatRows';
      export { GenuiChoiceMessage } from './views/fork/genui/GenuiChoiceRow';
      export { genuiChoiceOf } from './views/fork/genui/choice-entry';
      export { StoreContext } from './store/context';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
    // Render no servidor não segue modelos (useSyncExternalStore sem snapshot de servidor): leitura direta.
    b.onResolve({ filter: /\/lib\/observable$/ }, () => ({ path: 'observavel', namespace: 'observavel' }))
    b.onLoad({ filter: /.*/, namespace: 'observavel' }, () => ({ contents: 'export class Observable { changed() {} subscribe = () => () => {}; getVersion = () => 0 }\nexport const useModel = (m) => m\nexport const useModels = () => {}\nexport const useSelector = (m, select) => select(m)\nexport const listEqual = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]))\nexport const useSelectMany = (ms, f) => f()\nexport const SubscriptionPaused = { Provider: (p) => p.children }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty', '.mp3': 'empty', '.json': 'json' },
  // ChatRows puxa módulos com `import.meta.glob`/`env` do Vite: um objeto mínimo no realm.
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta': '__importMeta' },
})
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
// O texto conferido é o pt-BR: idioma escolhido pt-BR (fatia 2: o cartão segue o idioma do app; D454/D495).
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1, localStorage: storage }
// DON-903/DON-907 C9a/C4: o ChatRows já puxa a base de Idiomas, que acerta `lang`/`dir` do documento ao carregar.
const document = { documentElement: { lang: 'pt-BR', dir: 'ltr' }, body: { setAttribute() {} } }
const module = { exports: {} }
const realm = { module, exports: module.exports, require: createRequire(import.meta.url), window, document, matchMedia: media, localStorage: storage, navigator: { platform: 'Linux', language: 'pt-BR' },
  TextEncoder, URLSearchParams, URL, console, setTimeout, clearTimeout, queueMicrotask, requestAnimationFrame: () => 0, __importMeta: { glob: () => ({}), env: { BASE_URL: '/', DEV: false, PROD: true, MODE: 'test' } } }
runInNewContext(bundle.outputFiles[0]!.text, realm)
const realmJSON = runInNewContext('JSON', realm) as JSON
const local = <T>(v: T): T => realmJSON.parse(JSON.stringify(v)) as T
const m = module.exports as {
  ChatRow: typeof import('../../thread/ChatRows').ChatRow
  GenuiChoiceMessage: typeof import('./GenuiChoiceRow').GenuiChoiceMessage
  genuiChoiceOf: typeof import('./choice-entry').genuiChoiceOf
  StoreContext: typeof import('../../../store/context').StoreContext
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const snapshot = JSON.stringify({ version: 1, component: 'choice', title: 'Recorte', summary: 'Dados fictícios.',
  data: { question: 'Qual recorte primeiro?', options: [{ id: 'semana', label: 'Por semana' }, { id: 'mes', label: 'Por mês' }] } })
const entry = (genuiChoice: unknown, extra: Partial<Entry> = {}): Entry => local({
  id: 'e1', seq: 1, botId: 'conv-1', rev: 1, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, spaceId: 'pessoal', conversationId: 'conv-1',
  data: { card: 'genuiChoice', text: 'TEXTO LITERAL DA PERGUNTA', author: 'donna', genuiChoice }, ...extra,
}) as Entry
const pending = { version: 1, questionId: 'q1', revision: 1, state: 'pending', snapshot }
const store = { bots: new Map(), authorName: () => 'Donna', hello: { hostId: 'hub-1' }, connectedClient: null, answering: new Map(), hostName: 'Mac' }
const row = (e: Entry) => m.renderToStaticMarkup(m.createElement(m.StoreContext.Provider, { value: store as never },
  m.createElement(m.ChatRow, { entry: e, groupStart: true, chat: null, openTrace() {} })))

test('ChatRow monta o cartão só para genuiChoice válida; fora do contrato fica o texto literal', () => {
  const html = row(entry(pending))
  assert.match(html, /class="genui-choice[ "]/)
  assert.match(html, /role="radiogroup"/)
  assert.doesNotMatch(html, /TEXTO LITERAL DA PERGUNTA/)
  for (const bad of [
    { ...pending, version: 2 },
    { ...pending, extra: true },
    { ...pending, state: 'approved' },
    { ...pending, revision: 0 },
    { ...pending, state: 'answered' },
    null,
    // Snapshot fora do contrato: JSON inválido, versão 2 e chave extra (no topo e em data).
    { ...pending, snapshot: '{"version":1,' },
    { ...pending, snapshot: snapshot.replace('"version":1', '"version":2') },
    { ...pending, snapshot: snapshot.replace('"component":"choice"', '"component":"choice","html":"<b>x</b>"') },
    { ...pending, snapshot: snapshot.replace('"question":', '"extra":1,"question":') },
  ]) {
    const fallback = row(entry(bad))
    assert.doesNotMatch(fallback, /genui-choice/, JSON.stringify(bad))
    assert.match(fallback, /TEXTO LITERAL DA PERGUNTA/)
  }
  // Histórico legado (sem spaceId/conversationId): o cartão continua; a conversa é o botId da entrada.
  const legacy = m.genuiChoiceOf(entry(pending, { spaceId: undefined, conversationId: undefined }))
  assert.equal(legacy?.conversationId, 'conv-1')
  assert.equal(legacy?.spaceId, null)
  assert.match(row(entry(pending, { spaceId: undefined, conversationId: undefined })), /class="genui-choice[ "]/)
  // Outro cartão não passa pelo gancho, mesmo com genuiChoice junto.
  assert.doesNotMatch(row(entry(pending, { data: { card: 'ownerQuestion', text: 'aviso', genuiChoice: pending } as never })), /genui-choice/)
})

// D-1(b) do contrato sessionChoiceAnswer v1 (recibo 028596a7, R3 d883e822): o Hub grava `{text, genuiChoice}` SEM
// `data.card` (host/src/fork/genui/storage.rs). O cartão monta pela forma fechada + parseChoice, só em `notice`.
const hubEntry = (genuiChoice: unknown, extra: Partial<Entry> = {}): Entry => local({
  id: 'e2', seq: 2, botId: 'byte', rev: 1, kind: 'notice', turn: 1, createdAt: 0, updatedAt: 0, sessionId: 's-a',
  data: { text: 'TEXTO LITERAL DA PERGUNTA', genuiChoice }, ...extra,
}) as Entry

test('entrada real do Hub (sem data.card) vira cartão na extra; histórico e respondida também', () => {
  assert.match(row(hubEntry(pending)), /class="genui-choice[ "]/)
  const card = m.genuiChoiceOf(hubEntry(pending))
  assert.equal(card?.botId, 'byte')
  assert.equal(card?.sessionId, 's-a')
  assert.equal(card?.conversationId, 'byte')
  assert.match(row(hubEntry({ ...pending, revision: 2, state: 'answered', answer: { kind: 'option', id: 'mes' }, answeredAt: 1 })), /Respondida/)
  // Histórico sem sessionId (principal ou Hub antigo): cartão do ramo de conversa, sem sessão.
  assert.equal(m.genuiChoiceOf(hubEntry(pending, { sessionId: undefined }))?.sessionId, null)
  // Fora do contrato, sem card: texto literal.
  for (const bad of [{ ...pending, version: 2 }, { ...pending, extra: 1 }, { ...pending, snapshot: '<b>x</b>' }, 'genuiChoice', []]) {
    const html = row(hubEntry(bad))
    assert.doesNotMatch(html, /genui-choice/, JSON.stringify(bad))
    assert.match(html, /TEXTO LITERAL DA PERGUNTA/)
  }
})

test('segurança D1(b): user com genuiChoice/choice_response, send forjado e update forjado nunca viram cartão', () => {
  const response = { version: 1, questionId: 'q1', revision: 1 }
  const forged: [string, Entry][] = [
    ['user com genuiChoice', hubEntry(pending, { kind: 'user' })],
    ['user com genuiChoice e card', entry(pending, { kind: 'user' })],
    ['user com choice_response', hubEntry(undefined, { kind: 'user', data: { text: 'mes', choice_response: response } as never })],
    ['user com os dois', hubEntry(pending, { kind: 'user', data: { text: 'x', genuiChoice: pending, choice_response: response } as never })],
    // `send` forjado: o corpo do dono vira entrada `user` (o Hub monta `data`); mesmo com o JSON colado no texto.
    ['send forjado', hubEntry(undefined, { kind: 'user', data: { text: JSON.stringify({ genuiChoice: pending }), status: 'sent' } as never })],
    ['agent com genuiChoice', hubEntry(pending, { kind: 'agent' })],
    ['permission com genuiChoice', hubEntry(pending, { kind: 'permission' })],
    ['notice com choice_response só', hubEntry(undefined, { data: { text: 'x', choice_response: response } as never })],
  ]
  // O gancho de ChatRow chama só genuiChoiceOf; `user`/`agent`/`permission` nem chegam ao ramo default.
  for (const [why, e] of forged) assert.equal(m.genuiChoiceOf(e), null, why)
  for (const kind of ['thought', 'tool', 'plan', 'notíce', 'NOTICE']) assert.equal(m.genuiChoiceOf(hubEntry(pending, { kind })), null, kind)
  assert.doesNotMatch(row(hubEntry(undefined, { data: { text: 'x', choice_response: response } as never })), /class="genui-choice[ "]/)
  // Update forjado: a mesma entrada (rev maior) muda para forma inválida/forjada; a linha volta ao texto literal.
  const v1 = hubEntry(pending)
  assert.match(row(v1), /class="genui-choice[ "]/)
  for (const [why, data] of [
    ['chave extra na projeção', { text: 'TEXTO LITERAL DA PERGUNTA', genuiChoice: { ...pending, html: '<b>x</b>' } }],
    ['estado inventado', { text: 'TEXTO LITERAL DA PERGUNTA', genuiChoice: { ...pending, state: 'approved' } }],
    ['snapshot trocado por fora do contrato', { text: 'TEXTO LITERAL DA PERGUNTA', genuiChoice: { ...pending, snapshot: snapshot.replace('"component":"choice"', '"component":"choice","script":1') } }],
    ['outro cartão por cima', { text: 'TEXTO LITERAL DA PERGUNTA', card: 'ownerQuestion', genuiChoice: pending }],
  ] as const) {
    const v2 = local({ ...v1, rev: 2, data }) as Entry
    assert.equal(m.genuiChoiceOf(v2), null, why)
    assert.doesNotMatch(row(v2), /class="genui-choice[ "]/, why)
  }
  assert.equal(m.genuiChoiceOf(local({ ...v1, rev: 2, kind: 'user' }) as Entry), null, 'update para user')
})

const message = (genuiChoice: unknown, submission: unknown = null, error: string | null = null) => {
  const card = m.genuiChoiceOf(entry(genuiChoice))
  assert.ok(card)
  return m.renderToStaticMarkup(m.createElement(m.GenuiChoiceMessage, { card, name: 'Donna', time: '10:40', submission: submission ? local(submission) as never : null, error: error as never, onAnswer() {} }))
}
const owner = { questionId: 'q1', revision: 1, snapshot }
const answer = { kind: 'option', id: 'mes' }

test('enviando não vira Respondida; finais só com o estado do Hub; nada de aprovação', () => {
  const sending = message(pending, { ...owner, status: 'sending', answer })
  assert.match(sending, /Enviando…/)
  assert.doesNotMatch(sending, /Respondida|role="radio"/)
  const answered = message({ ...pending, revision: 2, state: 'answered', answer, answeredAt: Date.UTC(2026, 10, 6, 13, 0) }, { ...owner, status: 'sending', answer })
  assert.match(answered, /Respondida/)
  assert.match(answered, /Sua resposta/)
  const cancelled = message({ ...pending, revision: 2, state: 'cancelled', at: 1 })
  assert.match(cancelled, /Encerrada/)
  assert.doesNotMatch(cancelled, /role="radio"|Enviar resposta/)
  const expired = message({ ...pending, revision: 2, state: 'expired' })
  assert.match(expired, /Expirada/)
  for (const html of [sending, answered, cancelled, expired]) assert.doesNotMatch(html, /aprovad|autorizad|Permitir/i)
})

test('erro do envio: mensagem própria, escolha volta ao cartão, sem eco do código do Hub', () => {
  const html = message(pending, { ...owner, status: 'error', answer }, 'revision')
  assert.match(html, /A pergunta ou a conversa mudou enquanto você respondia/)
  assert.match(html, /role="radio" aria-checked="true"/)
  assert.doesNotMatch(html, /genui_|conflict|revision_/)
})
