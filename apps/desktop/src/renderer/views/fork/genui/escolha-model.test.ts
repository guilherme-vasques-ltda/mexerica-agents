import assert from 'node:assert/strict'
import test from 'node:test'
import {
  CHOICE_LIMITS,
  choiceSnapshot,
  currentDraft,
  draftAnswer,
  draftFrom,
  emptyDraft,
  parseAnswer,
  parseChoice,
  pickOption,
  projectChoice,
  typeText,
  type ChoiceV1,
} from './escolha-model.ts'

// D70: só as fronteiras do cartão de pergunta. Paridade com o validador `choice` v1 do Hub,
// rascunho local separado do envio, e Respondida só pelo estado do Hub.

const base = () => ({
  version: 1,
  component: 'choice',
  title: 'Escolha',
  summary: 'Dados informados pelo bot',
  data: { question: 'Qual opção?', options: [{ id: 'a', label: 'Opção A' }, { id: 'b.2', label: 'Opção B' }] },
})
const ok = (v: unknown): ChoiceV1 => {
  const r = parseChoice(v)
  assert.ok(r.ok, JSON.stringify(r))
  return r.value
}
const err = (v: unknown) => {
  const r = parseChoice(v)
  return r.ok ? 'ok' : r.error
}

test('aceita o exemplo do contrato como texto JSON e como valor, devolvendo cópia só com o contrato', () => {
  const value = base()
  const a = ok(JSON.stringify(value))
  const b = ok(value)
  assert.deepEqual(a, b)
  assert.notEqual(b.data.options, value.data.options)
})

test('objeto fechado em todos os níveis, componente e versão do catálogo', () => {
  assert.equal(err({ ...base(), extra: 1 }), 'genui_invalid_payload')
  assert.equal(err({ ...base(), data: { ...base().data, html: '<b>' } }), 'genui_invalid_payload')
  assert.equal(err({ ...base(), data: { ...base().data, options: [{ id: 'a', label: 'A', url: 'https://x' }] } }), 'genui_invalid_payload')
  assert.equal(err({ ...base(), component: 'chart' }), 'genui_invalid_payload')
  assert.equal(err({ ...base(), version: 2 }), 'genui_unsupported_version')
  assert.equal(err({ ...base(), version: '1' }), 'genui_invalid_payload')
  assert.equal(err('{"version":1,'), 'genui_invalid_payload')
  assert.equal(err('{"__proto__":{},"version":1,"component":"choice","title":"t","summary":"s","data":{"question":"q","options":[{"id":"a","label":"A"}]}}'), 'genui_invalid_payload')
})

test('version no JSON bruto exige literal uint como o u8 do Hub; objeto decodificado só conserva o número', () => {
  const raw = JSON.stringify(base())
  for (const key of ['version', 'vers\\u0069on']) {
    const withVersion = (token: string) => raw.replace('"version":1', `"${key}" : \n${token}`)
    for (const token of ['1.0', '1e0', '-0', '-1', '"1"', 'null']) {
      assert.equal(err(withVersion(token)), 'genui_invalid_payload', `${key}:${token}`)
    }
    assert.equal(err(withVersion('1')), 'ok')
    assert.equal(err(withVersion('2')), 'genui_unsupported_version')
    assert.equal(err(withVersion('255')), 'genui_unsupported_version')
    assert.equal(err(withVersion('256')), 'genui_invalid_payload')
  }
  // A informação lexical já não existe nesses objetos; 1.0 e 1e0 chegam como o mesmo Number 1.
  assert.equal(err(JSON.parse(raw.replace('"version":1', '"version":1.0'))), 'ok')
  assert.equal(err(JSON.parse(raw.replace('"version":1', '"version":1e0'))), 'ok')
})

