import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  activeSpaceAndPrincipal,
  countActiveProposalTasks,
  decideBody,
  includedPendingItemIds,
  offersBergaProposals,
  parseProposalDecideAnswer,
  parseTaskProposals,
  resolveItemStartBadge,
  taskStateBadge,
  visibleProposals,
  type ProposalItem,
  type TaskProposal,
} from './fork-propostas.ts'
import type { SpacesSnapshot } from './fork-spaces.ts'

test('só a edição Open não monta o cartão de propostas', () => {
  assert.equal(offersBergaProposals({ appScheme: 'mexerica-open' }), false)
  for (const appScheme of ['donnabot', 'donnabot-dev', undefined]) {
    assert.equal(offersBergaProposals({ appScheme }), true)
  }
})

const origin = { hubId: 'hub-1', spaceId: 'space-a' }
const UUID = '11111111-1111-4111-8111-111111111111'

const makeItem = (id: string, extra: Partial<ProposalItem> = {}): ProposalItem => ({
  id,
  title: `Item ${id}`,
  description: 'Descrição do item',
  acceptance: 'Critério de aceite',
  assigneeBotId: 'bot-1',
  included: true,
  taskId: null,
  taskState: null,
  startState: null,
  ...extra,
})

const makeProposal = (id: string, extra: Partial<TaskProposal> = {}): TaskProposal => ({
  id,
  version: 1,
  state: 'pending',
  botId: 'berga',
  projectId: 'proj-1',
  projectName: 'Projeto Um',
  title: `Proposta ${id}`,
  createdAt: 1000,
  decidedAt: null,
  items: [makeItem('i1')],
  ...extra,
})

test('parseTaskProposals valida origem, versão, limites e estados estritamente', () => {
  const raw = {
    version: 1,
    hubId: 'hub-1',
    spaceId: 'space-a',
    limits: { perBot: 1, total: 2 },
    proposals: [
      makeProposal('p1', {
        items: [
          makeItem('i1'),
          makeItem('i2', { assigneeBotId: null, included: false, taskId: 't2', taskState: 'review', startState: 'started' }),
        ],
      }),
    ],
  }
  const snap = parseTaskProposals(raw, origin)
  assert.equal(snap.proposals.length, 1)
  assert.deepEqual(snap.limits, { perBot: 1, total: 2 })
  assert.equal(snap.proposals[0]!.items[1]!.taskState, 'review')

  assert.throws(() => parseTaskProposals({ ...raw, version: 2 }, origin), /version/)
  assert.throws(() => parseTaskProposals({ ...raw, hubId: 'outro' }, origin), /hubId/)
  assert.throws(() => parseTaskProposals({ ...raw, spaceId: 'outro' }, origin), /spaceId/)
  assert.throws(() => parseTaskProposals({ ...raw, limits: { perBot: -1, total: 2 } }, origin), /perBot/)
  assert.throws(
    () => parseTaskProposals({ ...raw, proposals: [makeProposal('p1', { state: 'unknown' as never })] }, origin),
    /proposal\.state/,
  )
  assert.throws(
    () => parseTaskProposals({ ...raw, proposals: [makeProposal('p1', { items: [makeItem('i1', { taskState: 'aprovado' as never })] })] }, origin),
    /taskState/,
  )
  assert.throws(
    () => parseTaskProposals({ ...raw, proposals: [makeProposal('p1', { items: [makeItem('i1', { startState: 'queued' as never })] })] }, origin),
    /startState/,
  )
  assert.throws(
    () => parseTaskProposals({ ...raw, proposals: [makeProposal('p1', { items: [{ ...makeItem('i1'), included: 'sim' }] as never })] }, origin),
    /included/,
  )
})

test('parseProposalDecideAnswer e decideBody conferem contrato e omissões', () => {
  const p = makeProposal('p1', { version: 3 })
  const approveBody = decideBody(origin, p, 'approve', UUID, ['i2', 'i2', 'i3'])
  assert.deepEqual(approveBody, {
    hubId: 'hub-1',
    spaceId: 'space-a',
    proposalId: 'p1',
    expectedVersion: 3,
    clientRequestId: UUID,
    decision: 'approve',
    omitItemIds: ['i2', 'i3'],
  })

  const rejectBody = decideBody(origin, p, 'reject', UUID, ['i2'])
  assert.equal('omitItemIds' in rejectBody, false)

  assert.throws(() => decideBody(origin, p, 'approve', 'nao-uuid'), /clientRequestId/)

  const decided = makeProposal('p1', { version: 4, state: 'approved', decidedAt: 2000 })
  const parsed = parseProposalDecideAnswer({ proposal: decided }, origin, 'p1')
  assert.equal(parsed.proposal.version, 4)
  assert.throws(() => parseProposalDecideAnswer({ proposal: decided }, origin, 'outra'), /proposal\.id/)
  assert.throws(() => parseProposalDecideAnswer({ hubId: 'hub-9', proposal: decided }, origin), /hubId/)
})

test('review é selo de conferência aguardando o dono e nunca parece aprovado', () => {
  const review = taskStateBadge('review')
  const done = taskStateBadge('done')
  assert.equal(review.key, 'propostas.estadoTarefa.review')
  assert.equal(review.tone, 'attention')
  assert.equal(review.awaitingOwner, true)
  assert.notEqual(review.tone, done.tone)
  assert.equal(done.tone, 'success')
  assert.equal(done.awaitingOwner, false)
})

