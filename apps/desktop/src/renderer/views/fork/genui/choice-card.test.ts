import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'
import type { ChoiceCardProps } from './ChoiceCard'
import { choiceSnapshot, parseChoice, type ChoiceHubState, type ChoiceSubmission } from './escolha-model.ts'

// D70: fronteira do render do ChoiceCard real. Texto do payload só como texto (nada de HTML, link,
// imagem ou estilo dele); estados que não são pendentes não oferecem controle; render não emite intenção.
const bundle = await build({
  stdin: {
    contents: `export { ChoiceCard } from './views/fork/genui/ChoiceCard';
      export { createElement } from 'react';
      export { renderToStaticMarkup } from 'react-dom/server';`,
    resolveDir: new URL('../../../', import.meta.url).pathname, loader: 'ts',
  },
  bundle: true, platform: 'node', format: 'cjs', write: false, jsx: 'automatic',
  // O ícone é decorativo (aria-hidden) e lê o manifesto do Mac por store externo: fica um marcador vazio.
  plugins: [{ name: 'icone', setup(b) {
    b.onResolve({ filter: /\/components\/Icon$/ }, () => ({ path: 'icone', namespace: 'icone' }))
    b.onLoad({ filter: /.*/, namespace: 'icone' }, () => ({ contents: 'export function Icon() { return null }', loader: 'js' }))
  } }],
  loader: { '.css': 'empty', '.jpg': 'empty', '.png': 'empty', '.svg': 'empty', '.webp': 'empty' },
  define: { 'process.env.NODE_ENV': '"production"', 'import.meta.env.BASE_URL': '"/"' },
})
// Janela mínima: os módulos compartilhados (Icon, Controls, tema) leem preferências ao carregar.
const media = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
const window = { codync: { platform: 'linux' }, addEventListener() {}, removeEventListener() {}, matchMedia: media, devicePixelRatio: 1 }
// O texto conferido é o pt-BR: idioma escolhido pt-BR (fatia 2: o cartão segue o idioma do app; D454/D495).
const storage = { getItem: (k: string) => (k === 'appLanguage' ? '"pt-BR"' : null), setItem() {}, removeItem() {} }
const module = { exports: {} }
const realm = { module, exports: module.exports, require: createRequire(import.meta.url), window, matchMedia: media, localStorage: storage, TextEncoder, URLSearchParams, console, setTimeout, clearTimeout, queueMicrotask }
runInNewContext(bundle.outputFiles[0]!.text, realm)
// O parser só aceita objetos simples do próprio realm (outro protótipo falha fechado): os dados do
// teste nascem no realm do bundle, como nasceriam no renderer.
const realmJSON = runInNewContext('JSON', realm) as JSON
const local = <T>(v: T): T => realmJSON.parse(JSON.stringify(v)) as T
const m = module.exports as {
  ChoiceCard: typeof import('./ChoiceCard').ChoiceCard
  createElement: typeof import('react').createElement
  renderToStaticMarkup: typeof import('react-dom/server').renderToStaticMarkup
}

const payload = {
  version: 1, component: 'choice', title: 'Texto literal',
  summary: 'Resumo <img src=x onerror=alert(1)> [link](https://exemplo.invalid)',
  data: { question: 'Qual? <a href="https://exemplo.invalid">aqui</a>', options: [{ id: 'a', label: '<script>alert(1)</script>' }, { id: 'b', label: '![x](https://exemplo.invalid/x.png)' }] },
}
const parsedPayload = parseChoice(payload)
assert.ok(parsedPayload.ok)
const fixtureOwner = { questionId: 'q1', revision: 1, snapshot: choiceSnapshot(parsedPayload.value) }
const intents: unknown[] = []
const render = (p: Partial<ChoiceCardProps>) =>
  m.renderToStaticMarkup(m.createElement(m.ChoiceCard, {
    questionId: p.questionId ?? 'q1', botName: 'a Donna', onAnswer: (i) => intents.push(i),
    payload: local(p.payload ?? payload),
    hub: local(p.hub ?? ({ state: 'pending', revision: 1 } as ChoiceHubState)),
    submission: p.submission ? local(p.submission) : null,
  }))

test('texto do payload fica texto: nenhum elemento, link, imagem ou estilo vindo dele', () => {
  const html = render({})
  assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/)
  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/)
  assert.match(html, /!\[x\]\(https:\/\/exemplo\.invalid\/x\.png\)/)
  assert.doesNotMatch(html, /<(script|img|a|iframe|link)\b/i)
  // Nenhum atributo ativo ou de estilo em tag alguma (o texto escapado não conta: `<` vira &lt;).
  assert.doesNotMatch(html, /<[^>]*\s(href|src|style|on\w+)=/i)
  // Pendente: grupo de rádios, campo rotulado e Enviar resposta inativo sem resposta.
  assert.equal(html.match(/role="radio"/g)?.length, 3)
  assert.match(html, /<label for="[^"]+">Ou escreva outra resposta<\/label><textarea/)
  assert.match(html, /<button type="button" class="styled-button primary" disabled=""[^>]*>Enviar resposta<\/button>/)
  assert.doesNotMatch(html, /Permitir|aprova/i)
})