test('não executa getters nem toJSON do valor recebido', () => {
  let ran = 0
  const withGetter = base() as Record<string, unknown>
  Object.defineProperty(withGetter, 'title', { enumerable: true, get: () => { ran++; return 'x' } })
  assert.equal(err(withGetter), 'genui_invalid_payload')
  const withToJSON = { ...base(), toJSON: () => { ran++; return {} } }
  assert.equal(err(withToJSON), 'genui_invalid_payload')
  const inherited = Object.assign(Object.create({ toJSON: () => { ran++; return {} } }), base())
  assert.equal(err(inherited), 'genui_invalid_payload')
  assert.equal(ran, 0)
})

test('JSON bruto recusa campos duplicados no topo, data e opções, inclusive chaves escapadas equivalentes', () => {
  const raw = JSON.stringify(base())
  for (const [key, token] of [['title', '"Escolha"'], ['question', '"Qual opção?"'], ['label', '"Opção A"']] as const) {
    for (const repeated of [key, `\\u${key.charCodeAt(0).toString(16).padStart(4, '0')}${key.slice(1)}`]) {
      assert.equal(err(raw.replace(`"${key}":${token}`, `"${key}":${token},"${repeated}":${token}`)), 'genui_invalid_payload', repeated)
    }
  }
})

test('limites contam pontos de código (chars do Rust), com acento e emoji', () => {
  const emoji = '😀' // 2 unidades UTF-16, 1 ponto de código, 4 bytes UTF-8
  assert.equal(err({ ...base(), title: emoji.repeat(CHOICE_LIMITS.title) }), 'ok')
  assert.equal(err({ ...base(), title: emoji.repeat(CHOICE_LIMITS.title + 1) }), 'genui_invalid_text')
  assert.equal(err({ ...base(), title: 'é'.repeat(CHOICE_LIMITS.title) }), 'ok')
  assert.equal(err({ ...base(), summary: 'ç'.repeat(CHOICE_LIMITS.summary + 1) }), 'genui_invalid_text')
  assert.equal(err({ ...base(), data: { ...base().data, question: emoji.repeat(CHOICE_LIMITS.question) } }), 'ok')
  assert.equal(err({ ...base(), data: { question: 'q', options: [{ id: 'a', label: emoji.repeat(161) }] } }), 'genui_invalid_text')
  assert.equal(err({ ...base(), title: '\ud83d' }), 'genui_invalid_payload')
})

test('texto vazio, só espaço Unicode ou com controle Cc é recusado; \\n e \\t passam', () => {
  assert.equal(err({ ...base(), title: '' }), 'genui_invalid_text')
  assert.equal(err({ ...base(), title: ' \u3000\u00a0\n' }), 'genui_invalid_text')
  assert.equal(err({ ...base(), summary: 'a\u0007b' }), 'genui_invalid_text')
  assert.equal(err({ ...base(), summary: 'a\u0085b' }), 'genui_invalid_text')
  assert.equal(err({ ...base(), summary: 'a\u007fb' }), 'genui_invalid_text')
  assert.equal(err({ ...base(), summary: 'linha\n\tsegunda' }), 'ok')
})

test('64 KiB em bytes UTF-8 do JSON bruto', () => {
  // Espaço em branco do próprio JSON: conta no limite bruto sem mudar os textos.
  const pad = (n: number) => JSON.stringify(base()).replace('{', `{${' '.repeat(n)}`)
  const over = pad(CHOICE_LIMITS.maxBytes - new TextEncoder().encode(JSON.stringify(base())).length + 1)
  assert.equal(err(pad(CHOICE_LIMITS.maxBytes - new TextEncoder().encode(JSON.stringify(base())).length)), 'ok')
  assert.equal(new TextEncoder().encode(over).length, CHOICE_LIMITS.maxBytes + 1)
  assert.equal(err(over), 'genui_payload_too_large')
  // 3 bytes por "€": 22 mil caracteres passam de 64 KiB em bytes, mesmo abaixo em unidades.
  assert.equal(err(JSON.stringify(base()).replace('"summary":"', `"summary":"${'€'.repeat(22_000)}`)), 'genui_payload_too_large')
})

