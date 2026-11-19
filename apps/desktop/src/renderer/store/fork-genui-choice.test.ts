import assert from 'node:assert/strict'
import test from 'node:test'
import { ChoiceAnswers, type ChoiceSource } from './fork-genui-choice.ts'
import { tradutor } from '../lib/fork-i18n/index.ts'
import { choiceErrorText, choiceSendError, type ChoiceEntry, type ChoiceSendError } from '../views/fork/genui/choice-entry.ts'

// D70 (G1b app): a intenção do cartão sai pela RPC de conversa com os campos do contrato, no Space e na
// conversa da entrada; a principal não recebe RPC; sucesso não marca Respondida; erro guarda só o código.

const card: ChoiceEntry = { questionId: 'q1', snapshot: '{}', hub: { state: 'pending', revision: 1 }, spaceId: 'pessoal', conversationId: 'conv-1', botId: 'conv-1', sessionId: null }
const intent = { questionId: 'q1', revision: 1, snapshot: '{}', answer: { kind: 'option', id: 'a' } as const }
const conversation = (over: object = {}) => ({ id: 'conv-1', spaceId: 'pessoal', botId: 'donna', revision: 7, closed: false, ...over })

function source(answer: (body: Record<string, unknown>) => unknown, list = [conversation()]) {
  const calls: [string, Record<string, unknown>][] = []
  const src: ChoiceSource = {
    hello: { hostId: 'hub-1' },
    connectedClient: {
      async call(method, body) {
        const b = body as Record<string, unknown>
        calls.push([method, b])
        if (method === 'donnaConversations') return { hubId: 'hub-1', spaceId: 'pessoal', conversations: list }
        return answer(b)
      },
    },
  }
  return { src, calls }
}
const hubError = (status: number, code: string) => Object.assign(new Error(code), { status })

test('envia os campos do contrato e continua Enviando até o Hub mudar o estado no fio', async () => {
  const { src, calls } = source(() => ({ hubId: 'hub-1', context: { spaceId: 'pessoal', conversationId: 'conv-1' }, question: { state: 'answered' }, entry: { conversationId: 'conv-1' }, replayed: false }))
  const model = new ChoiceAnswers()
  await model.answer(src, card, intent)
  assert.deepEqual(calls.map((c) => c[0]), ['donnaConversations', 'donnaConversationChoiceAnswer'])
  assert.deepEqual(calls[1]![1], { expectedRevision: 7, questionId: 'q1', revision: 1, answer: { kind: 'option', id: 'a' }, conversationId: 'conv-1', hubId: 'hub-1', spaceId: 'pessoal' })
  assert.equal(model.submission(src, 'q1')?.submission.status, 'sending')
  // Histórico legado sem spaceId: vai ao Pessoal deste Hub.
  const legacy = source(() => ({ hubId: 'hub-1', entry: { conversationId: 'conv-1' } }))
  await new ChoiceAnswers().answer({ ...legacy.src, spaces: { personalSpaceId: 'pessoal' } }, { ...card, spaceId: null }, intent)
  assert.equal(legacy.calls[1]![1].spaceId, 'pessoal')
  const none = source(() => { throw new Error('não devia chamar') })
  const lost = new ChoiceAnswers()
  await lost.answer(none.src, { ...card, spaceId: null }, intent)
  assert.equal(none.calls.length, 0)
  assert.equal(lost.submission(none.src, 'q1')?.error, 'unsupported')
  // Outro Hub conectado: o envio anterior não aparece.
  assert.equal(model.submission({ ...src, hello: { hostId: 'hub-2' } }, 'q1'), null)
})

test('conversa principal ou fechada: nenhuma RPC de resposta', async () => {
  for (const [list, kind] of [[[conversation({ id: 'conv-1', botId: 'conv-1' })], 'unsupported'], [[], 'unsupported'], [[conversation({ closed: true })], 'closed']] as const) {
    const { src, calls } = source(() => { throw new Error('não devia chamar') }, [...list])
    const model = new ChoiceAnswers()
    await model.answer(src, card, intent)
    assert.deepEqual(calls.map((c) => c[0]), ['donnaConversations'])
    assert.equal(model.submission(src, 'q1')?.submission.status, 'error')
    assert.equal(model.submission(src, 'q1')?.error, kind)
  }
})

