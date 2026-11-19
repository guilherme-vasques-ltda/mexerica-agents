import assert from 'node:assert/strict'
import test from 'node:test'
import type { Bot, Entry } from '../../../../shared/models.ts'
import type { PendingQuestion } from '../../../../shared/fork-owner-question.ts'
import { traduzir, traduzirPlural } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { chatActivity, combinedState, pendingApprovals, pendingQuestions, entriesOfSession, nowSummary, sessionState, splitSessions, type HubSession } from './estado.ts'

/** O mesmo `t` de `useT`, sem o módulo de estado (que mexe no documento ao carregar). */
const tradutor = (idioma: AppLanguage): Tradutor => {
  const plano = traduzir as (i: AppLanguage, chave: string, params?: Record<string, string | number>) => string
  const plural = traduzirPlural as (i: AppLanguage, base: string, n: number) => string
  return Object.assign((chave: string, params?: Record<string, string | number>) => plano(idioma, chave, params), {
    plural: (base: string, n: number) => plural(idioma, base, n),
  }) as unknown as Tradutor
}

const session = (id: string, extra: Partial<HubSession> = {}): HubSession => ({
  id, chatId: 'ana', principal: id === 'ana', origin: { kind: 'owner' }, sourceSession: null, parentSession: null, depth: 0,
  title: id, titleSource: 'owner', state: 'open', closedReason: null, closedAt: null, createdAt: 0, lastUsedAt: 0, revision: 1, ...extra,
})
const entry = (id: string, kind: string, sessionId?: string, data: Entry['data'] = {}): Entry =>
  ({ id, seq: 0, botId: 'ana', rev: 0, kind, turn: 1, data, createdAt: 0, updatedAt: 0, ...(sessionId ? { sessionId } : {}) })
const bot = (status: Bot['status'], workingChat: string | null = null) => ({ id: 'ana', status, workingChat, pendingQuestions: [] }) as unknown as Bot

test('estado da sessão: rodando onde foi o último pedido; pede você com aprovação pendente', () => {
  const entries = [entry('1', 'user', 'reels'), entry('2', 'permission', 'caixa', { status: 'pending' }), entry('3', 'user', 'legendas')]
  const a = chatActivity('ana', [bot('working')], entries)
  assert.equal(sessionState(session('legendas'), a), 'running')
  assert.equal(sessionState(session('caixa'), a), 'needsYou')
  assert.equal(sessionState(session('reels'), a), 'idle')
  assert.equal(sessionState(session('reels', { state: 'closed' }), a), 'archived')
  // O campo do Hub vence a derivação.
  assert.equal(sessionState(session('reels', { activity: 'running' }), a), 'running')
  // Trabalho em outra conversa não conta nesta.
  assert.equal(sessionState(session('legendas'), chatActivity('ana', [bot('working', 'ship')], entries)), 'idle')
  // Sem sessionId (Hub antigo), tudo é da principal.
  assert.equal(sessionState(session('ana'), chatActivity('ana', [bot('needsInput')], [entry('1', 'user')])), 'needsYou')
})

test('estado da sessão: "pede você" vem de uma lista de pendências, e uma segunda fonte entra sem mudar nada', () => {
  const entries = [entry('1', 'user', 'reels'), entry('2', 'permission', 'caixa', { status: 'pending', author: 'ana' }), entry('3', 'permission', 'velha', { status: 'answered' })]
  assert.deepEqual(pendingApprovals('ana', entries), [{ botId: 'ana', sessionId: 'caixa' }])
  // Uma fonte falsa (como a pergunta pendente da Torre) somada às aprovações.
  const question = [{ botId: 'ana', sessionId: 'reels' }]
  const a = chatActivity('ana', [bot('idle')], entries, [...pendingApprovals('ana', entries), ...question])
  assert.equal(sessionState(session('caixa'), a), 'needsYou')
  assert.equal(sessionState(session('reels'), a), 'needsYou')
  assert.equal(sessionState(session('velha'), a), 'idle')
  assert.equal(combinedState([sessionState(session('ana'), a), sessionState(session('reels'), a)]), 'needsYou')
})

