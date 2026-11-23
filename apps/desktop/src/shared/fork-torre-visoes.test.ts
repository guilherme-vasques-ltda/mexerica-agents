import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseTowerViewSnapshot, parseWorkCard, type TowerViewSnapshot, type WorkCard } from './fork-torre-visoes.ts'

const scope = { hubId: 'hub-1', spaceId: 'space-1' }
const overview = { ...scope, perspective: 'overview' } as const
const director = { ...scope, perspective: 'director', directorBotId: 'director-1' } as const
const bot = { ...scope, perspective: 'bot', botId: 'bot-1' } as const
const card: WorkCard = {
  taskId: 'task-1', projectId: 'project-1', projectName: 'Jardim', spaceId: scope.spaceId, parentTaskId: null,
  title: 'Regar as plantas', state: 'backlog', bucket: 'backlog',
  origin: { kind: 'owner_direct', rootBotId: null, delegatedByBotId: null, routineId: null, admittedAt: null },
  requesterLabel: 'owner', requesterBotId: null, approverRole: 'owner', directorBotId: null, assigneeBotId: bot.botId,
  nextAction: null, blocker: { kind: 'none', reason: null, attentionItemId: null },
  delivery: { hasEvidence: false, evidenceSummary: null, acceptedByOwner: false },
  declaredUpdatedAt: 1, observedExecution: { active: false, status: 'unavailable', observedAt: null, machineLabel: null },
  capabilities: { canDirectorInterrupt: false, canOwnerAccept: false },
}
const snapshot: TowerViewSnapshot = {
  version: 1, ...overview, directorBotId: null, botId: null, collectedAt: 2, cards: [card],
  counts: { needsYou: 0, observedWorking: 0, waitingOrBlocked: 0, totalVisible: 1 },
}

test('as três perspectivas preservam a ficha e descartam campos fora do contrato', () => {
  const wire = { ...snapshot, extra: true, cards: [{ ...card, extra: true, origin: { ...card.origin, extra: true } }] }
  assert.deepEqual(parseTowerViewSnapshot(wire, overview), snapshot)
  assert.deepEqual(parseTowerViewSnapshot(wire), snapshot)
  assert.deepEqual(parseWorkCard(card), card)
  assert.deepEqual(parseTowerViewSnapshot({ ...wire, ...director }, director), { ...snapshot, ...director })
  assert.deepEqual(parseTowerViewSnapshot({ ...wire, ...bot }, bot), { ...snapshot, ...bot })
})

test('recusa outra origem ou perspectiva e seletores inesperados', () => {
  for (const bad of [null, { ...snapshot, version: 2 }, { ...snapshot, hubId: 'hub-2' }, { ...snapshot, spaceId: 'space-2' },
    { ...snapshot, perspective: 'unknown' }, { ...snapshot, directorBotId: director.directorBotId },
    { ...snapshot, botId: undefined }, { ...snapshot, collectedAt: Infinity }]) {
    assert.equal(parseTowerViewSnapshot(bad, overview), null)
  }
  assert.equal(parseTowerViewSnapshot({ ...snapshot, ...director, directorBotId: 'director-2' }, director), null)
  assert.equal(parseTowerViewSnapshot({ ...snapshot, ...bot, botId: 'bot-2' }, bot), null)
  assert.equal(parseTowerViewSnapshot({ ...snapshot, ...bot, botId: null }), null)
})

test('a projeção autorizada não exige que um participante seja o executor atribuído', () => {
  const visible = { ...snapshot, ...bot, cards: [{ ...card, assigneeBotId: 'bot-2' }] }
  assert.deepEqual(parseTowerViewSnapshot(visible, bot), visible)
})

test('recusa a ficha inteira quando falta sinal ou ela declara origem, ação ou execução incompatível', () => {
  for (const bad of [null, { ...card, spaceId: 'space-2' }, { ...card, state: 'archived' }, { ...card, bucket: 'working' },
    { ...card, parentTaskId: undefined }, { ...card, approverRole: 'bot' }, { ...card, requesterLabel: 'human' },
    { ...card, origin: { ...card.origin, rootBotId: 'bot-2' } }, { ...card, origin: { ...card.origin, kind: 'manual' } },
    { ...card, blocker: { ...card.blocker, kind: 'waiting' } }, { ...card, delivery: { ...card.delivery, hasEvidence: 'false' } },
    { ...card, capabilities: { ...card.capabilities, canDirectorInterrupt: true } },
    { ...card, origin: { ...card.origin, kind: 'unknown' }, requesterLabel: 'unknown',
      capabilities: { ...card.capabilities, canDirectorInterrupt: true } },
    { ...card, capabilities: { canDirectorInterrupt: false } }, { ...card, declaredUpdatedAt: -1 },
    { ...card, observedExecution: { ...card.observedExecution, machineLabel: 'machine.example.invalid' } },
    { ...card, observedExecution: { ...card.observedExecution, status: 'active' } },
    { ...card, observedExecution: { ...card.observedExecution, observedAt: undefined } },
    { ...card, nextAction: '🍊'.repeat(241) }]) {
    assert.equal(parseWorkCard(bad, scope.spaceId), null)
    assert.equal(parseTowerViewSnapshot({ ...snapshot, cards: [bad], counts: snapshot.counts }, overview), null)
  }
})

test('o bucket observado não inventa execução ativa nem máquina', () => {
  const observed = { ...card, bucket: 'observed_working', state: 'in_progress' }
  const wire = { ...snapshot, cards: [observed], counts: { ...snapshot.counts, observedWorking: 1 } }
  assert.deepEqual(parseTowerViewSnapshot(wire, overview), wire)
})

test('contagens correspondem somente às fichas visíveis e não aceitam duplicatas', () => {
  const cards: WorkCard[] = [card, { ...card, taskId: 'task-2', state: 'review', bucket: 'needs_you' },
    { ...card, taskId: 'task-3', state: 'in_progress', bucket: 'observed_working',
      observedExecution: { active: true, status: 'working', observedAt: 2, machineLabel: 'machine.example.invalid' } },
    { ...card, taskId: 'task-4', state: 'blocked', bucket: 'waiting_or_blocked' },
    { ...card, taskId: 'task-5', state: 'done', bucket: 'done' }]
  const counts = { needsYou: 1, observedWorking: 1, waitingOrBlocked: 1, totalVisible: 5 }
  assert.deepEqual(parseTowerViewSnapshot({ ...snapshot, cards, counts: { ...counts, hidden: 20 } }, overview), { ...snapshot, cards, counts })
  assert.equal(parseTowerViewSnapshot({ ...snapshot, cards, counts: { ...counts, totalVisible: 20 } }, overview), null)
  assert.equal(parseTowerViewSnapshot({ ...snapshot, cards, counts: { ...counts, needsYou: 2 } }, overview), null)
  assert.equal(parseTowerViewSnapshot({ ...snapshot, cards: [card, card], counts: { ...snapshot.counts, totalVisible: 2 } }, overview), null)
  const empty = { ...snapshot, cards: [], counts: { ...snapshot.counts, totalVisible: 0 } }
  assert.deepEqual(parseTowerViewSnapshot(empty, overview), empty)
})
