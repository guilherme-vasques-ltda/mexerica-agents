import { test } from 'node:test'
import assert from 'node:assert/strict'
import { countByTone, groupBots, groupOf, listedBots, openTarget, parseTowerSnapshot, queueOf, statusOf, uninformedCount, usageOf, canOpenToStop } from './fork-torre.ts'

// VG-02: ausente aparece como ausente; nada de outro Hub/space é pintado; Abrir só navega ao destino exato.
const U = { availability: 'unavailable' }
const base = (bots: unknown[]) => ({ version: 1, hubId: 'h1', snapshotId: 's', collectedAt: 1, spaceId: 'p', bots })
const bot = (botId: string, runtime: unknown, extra: Record<string, unknown> = {}) => ({
  botId, name: botId, title: null, avatarColor: '', avatarShape: '', avatarConfig: null, runtime, open: null,
  pending: U, queue: U, usage: { dailyBotTokens: U, subscriptionLimit: U, contextTokens: U }, ...extra,
})
const working = { availability: 'available', source: 'hub.runtime_audiencia', coverage: 'c', value: { status: 'working', activity: 'A', startedAt: 5, resumeAt: null, reason: null, conversation: null } }

test('indisponível não vira Livre, zero nem fila vazia; desconhecido é indisponível', () => {
  const snap = parseTowerSnapshot(base([bot('a', U), bot('b', { availability: 'available', source: 's', coverage: 'c', value: { status: 'dormindo' } }), bot('c', working)]), { hubId: 'h1', spaceId: 'p' })
  const [a, b, c] = snap.bots
  for (const x of [a!, b!]) {
    assert.deepEqual(statusOf(x), { tone: 'unknown' })
    assert.equal(groupOf(x), null)
  }
  assert.deepEqual(statusOf(c!), { tone: 'working' })
  assert.deepEqual(countByTone(snap.bots), { working: 1, needsInput: 0, waiting: 0, error: 0, idle: 0, unknown: 2 })
  // Sem medição: null (a tela diz “Fila não disponível”/“Não informado”), nunca 0.
  assert.equal(queueOf(a!), null)
  assert.equal(usageOf(a!.usage.dailyBotTokens), null)
  // Pendência original disponível leva o bot a “Esperam você”, mesmo trabalhando.
  const p = parseTowerSnapshot(base([bot('d', working, { pending: { availability: 'available', source: 'entries.permission', coverage: 'c', value: [{ entryId: 'e1', sessionRef: { id: 'd', chatId: 'd' }, kind: 'permission', status: 'pending', conversation: { conversationId: 'd', revision: 1 } }] } })]), { hubId: 'h1', spaceId: 'p' })
  assert.equal(groupOf(p.bots[0]!), 'waitingYou')
})

test('D341: Livre e sem estado ficam fora do Board; erro, aguardando e pendência continuam', () => {
  const rt = (status: string) => ({ availability: 'available', source: 's', coverage: 'c', value: { status, activity: null, startedAt: null, resumeAt: null, reason: null, conversation: null } })
  const perm = { availability: 'available', source: 'entries.permission', coverage: 'c', value: [{ entryId: 'e1', sessionRef: { id: 'u2', chatId: 'u2' }, kind: 'permission', status: 'pending', conversation: { conversationId: 'u2', revision: 1 } }] }
  const snap = parseTowerSnapshot(base([bot('livre', rt('idle')), bot('u', U), bot('w', rt('working')), bot('e', rt('error')), bot('a', rt('waiting')), bot('u2', U, { pending: perm })]), { hubId: 'h1', spaceId: 'p' })
  assert.equal(groupOf(snap.bots[0]!), null)
  assert.deepEqual(listedBots(snap.bots).map((b) => b.botId), ['w', 'e', 'a', 'u2'])
  assert.deepEqual(groupBots(snap.bots).map((g) => g.group), ['waitingYou', 'working', 'waitingOrError'])
  assert.equal(uninformedCount(snap.bots), 1)
})

test('fotografia de outro Hub ou space é recusada', () => {
  assert.throws(() => parseTowerSnapshot(base([]), { hubId: 'outro', spaceId: 'p' }), /torre_invalid_answer/)
  assert.throws(() => parseTowerSnapshot(base([]), { hubId: 'h1', spaceId: 'loja' }), /torre_invalid_answer/)
})

test('Abrir: conversa exata no space; no Pessoal só a do próprio bot, sem cair na principal; parar só com trabalho', () => {
  assert.deepEqual(openTarget('conv-9', 'b', 'loja', 'p'), { kind: 'space', spaceId: 'loja', conversationId: 'conv-9' })
  assert.deepEqual(openTarget('b', 'b', 'p', 'p'), { kind: 'personal', botId: 'b' })
  assert.equal(openTarget('outra', 'b', 'p', 'p'), null)
  const open = { label: 'Abrir conversa do bot', botId: 'c', conversation: { conversationId: 'c', revision: 1 }, sessionRef: null }
  const snap = parseTowerSnapshot(base([bot('c', working, { open }), bot('x', U, { open: { ...open, botId: 'c' } })]), { hubId: 'h1', spaceId: 'p' })
  assert.equal(canOpenToStop(snap.bots[0]!), true)
  // Destino de outro bot não é navegação deste cartão.
  assert.equal(snap.bots[1]!.open, null)
  assert.equal(canOpenToStop(snap.bots[1]!), false)
})