test('erro do Hub: só o código estável fica; um envio por vez; sem link não tenta', async () => {
  let release: (v: unknown) => void = () => {}
  const { src, calls } = source(() => new Promise((_, reject) => { release = reject }))
  const model = new ChoiceAnswers()
  const first = model.answer(src, card, intent)
  await new Promise((r) => setTimeout(r, 0))
  await model.answer(src, card, { ...intent, answer: { kind: 'none' } as never })
  assert.equal(calls.filter((c) => c[0] === 'donnaConversationChoiceAnswer').length, 1)
  release(hubError(409, 'genui_revision_conflict'))
  await first
  assert.deepEqual(model.submission(src, 'q1'), { submission: { ...intent, status: 'error' }, error: 'revision' })

  const offline = new ChoiceAnswers()
  await offline.answer({ hello: { hostId: 'hub-1' }, connectedClient: null }, card, intent)
  assert.equal(offline.submission({ hello: { hostId: 'hub-1' }, connectedClient: null }, 'q1')?.error, 'link')
})

test('códigos do Hub viram textos próprios, sem eco', () => {
  const cases: [unknown, ChoiceSendError][] = [
    [hubError(409, 'conversation_revision_conflict'), 'revision'],
    [hubError(400, 'genui_final_state'), 'final'],
    [hubError(400, 'conversation_closed'), 'closed'],
    [hubError(400, 'genui_context_reset'), 'closed'],
    [hubError(400, 'genui_unknown_option'), 'invalid'],
    [hubError(400, 'genui_question_scope_mismatch'), 'scope'],
    [hubError(400, 'genui_conversation_required'), 'unsupported'],
    [hubError(403, 'qualquer'), 'forbidden'],
    [hubError(0, 'rede'), 'link'],
    [hubError(500, 'SEGREDO <b>resposta do dono</b>'), 'generic'],
    // sessionChoiceAnswer (contrato v1, R3 seção 3): prefixo estável, resto da mensagem nunca exibido.
    [hubError(400, 'invalid_field: pedido de resposta inválido.'), 'invalid'],
    [hubError(400, 'genui_question_required'), 'invalid'],
    [hubError(400, 'genui_invalid_text'), 'invalid'],
    [hubError(409, 'session_revision_conflict'), 'revision'],
    [hubError(409, 'genui_cas_conflict'), 'revision'],
    [hubError(400, 'genui_deadline_elapsed'), 'final'],
    [hubError(400, 'session_closed'), 'closed'],
    [hubError(400, 'unknown_session'), 'scope'],
    [hubError(400, 'genui_session_mismatch'), 'scope'],
    [hubError(400, 'unknown bot'), 'scope'],
    [hubError(400, 'session_chat_unsupported'), 'unsupported'],
    [hubError(400, 'not_allowed: autorização original encerrada'), 'forbidden'],
    [hubError(400, 'session_selection_unavailable: SEGREDO conta x'), 'generic'],
    [hubError(400, 'engine_unavailable'), 'generic'],
    [hubError(400, 'genui_revision_conflictX'), 'generic'],
    ['string solta', 'generic'],
  ]
  for (const [e, kind] of cases) {
    assert.equal(choiceSendError(e), kind)
    assert.doesNotMatch(choiceErrorText(tradutor('pt-BR'), kind), /genui_|conflict|SEGREDO|_/)
  }
})

// G1b nas extras UI01 (sessionChoiceAnswer v1, recibo 028596a7, R3 d883e822): intenção `choice` capturada antes
// do primeiro await, catálogo relido no mesmo link, validade conferida em volta de cada await, só os seis campos.
const extra: ChoiceEntry = { ...card, spaceId: 'pessoal', conversationId: 'byte', botId: 'byte', sessionId: 's-a' }
const sessionRow = (over: object = {}) => ({ id: 's-a', chatId: 'byte', principal: false, state: 'open', revision: 4, ...over })
const receipt = (over: object = {}) => ({ session: { id: 's-a', chatId: 'byte' }, question: { questionId: 'q1', state: 'answered' }, entry: { botId: 'byte', sessionId: 's-a', kind: 'user' }, replayed: false, ...over })