test('scheduled vira Aguardando vaga com 2 ativas ou quando o bot já ocupa a vaga dele', () => {
  const limits = { perBot: 1, total: 2 }
  const scheduledB3 = makeItem('i3', { assigneeBotId: 'bot-3', taskId: 't3', taskState: 'backlog', startState: 'scheduled' })
  const scheduledB1 = makeItem('i4', { assigneeBotId: 'bot-1', taskId: 't4', taskState: 'backlog', startState: 'scheduled' })

  const pOneActive = makeProposal('p1', {
    state: 'approved',
    items: [
      makeItem('i1', { assigneeBotId: 'bot-1', taskId: 't1', taskState: 'in_progress', startState: 'started' }),
      scheduledB3,
      scheduledB1,
    ],
  })
  const countsOne = countActiveProposalTasks([pOneActive])
  assert.equal(countsOne.total, 1)
  assert.equal(resolveItemStartBadge(scheduledB3, limits, countsOne)?.key, 'propostas.inicio.scheduled')
  assert.equal(resolveItemStartBadge(scheduledB1, limits, countsOne)?.key, 'propostas.inicio.aguardandoVaga')

  const pTwoActive = makeProposal('p2', {
    state: 'approved',
    items: [
      makeItem('i1', { assigneeBotId: 'bot-1', taskId: 't1', taskState: 'in_progress', startState: 'started' }),
      makeItem('i2', { assigneeBotId: 'bot-2', taskId: 't2', taskState: 'in_progress', startState: 'started' }),
      scheduledB3,
    ],
  })
  const countsTwo = countActiveProposalTasks([pTwoActive])
  assert.equal(countsTwo.total, 2)
  assert.equal(resolveItemStartBadge(scheduledB3, limits, countsTwo)?.key, 'propostas.inicio.aguardandoVaga')

  // Tarefa em conferência ou concluída não ocupa vaga de execução ativa.
  const pOneReview = makeProposal('p3', {
    state: 'approved',
    items: [
      makeItem('i1', { assigneeBotId: 'bot-1', taskId: 't1', taskState: 'review', startState: 'started' }),
      makeItem('i2', { assigneeBotId: 'bot-2', taskId: 't2', taskState: 'in_progress', startState: 'started' }),
      scheduledB3,
    ],
  })
  const countsReview = countActiveProposalTasks([pOneReview])
  assert.equal(countsReview.total, 1)
  assert.equal(resolveItemStartBadge(scheduledB3, limits, countsReview)?.key, 'propostas.inicio.scheduled')
})

test('visibilidade mantém pendentes e aprovadas com tarefa aberta; esconde concluídas e recusadas', () => {
  const pending = makeProposal('p-pend', { state: 'pending' })
  const approvedOpen = makeProposal('p-open', {
    state: 'approved',
    items: [makeItem('i1', { taskId: 't1', taskState: 'review', startState: 'started' })],
  })
  const approvedDone = makeProposal('p-done', {
    state: 'approved',
    items: [
      makeItem('i1', { taskId: 't1', taskState: 'done', startState: 'started' }),
      makeItem('i2', { included: false, taskId: null, taskState: null }),
    ],
  })
  const rejected = makeProposal('p-rej', { state: 'rejected' })

  const list = visibleProposals({
    version: 1,
    hubId: 'hub-1',
    spaceId: 'space-a',
    limits: { perBot: 1, total: 2 },
    proposals: [approvedDone, approvedOpen, rejected, pending],
  })
  assert.deepEqual(list.map((p) => p.id), ['p-pend', 'p-open'])
  assert.deepEqual(includedPendingItemIds(makeProposal('x', { items: [makeItem('a'), makeItem('b'), makeItem('c', { included: false })] }), new Set(['a'])), ['b'])
})

test('activeSpaceAndPrincipal descobre o principal do Space ativo e nunca presume o Pessoal', () => {
  const spaces: SpacesSnapshot = {
    version: 1,
    personalSpaceId: 'pessoal',
    revision: 1,
    spaces: [
      { id: 'pessoal', kind: 'personal', name: 'Pessoal', position: 0, revision: 1, principalBotId: 'berga-pessoal', tile: null, appearanceConfigured: false, appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null },
      { id: 'estudio', kind: 'space', name: 'Estúdio', position: 1, revision: 1, principalBotId: 'berga-estudio', tile: null, appearanceConfigured: true, appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null },
      { id: 'legado', kind: 'space', name: 'Legado', position: 2, revision: 1, principalBotId: null, tile: null, appearanceConfigured: true, appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null },
    ],
  }

  assert.deepEqual(activeSpaceAndPrincipal({ spaceId: 'estudio', spaces, principal: { id: 'berga-pessoal' } }), { spaceId: 'estudio', principalBotId: 'berga-estudio' })
  assert.deepEqual(activeSpaceAndPrincipal({ spaceId: 'legado', spaces, principal: { id: 'berga-pessoal' } }), { spaceId: 'legado', principalBotId: null })
  assert.deepEqual(activeSpaceAndPrincipal({ spaceId: undefined, hello: { spaces }, principal: { id: 'berga-pessoal' } }), { spaceId: 'pessoal', principalBotId: 'berga-pessoal' })
})