test('enviando, sem confirmação e finais: opções legíveis, sem controle; Respondida só com o Hub', () => {
  const answer = { kind: 'option', id: 'a' } as const
  const cases: [Partial<ChoiceCardProps>, string][] = [
    [{ submission: { ...fixtureOwner, status: 'sending', answer } }, 'Enviando…'],
    [{ submission: { ...fixtureOwner, status: 'unconfirmed', answer } }, 'Sem confirmação'],
    [{ hub: { state: 'answered', revision: 2, answer, answeredAt: 0 } }, 'Respondida'],
    [{ hub: { state: 'expired', revision: 2, at: null } }, 'Expirada'],
    [{ hub: { state: 'cancelled', revision: 2 } }, 'Encerrada'],
  ]
  for (const [props, label] of cases) {
    const html = render(props)
    assert.match(html, new RegExp(label))
    assert.doesNotMatch(html, /role="radio"|<textarea|<button/, label)
    assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/, label)
    assert.equal(/Sua resposta<\/span>/.test(html), label === 'Respondida', label)
  }
})

test('payload fora do contrato não mostra nada dele; render nunca emite intenção', () => {
  const html = render({ payload: { ...payload, extra: 'SENTINELA-123' } })
  assert.match(html, /Pergunta indisponível/)
  assert.doesNotMatch(html, /SENTINELA|alert|Texto literal/)
  assert.deepEqual(intents, [])
})

test('envio antigo ou sem vínculo completo não bloqueia nem restaura resposta de outro dono', () => {
  const parsed = parseChoice(payload)
  assert.ok(parsed.ok)
  const owner = { questionId: 'q1', revision: 1, snapshot: choiceSnapshot(parsed.value) }
  for (const status of ['error', 'sending', 'unconfirmed'] as const) {
    const submission = { ...owner, status, answer: { kind: 'option', id: 'a' } as const }
    const cases: Partial<ChoiceCardProps>[] = [
      { questionId: 'q2', submission },
      { payload: { ...payload, title: 'Outra pergunta' }, submission },
      { hub: { state: 'pending', revision: 2 }, submission },
    ]
    for (const field of ['questionId', 'revision', 'snapshot'] as const) {
      const incomplete: Partial<ChoiceSubmission> = { ...submission }
      delete incomplete[field]
      // Compatibilidade em execução: consumidor antigo pode escapar do tipo novo; deve ficar inerte.
      cases.push({ submission: incomplete as ChoiceSubmission })
    }
    for (const props of cases) {
      const html = render(props)
      assert.match(html, /data-status="pending"/, `${status}: ${JSON.stringify(props)}`)
      assert.doesNotMatch(html, /aria-checked="true"/)
      assert.match(html, /<textarea[^>]*><\/textarea>/)
      assert.match(html, /<button type="button" class="styled-button primary" disabled=""/)
    }
  }
  assert.deepEqual(intents, [])
})

test('status forjado no envio não encerra pergunta pendente; finais só vêm do Hub', () => {
  const answer = { kind: 'option', id: 'a' } as const
  for (const status of ['answered', 'cancelled', 'expired', 'pending', 'outro', undefined, null, 7, false, {}]) {
    // Bypass intencional do tipo TS: props em execução podem vir de um consumidor incorreto.
    const value = { ...fixtureOwner, answer, ...(status === undefined ? {} : { status }) }
    const submission = value as unknown as ChoiceSubmission
    const html = render({ submission })
    assert.match(html, /data-status="pending"/, `status: ${JSON.stringify(status)}`)
    assert.doesNotMatch(html, /aria-checked="true"|Sua resposta<\/span>|Respondida|Encerrada|Expirada/)
    assert.match(html, /<textarea[^>]*><\/textarea>/)
    assert.match(html, /<button type="button" class="styled-button primary" disabled=""/)
  }
  const legitimateError = render({ submission: { ...fixtureOwner, status: 'error', answer } })
  assert.match(legitimateError, /data-status="error"/)
  assert.match(legitimateError, /aria-checked="true"/)
  assert.match(legitimateError, /<button type="button" class="styled-button primary" aria-describedby=/)
  const forged = { ...fixtureOwner, status: 'answered', answer: { kind: 'option', id: 'b' } } as unknown as ChoiceSubmission
  const finals: ChoiceHubState[] = [
    { state: 'answered', revision: 1, answer, answeredAt: 0 },
    { state: 'cancelled', revision: 1 },
    { state: 'expired', revision: 1 },
  ]
  for (const hub of finals) {
    const html = render({ hub, submission: forged })
    assert.match(html, new RegExp(`data-status="${hub.state}"`))
    assert.doesNotMatch(html, /role="radio"|<textarea|<button/)
    assert.equal(/Sua resposta<\/span>/.test(html), hub.state === 'answered')
  }
  assert.deepEqual(intents, [])
})