function sessionSource(opts: { actions?: unknown; rows?: object[]; answer?: (b: Record<string, unknown>) => unknown; announced?: boolean; onCall?: (method: string) => void; reject?: Partial<Record<'sessions' | 'sessionChoiceAnswer', unknown>> } = {}) {
  const calls: [string, Record<string, unknown>][] = []
  let live = true
  const client = {
    async call(method: string, body: unknown) {
      const b = body as Record<string, unknown>
      calls.push([method, b])
      opts.onCall?.(method)
      if (opts.reject && method in opts.reject) throw opts.reject[method as 'sessions']
      if (method === 'sessions') return { sessions: opts.rows ?? [sessionRow()], sessionActions: opts.actions ?? ['choice'] }
      if (method === 'sessionChoiceAnswer') return (opts.answer ?? (() => receipt()))(b)
      throw new Error('método inesperado')
    },
  }
  const intents: unknown[][] = []
  const src: ChoiceSource = {
    hello: { hostId: 'hub-1' },
    // O link de Space nunca é usado no ramo de sessão.
    connectedClient: { async call(method) { calls.push([`space:${method}`, {}]); throw new Error('não devia chamar') } },
    sessions: {
      intent(chatId, sessionId, action) {
        intents.push([chatId, sessionId, action])
        return opts.announced === false ? null : { chatId, id: sessionId, gen: 1, client: client as never }
      },
      current: (_intent, c) => live && c === (client as never),
      of: () => [sessionRow({ state: opts.announced === false ? 'closed' : 'open' })],
    },
  }
  return { src, calls, intents, drop: () => { live = false } }
}

test('extra: intenção choice, releitura no mesmo link e só os seis campos (sem hubId/spaceId/conversationId)', async () => {
  const { src, calls, intents } = sessionSource()
  const model = new ChoiceAnswers()
  await model.answer(src, extra, intent)
  assert.deepEqual(intents, [['byte', 's-a', 'choice']])
  assert.deepEqual(calls.map((c) => c[0]), ['sessions', 'sessionChoiceAnswer'])
  assert.deepEqual(calls[0]![1], { botId: 'byte' })
  assert.deepEqual(calls[1]![1], { botId: 'byte', sessionId: 's-a', expectedRevision: 4, questionId: 'q1', revision: 1, answer: { kind: 'option', id: 'a' } })
  assert.equal(model.submission(src, 'q1')?.submission.status, 'sending')
  assert.equal(model.submission(src, 'q1')?.error, null)
  // Replay idêntico do Hub (`replayed: true`) é o mesmo recibo: continua Enviando até o fio.
  const again = sessionSource({ answer: () => receipt({ replayed: true }) })
  const replay = new ChoiceAnswers()
  await replay.answer(again.src, extra, intent)
  assert.equal(replay.submission(again.src, 'q1')?.error, null)
  // Principal (`sessionId === botId`) e entrada sem sessão seguem o ramo de conversa existente, brutos.
  for (const c of [{ ...extra, sessionId: 'byte' }, { ...extra, sessionId: null }]) {
    const s = sessionSource()
    await new ChoiceAnswers().answer(s.src, c, intent)
    assert.deepEqual(s.calls.map((x) => x[0]), ['space:donnaConversations'])
    assert.equal(s.intents.length, 0)
  }
})

