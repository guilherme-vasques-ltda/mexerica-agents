// MB-1b D70: Hub antigo sem o campo vira [], dado fora do contrato não entra, pergunta sem turno não é trabalho.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { isWorking, isWorkingIn, needsInput, normalizeBot } from './models.ts'
import { cachedBots, isCurrentMirrorSchema, MIRROR_SCHEMA, mirrorCacheStamp } from './fork-mirror-cache.ts'
import { onlyQuestionsPending, ownerQuestionOf, readPendingQuestions } from './fork-owner-question.ts'

const question = (n: number) => ({ entryId: `e${n}`, chatId: 'bot', text: `Pergunta ${n}?`, askedAt: n })

test('Hub antigo ou cache sem pendingQuestions normaliza para lista vazia', () => {
  assert.deepEqual(normalizeBot({ id: 'bot', status: 'idle' }).pendingQuestions, [])
  assert.deepEqual(readPendingQuestions(undefined), [])
  assert.deepEqual(readPendingQuestions({ entryId: 'e1' }), [])
  // Cache de schema anterior: o runtime entra indisponível e sem perguntas.
  const [old] = cachedBots('donnabot-mirror-v1/1/1', [normalizeBot({ id: 'bot', status: 'needsInput', pendingQuestions: [question(1)] })])
  assert.equal(old.status, 'unavailable')
  assert.deepEqual(old.pendingQuestions, [])
})

test('só perguntas válidas, no máximo cinco, sessão opcional', () => {
  const raw = [{ entryId: '', chatId: 'bot', text: 'x', askedAt: 1 }, { ...question(0), sessionId: 's2' }, ...[1, 2, 3, 4, 5, 6].map(question)]
  const read = readPendingQuestions(raw)
  assert.equal(read.length, 5)
  assert.equal(read[0].sessionId, 's2')
  assert.equal('sessionId' in read[1], false)
})

test('Precisa de você só por pergunta não é trabalho; com turno (permissão) continua sendo', () => {
  const asking = normalizeBot({ id: 'bot', status: 'needsInput', workingChat: null, pendingQuestions: [question(1)] })
  assert.equal(needsInput(asking), true)
  assert.equal(onlyQuestionsPending(asking), true)
  assert.equal(isWorking(asking), false, 'sem Parar, fila ou indicador de trabalho')
  const approving = normalizeBot({ id: 'bot', status: 'needsInput', workingChat: 'bot', pendingQuestions: [question(1)] })
  assert.equal(isWorking(approving), true)
})

// MB-1b fix (06/11): a forma que o Hub manda para pergunta no canal sem turno (teste do Host
// pergunta_no_canal_sem_turno_nao_vira_trabalho_no_canal_nem_no_membro). A conversa decide "trabalhando", Parar e
// fila por isWorkingIn (ThreadView e Composer): nem o canal nem o membro aparecem trabalhando ali.
test('pergunta no canal sem turno: nem o canal nem o membro aparecem trabalhando; turno real aparece', () => {
  const group = normalizeBot({ id: 'crew', kind: 'group', members: ['a'], status: 'idle', workingChat: null, pendingQuestions: [] })
  const member = normalizeBot({ id: 'a', status: 'needsInput', workingChat: null, pendingQuestions: [{ ...question(1), chatId: 'crew' }] })
  assert.equal(isWorkingIn(group, 'crew'), false)
  assert.equal(isWorkingIn(member, 'crew'), false)
  assert.equal(isWorkingIn(member, 'a'), false, 'na conversa direta do membro também não')
  const busy = normalizeBot({ id: 'crew', kind: 'group', members: ['a'], status: 'working', activity: 'a: lendo', workingChat: 'crew', pendingQuestions: [] })
  assert.equal(isWorkingIn(busy, 'crew'), true)
})

test('cartão: estado do Hub, answeredBy só quando respondida', () => {
  assert.equal(ownerQuestionOf({ card: 'other', ownerQuestion: {} }), null)
  assert.equal(ownerQuestionOf({ card: 'ownerQuestion', ownerQuestion: { status: 'aceita', chatId: 'bot' } }), null)
  const answered = ownerQuestionOf({ card: 'ownerQuestion', ownerQuestion: { status: 'answered', chatId: 'bot', sessionId: 'bot', askedAt: 1, answeredBy: 'm1' } })
  assert.equal(answered?.answeredBy, 'm1')
  const withdrawn = ownerQuestionOf({ card: 'ownerQuestion', ownerQuestion: { status: 'withdrawn', chatId: 'bot', answeredBy: 'm1' } })
  assert.equal(withdrawn?.answeredBy, null)
  assert.equal(withdrawn?.sessionId, 'bot')
})

test('MIRROR_SCHEMA 16+: cache do schema 15 (sem pendingQuestions) não é atual', () => {
  assert.equal(mirrorCacheStamp('1', '1'), `donnabot-mirror-v${MIRROR_SCHEMA}/1/1`)
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v15/1/1'), false)
  const [old] = cachedBots('donnabot-mirror-v15/1/1', [normalizeBot({ id: 'bot', status: 'idle' })])
  assert.deepEqual(old.pendingQuestions, [])
})
