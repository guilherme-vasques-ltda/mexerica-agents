import assert from 'node:assert/strict'
import test from 'node:test'
import type { Bot, Entry } from '../../../../shared/models.ts'
import { chatActivity, combinedState, nowSummary, pendingApprovals, pendingChoices, sessionState, type HubSession } from './estado.ts'
import { sessionDetail } from './detalhe.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

// Dono, 07/11 (docs/reference/design/painel-recente-20261107): título com reserva, estado por sessão e a escolha no "pede você".
const at = (n: number) => n * 1000
const entry = (id: string, kind: string, sessionId: string | undefined, data: Entry['data'], t = 0): Entry =>
  ({ id, seq: 0, botId: 'ana', rev: 0, kind, turn: 1, data, createdAt: at(t), updatedAt: at(t), ...(sessionId ? { sessionId } : {}) })
const session = (id: string, title = ''): HubSession => ({ id, chatId: 'ana', principal: id === 'ana', origin: { kind: 'owner' }, sourceSession: null, parentSession: null,
  depth: 0, title, titleSource: 'first_message', state: 'open', closedReason: null, closedAt: null, createdAt: 0, lastUsedAt: at(1), revision: 1 })
const bot = (status: Bot['status']) => ({ id: 'ana', status, workingChat: null, pendingQuestions: [] }) as unknown as Bot
const snapshot = JSON.stringify({ version: 1, component: 'choice', title: 'Guarda', summary: 'Duas saídas.', data: { question: 'Qual primeiro?', options: [{ id: 'a', label: 'A' }, { id: 'b', label: 'B' }] } })
const choice = (id: string, sessionId: string, state = 'pending') =>
  entry(id, 'notice', sessionId, { text: 'Qual primeiro?', genuiChoice: { version: 1, questionId: `q-${id}`, revision: 1, state, snapshot } } as Entry['data'], 5)
const known = new Set(['guarda', 'docs', 'fila', 'vazia'])

test('título: o do Hub; vazio, o 1º pedido resumido; sem pedido, null ("Nova sessão")', () => {
  const entries = [entry('1', 'user', 'docs', { text: '## **Rascunho** do changelog\nresto' }, 1), entry('2', 'agent', 'docs', { text: 'Feito.' }, 2)]
  assert.equal(sessionDetail(bot('idle'), session('docs', 'Changelog'), 'idle', entries, known).title, 'Changelog')
  assert.equal(sessionDetail(bot('idle'), session('docs'), 'idle', entries, known).title, 'Rascunho do changelog')
  assert.equal(sessionDetail(bot('idle'), session('vazia'), 'idle', entries, known).title, null)
  const d = sessionDetail(bot('idle'), session('docs'), 'idle', entries, known)
  assert.deepEqual(d.last, { at: at(2), mine: false, text: 'Feito.' })
})

test('cartão de escolha pendente pede você: na sessão, no Agora e no ponto do bot', () => {
  const entries = [entry('1', 'user', 'guarda', { text: 'faz a guarda' }, 1), choice('c', 'guarda'), choice('velha', 'docs', 'answered')]
  const pending = [...pendingApprovals('ana', entries), ...pendingChoices('ana', entries)]
  assert.deepEqual(pending.map((p) => p.sessionId), ['guarda'])
  const a = chatActivity('ana', [bot('working')], entries, pending)
  const states = [sessionState(session('ana'), a), sessionState(session('guarda'), a), sessionState(session('docs'), a)]
  assert.deepEqual(states, ['idle', 'needsYou', 'idle'])
  assert.equal(combinedState(states), 'needsYou')
  const t = Object.assign(() => '', { plural: () => '' }) as unknown as Tradutor
  assert.equal(nowSummary(states, t).needs, 1)
  const d = sessionDetail(bot('working'), session('guarda'), states[1]!, entries, known)
  assert.deepEqual([d.state, d.need], ['needsYou', { kind: 'choice', text: 'Qual primeiro?' }])
})

test('na fila só com a sessão livre; arquivada não pede nada', () => {
  const entries = [entry('1', 'user', 'fila', { text: 'mais uma', status: 'queued' }, 1), entry('2', 'permission', 'docs', { status: 'pending', title: 'Editar' }, 1)]
  assert.equal(sessionDetail(bot('working'), session('fila'), 'idle', entries, known).state, 'queued')
  assert.equal(sessionDetail(bot('working'), session('fila'), 'running', entries, known).state, 'running')
  const arq = sessionDetail(bot('idle'), session('docs'), 'archived', entries, known)
  assert.deepEqual([arq.state, arq.need], ['archived', null])
})

// R1-01 (#509): pela cadeia real, a mensagem na fila não leva "Rodando" para a sessão dela.
test('bot trabalhando, A enviado e B na fila: A Rodando, B Na fila', () => {
  const entries = [entry('1', 'user', 'docs', { text: 'faz A', status: 'sent' }, 1), entry('2', 'user', 'fila', { text: 'depois B', status: 'queued' }, 2)]
  const chat = bot('working')
  const a = chatActivity('ana', [chat], entries, pendingApprovals('ana', entries))
  const detalhe = (id: string) => sessionDetail(chat, session(id), sessionState(session(id), a), entries, known).state
  assert.deepEqual([detalhe('docs'), detalhe('fila')], ['running', 'queued'])
})

// R1-02 (#509): a principal do Hub ganha a mesma reserva de título e a hora da última interação.
test('principal: título do 1º pedido, hora da última mensagem', () => {
  const entries = [entry('1', 'user', undefined, { text: 'Resumo da semana\nmais' }, 3), entry('2', 'agent', 'ana', { text: 'Pronto.' }, 4), entry('3', 'user', 'docs', { text: 'outra' }, 5)]
  const d = sessionDetail(bot('idle'), session('ana'), 'idle', entries, known)
  assert.deepEqual([d.title, d.at, d.last?.text], ['Resumo da semana', at(4), 'Pronto.'])
  assert.equal(sessionDetail(bot('idle'), session('ana', 'Do Hub'), 'idle', entries, known).title, 'Do Hub')
  assert.equal(sessionDetail(bot('idle'), session('ana'), 'idle', [], known).title, null)
})