test('extra: sem anúncio, sessão fechada/sumida/revisão ilegível relida — nenhuma RPC de resposta', async () => {
  const cases: [Parameters<typeof sessionSource>[0], ChoiceSendError, string[]][] = [
    [{ announced: false }, 'closed', []],
    [{ actions: ['create', 'send'] }, 'unsupported', ['sessions']],
    [{ actions: 'choice' }, 'unsupported', ['sessions']],
    [{ rows: [sessionRow({ state: 'closed' })] }, 'closed', ['sessions']],
    [{ rows: [] }, 'scope', ['sessions']],
    [{ rows: [sessionRow({ chatId: 'outro' })] }, 'scope', ['sessions']],
    [{ rows: [sessionRow({ principal: true })] }, 'scope', ['sessions']],
    [{ rows: [sessionRow({ revision: '4' })] }, 'generic', ['sessions']],
  ]
  for (const [opts, kind, methods] of cases) {
    const s = sessionSource(opts)
    const model = new ChoiceAnswers()
    await model.answer(s.src, extra, intent)
    assert.deepEqual(s.calls.map((c) => c[0]), methods, JSON.stringify(opts))
    assert.equal(model.submission(s.src, 'q1')?.error, kind, JSON.stringify(opts))
  }
  // Sem o catálogo no store (ou intenção nula de sessão aberta): não suportado, sem cair na conversa.
  const bare = sessionSource({ announced: false })
  const model = new ChoiceAnswers()
  await model.answer({ ...bare.src, sessions: { ...bare.src.sessions!, of: () => [sessionRow()] } }, extra, intent)
  assert.equal(model.submission(bare.src, 'q1')?.error, 'unsupported')
  assert.equal(bare.calls.length, 0)
})

test('extra: link/época trocados na releitura ou na RPC = link, sem RPC nem adoção', async () => {
  const d = sessionSource({ onCall: (m) => { if (m === 'sessions') d.drop() } })
  const m1 = new ChoiceAnswers()
  await m1.answer(d.src, extra, intent)
  assert.deepEqual(d.calls.map((c) => c[0]), ['sessions'])
  assert.equal(m1.submission(d.src, 'q1')?.error, 'link')
  const r = sessionSource({ onCall: (m) => { if (m === 'sessionChoiceAnswer') r.drop() } })
  const m2 = new ChoiceAnswers()
  await m2.answer(r.src, extra, intent)
  assert.deepEqual(r.calls.map((c) => c[0]), ['sessions', 'sessionChoiceAnswer'])
  assert.equal(m2.submission(r.src, 'q1')?.error, 'link', 'resposta de antes da troca não é adotada')
})

test('extra: resultado fora do destino ou malformado não é adotado; erro do Hub vira código', async () => {
  for (const bad of [
    receipt({ entry: { botId: 'byte', sessionId: 'byte' } }),
    receipt({ entry: { botId: 'outro', sessionId: 's-a' } }),
    receipt({ session: { id: 's-b', chatId: 'byte' } }),
    receipt({ question: { questionId: 'q2' } }),
    receipt({ replayed: 'false' }),
    {},
    null,
  ]) {
    const s = sessionSource({ answer: () => bad })
    const model = new ChoiceAnswers()
    await model.answer(s.src, extra, intent)
    assert.equal(model.submission(s.src, 'q1')?.error, 'scope', JSON.stringify(bad))
  }
  for (const [code, kind] of [['session_revision_conflict', 'revision'], ['genui_question_scope_mismatch', 'scope'], ['invalid_field: pedido de resposta inválido.', 'invalid'], ['session_selection_unavailable: x', 'generic']] as const) {
    const s = sessionSource({ answer: () => { throw hubError(400, code) } })
    const model = new ChoiceAnswers()
    await model.answer(s.src, extra, intent)
    assert.equal(model.submission(s.src, 'q1')?.error, kind, code)
  }
})

test('extra: rejeição tardia na releitura ou na RPC depois da troca de link = link, sem adotar o erro antigo', async () => {
  for (const method of ['sessions', 'sessionChoiceAnswer'] as const) {
    // O hello/troca de link acontece durante o await, e a chamada então rejeita com um erro da época velha.
    const s = sessionSource({ onCall: (m) => { if (m === method) s.drop() }, reject: { [method]: hubError(400, 'session_closed') } })
    const model = new ChoiceAnswers()
    await model.answer(s.src, extra, intent)
    assert.equal(model.submission(s.src, 'q1')?.error, 'link', method)
    assert.equal(s.calls.filter((c) => c[0] === 'sessionChoiceAnswer').length, method === 'sessions' ? 0 : 1, method)
  }
  // Controle: sem troca de link, o mesmo erro vira o código dele.
  const s = sessionSource({ reject: { sessionChoiceAnswer: hubError(400, 'session_closed') } })
  const model = new ChoiceAnswers()
  await model.answer(s.src, extra, intent)
  assert.equal(model.submission(s.src, 'q1')?.error, 'closed')
})