test('opções: 1 a 8, IDs ASCII opacos, únicos e limitados', () => {
  const opts = (options: unknown[]) => ({ ...base(), data: { question: 'q', options } })
  assert.equal(err(opts([])), 'genui_invalid_options')
  assert.equal(err(opts(Array.from({ length: 9 }, (_, i) => ({ id: `o${i}`, label: 'x' })))), 'genui_invalid_options')
  assert.equal(err(opts(Array.from({ length: 8 }, (_, i) => ({ id: `o${i}`, label: 'x' })))), 'ok')
  for (const id of ['', 'a b', 'https://x', 'á', 'a/b', 'x'.repeat(161)]) assert.equal(err(opts([{ id, label: 'x' }])), 'genui_invalid_option_id', id)
  assert.equal(err(opts([{ id: 'x'.repeat(160), label: 'x' }])), 'ok')
  assert.equal(err(opts([{ id: 'a', label: 'x' }, { id: 'a', label: 'y' }])), 'genui_duplicate_option_id')
})

test('resposta fechada como no Hub: none com extra é recusado; opção precisa existir', () => {
  const choice = ok(base())
  assert.deepEqual(parseAnswer({ kind: 'none' }, choice), { ok: true, value: { kind: 'none' } })
  assert.equal(parseAnswer({ kind: 'none', id: 'a' }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'none', text: '' }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'option', id: 'zz' }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'option', id: 'a', label: 'A' }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'text', text: ' ' }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'text', text: '😀'.repeat(4_000) }, choice).ok, true)
  assert.equal(parseAnswer({ kind: 'text', text: '😀'.repeat(4_001) }, choice).ok, false)
  assert.equal(parseAnswer({ kind: 'approve' }, choice).ok, false)
})

test('rascunho: nada sai sem escolha; escrever escolhe o texto; marcar opção mantém o texto sem enviá-lo', () => {
  const choice = ok(base())
  assert.deepEqual(draftAnswer(emptyDraft, choice), { ok: false, reason: 'empty' })
  const typed = typeText(emptyDraft, '  outra coisa  ')
  assert.deepEqual(draftAnswer(typed, choice), { ok: true, answer: { kind: 'text', text: 'outra coisa' } })
  const picked = pickOption(typed, { kind: 'option', id: 'b.2' })
  assert.equal(picked.text, '  outra coisa  ')
  assert.deepEqual(draftAnswer(picked, choice), { ok: true, answer: { kind: 'option', id: 'b.2' } })
  assert.deepEqual(draftAnswer(typeText(picked, ''), choice), { ok: true, answer: { kind: 'option', id: 'b.2' } })
  assert.equal(draftAnswer(typeText(emptyDraft, '\n \t'), choice).ok, false)
  assert.deepEqual(draftAnswer(typeText(emptyDraft, 'x'.repeat(4_001)), choice), { ok: false, reason: 'text_too_long' })
  assert.deepEqual(draftAnswer(typeText(emptyDraft, 'a\u0007'), choice), { ok: false, reason: 'text_invalid' })
  assert.deepEqual(draftAnswer(pickOption(emptyDraft, { kind: 'option', id: 'zz' }), choice), { ok: false, reason: 'unknown_option' })
})