test('estado da sessão: a pergunta pendente ao dono (MB-1b) é a 2ª fonte; sem sessionId, vale a principal', () => {
  // Dublê no tipo oficial (shared/fork-owner-question.ts): o Hub manda needsInput sem turno rodando.
  const questions: PendingQuestion[] = [
    { entryId: 'q1', chatId: 'ana', sessionId: 'reels', text: 'Posto hoje?', askedAt: 1 },
    { entryId: 'q2', chatId: 'ana', text: 'E o tom?', askedAt: 2 },
    { entryId: 'q3', chatId: 'outro', sessionId: 'x', text: 'Outra conversa', askedAt: 3 },
  ]
  const asking = { ...bot('needsInput'), pendingQuestions: questions } as Bot
  assert.deepEqual(pendingQuestions('ana', [asking]), [{ botId: 'ana', sessionId: 'reels' }, { botId: 'ana', sessionId: 'ana' }])
  // Hub antigo ou cache: o normalizeBot entrega [].
  assert.deepEqual(pendingQuestions('ana', [{ ...bot('idle'), pendingQuestions: [] } as Bot]), [])
  const entries = [entry('1', 'user', 'caixa')]
  const a = chatActivity('ana', [asking], entries, [...pendingApprovals('ana', entries), ...pendingQuestions('ana', [asking])])
  assert.equal(sessionState(session('reels'), a), 'needsYou')
  assert.equal(sessionState(session('ana'), a), 'needsYou')
  // A pendência diz onde é: a sessão atual não herda o "pede você" do status.
  assert.equal(sessionState(session('caixa'), a), 'idle')
  // O bot soma as sessões: com só a pergunta, ele aguarda você.
  assert.equal(combinedState([sessionState(session('ana'), a), sessionState(session('caixa'), a)]), 'needsYou')
  // Só pergunta e nada localizado (ex.: pergunta de outra conversa): o status não vira "pede você" aqui.
  const elsewhere = { ...bot('needsInput'), pendingQuestions: [questions[2]!] } as Bot
  assert.equal(sessionState(session('ana'), chatActivity('ana', [elsewhere], [], pendingQuestions('ana', [elsewhere]))), 'idle')
})

test('estado do bot soma as sessões e o Agora resume', () => {
  assert.equal(combinedState(['idle', 'running', 'archived']), 'running')
  assert.equal(combinedState(['running', 'needsYou']), 'needsYou')
  assert.equal(combinedState(['archived']), 'idle')
  assert.equal(nowSummary(['running', 'running', 'needsYou', 'idle'], tradutor('pt-BR')).text, '2 rodando · 1 pede você')
  assert.equal(nowSummary(['idle'], tradutor('pt-BR')).text, 'Livre')
  assert.equal(nowSummary(['running', 'needsYou', 'needsYou'], tradutor('en')).text, '1 running · 2 need you')
})

test('sessões: principal fora, abertas por uso, arquivadas à parte; entradas por sessão', () => {
  const list = [session('ana'), session('a', { lastUsedAt: 1 }), session('b', { lastUsedAt: 3 }), session('c', { state: 'closed' })]
  const { open, archived } = splitSessions(list)
  assert.deepEqual(open.map((s) => s.id), ['b', 'a'])
  assert.deepEqual(archived.map((s) => s.id), ['c'])
  const entries = [entry('1', 'user'), entry('2', 'user', 'a'), entry('3', 'user', 'sumida'), entry('4', 'user', 'ana')]
  const known = new Set(['a', 'b', 'c'])
  assert.deepEqual(entriesOfSession(entries, 'ana', null, known).map((e) => e.id), ['1', '3', '4'])
  assert.deepEqual(entriesOfSession(entries, 'ana', 'a', known).map((e) => e.id), ['2'])
  assert.equal(entriesOfSession(entries, 'ana', null, new Set()), entries)
})