test('Respondida só pelo Hub com resposta válida; envio do consumidor nunca vira sucesso', () => {
  const choice = ok(base())
  const pending = { state: 'pending', revision: 3 } as const
  const owner = { questionId: 'q1', revision: 3, snapshot: choiceSnapshot(choice) }
  const answer = { kind: 'option', id: 'a' } as const
  assert.equal(projectChoice(choice, pending, null, owner).status, 'pending')
  for (const status of ['sending', 'unconfirmed'] as const) {
    const v = projectChoice(choice, pending, { ...owner, status, answer }, owner)
    assert.equal(v.status, status)
    assert.equal(v.editable, false)
  }
  const failed = projectChoice(choice, pending, { ...owner, status: 'error', answer }, owner)
  assert.deepEqual([failed.status, failed.editable], ['error', true])
  // Envio de outra revisão é velho; não mostra nada como enviado.
  assert.equal(projectChoice(choice, pending, { ...owner, status: 'sending', revision: 2, answer }, owner).status, 'pending')
  // Resposta inválida para esta pergunta não marca Respondida.
  const finalOwner = { ...owner, revision: 4 }
  assert.equal(projectChoice(choice, { state: 'answered', revision: 4, answer: { kind: 'option', id: 'zz' }, answeredAt: 1 }, null, finalOwner).status, 'unavailable')
  assert.equal(projectChoice(choice, { state: 'answered', revision: 4, answer: { kind: 'none', extra: 1 }, answeredAt: 1 }, null, finalOwner).status, 'unavailable')
  const done = projectChoice(choice, { state: 'answered', revision: 4, answer, answeredAt: 10 }, { ...finalOwner, status: 'sending', answer: { kind: 'none' } }, finalOwner)
  assert.deepEqual([done.status, done.editable, done.shown], ['answered', false, answer])
  for (const state of ['cancelled', 'expired'] as const) {
    const v = projectChoice(choice, { state, revision: 4, at: 5 }, { ...finalOwner, status: 'error', answer }, finalOwner)
    assert.deepEqual([v.status, v.editable, v.shown], [state, false, null])
  }
})

test('erro de envio devolve a mesma escolha ao rascunho', () => {
  assert.deepEqual(draftFrom({ kind: 'text', text: 'oi' }), { pick: { kind: 'text' }, text: 'oi' })
  assert.deepEqual(draftFrom({ kind: 'option', id: 'a' }), { pick: { kind: 'option', id: 'a' }, text: '' })
})

test('depois de erro, apagar toda a resposta escrita mantém vazio e permite editar de novo', () => {
  const choice = ok(base())
  const owner = { questionId: 'q1', revision: 3, snapshot: choiceSnapshot(choice) }
  const view = projectChoice(choice, { state: 'pending', revision: 3 }, { ...owner, status: 'error', answer: { kind: 'text', text: 'resposta anterior' } }, owner)
  const restored = currentDraft(null, owner, view)
  assert.equal(restored.text, 'resposta anterior')
  assert.deepEqual(currentDraft({ owner, draft: null }, owner, view), restored)
  const held = { owner, draft: typeText(restored, '') }
  const cleared = currentDraft(held, owner, view)
  assert.deepEqual(cleared, emptyDraft)
  assert.equal(draftAnswer(cleared, choice).ok, false)
  assert.deepEqual(draftAnswer(typeText(cleared, 'nova resposta'), choice), { ok: true, answer: { kind: 'text', text: 'nova resposta' } })
})

test('rascunho pertence a pergunta, revisão e conteúdo: troca descarta antes de emitir', () => {
  const choice = ok(base())
  const owner = { questionId: 'q1', revision: 3, snapshot: choiceSnapshot(choice) }
  const held = { owner, draft: pickOption(emptyDraft, { kind: 'option', id: 'a' }) }
  const pending = projectChoice(choice, { state: 'pending', revision: 3 }, null, owner)
  assert.deepEqual(currentDraft(held, owner, pending), held.draft)
  // Mesmo conteúdo vindo como objeto novo continua o mesmo dono.
  assert.deepEqual(currentDraft(held, { ...owner, snapshot: choiceSnapshot(ok(JSON.stringify(base()))) }, pending), held.draft)
  for (const other of [{ ...owner, questionId: 'q2' }, { ...owner, revision: 4 }, { ...owner, snapshot: choiceSnapshot(ok({ ...base(), title: 'Outra' })) }]) {
    const view = projectChoice(choice, { state: 'pending', revision: other.revision }, null, other)
    assert.deepEqual(currentDraft(held, other, view), emptyDraft)
    assert.equal(draftAnswer(currentDraft(held, other, view), choice).ok, false)
  }
  const failed = projectChoice(choice, { state: 'pending', revision: 3 }, { ...owner, status: 'error', answer: { kind: 'none' } }, owner)
  assert.deepEqual(currentDraft(null, owner, failed), { pick: { kind: 'none' }, text: '' })
})
