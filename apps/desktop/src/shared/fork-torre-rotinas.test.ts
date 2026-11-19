import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseTowerSnapshot } from './fork-torre.ts'
import { parseRoutineCreations, parseRoutineReviewResult } from './fork-torre-rotinas.ts'

const item = { entryId: 'notice-1', entryRev: 7, routineId: 'routine-1', botId: 'bot-1', botName: 'Berga',
  name: 'Revisar o jardim', createdAt: 1_700_000_000_000, when: 'Toda sexta às 9h', nextRunAt: null, status: 'active' as const, externalReleasePending: false, routineRevision: 'a'.repeat(64) }
const block = { botId: 'bot-1', botName: 'Berga', limit: 10, used: 10, retryAt: 1_700_086_400_000,
  reason: 'Cota atingida.', howToIncrease: 'O ajuste estará em Trabalho que ninguém pediu quando disponível.', settingsAvailable: false }
const raw = { version: 1, hubId: 'hub-1', spaceId: 'space-1', snapshotId: 'snapshot-1', collectedAt: 1, bots: [] }
const expected = { hubId: 'hub-1', spaceId: 'space-1' }
const observed = (value: unknown) => ({ availability: 'available', source: 'hub', coverage: 'space', value })

test('Hub antigo e projeção malformada ficam indisponíveis; não viram zero criações', () => {
  assert.deepEqual(parseTowerSnapshot(raw, expected).routineCreations, { availability: 'unavailable' })
  const malformed = [null, {}, { items: [], blocked: null }, { items: [{ ...item, status: 'pending' }], blocked: [] },
    { items: [{ ...item, nextRunAt: undefined }], blocked: [] }, { items: [{ ...item, entryRev: NaN }], blocked: [] },
    { items: [{ ...item, botName: ' ' }], blocked: [] }, { items: [item, item], blocked: [] },
    { items: [], blocked: [{ ...block, settingsAvailable: 'false' }] }, { items: [], blocked: [{ ...block, used: -1 }] },
    { items: [], blocked: [{ ...block, retryAt: Infinity }] }, { items: [], blocked: [block, block] }]
  for (const value of malformed) {
    assert.equal(parseRoutineCreations(value), null)
    assert.deepEqual(parseTowerSnapshot({ ...raw, routineCreations: observed(value) }, expected).routineCreations, { availability: 'unavailable' })
  }
})

test('preserva criações ativas/pausadas e diagnóstico sem inventar rotina para a cota', () => {
  const value = { items: [item, { ...item, entryId: 'notice-2', routineId: 'routine-2', status: 'paused', nextRunAt: 1_700_086_400_000 }], blocked: [block] }
  assert.deepEqual(parseRoutineCreations(value), value)
  assert.deepEqual(parseRoutineCreations({ items: [], blocked: [block] }), { items: [], blocked: [block] })
  assert.deepEqual(parseTowerSnapshot({ ...raw, routineCreations: observed(value) }, expected).routineCreations, observed(value))
})

test('resposta resolvida só confirma a identidade e decisão solicitadas, com revisão válida', () => {
  const answer = { entryId: item.entryId, routineId: item.routineId, decision: 'kept', rev: 8 }
  assert.deepEqual(parseRoutineReviewResult(answer, item, 'keep'), answer)
  assert.deepEqual(parseRoutineReviewResult({ ...answer, decision: 'cancelled' }, item, 'cancel'), { ...answer, decision: 'cancelled' })
  assert.deepEqual(parseRoutineReviewResult({ ...answer, decision: 'enabled' }, item, 'enable'), { ...answer, decision: 'enabled' })
  for (const bad of [undefined, {}, { ...answer, entryId: 'outra' }, { ...answer, routineId: 'outra' },
    { ...answer, decision: 'cancelled' }, { ...answer, rev: NaN }, { ...answer, rev: -1 },
    { ...answer, rev: item.entryRev }, { ...answer, rev: item.entryRev - 1 }]) {
    assert.throws(() => parseRoutineReviewResult(bad, item, 'keep'), /routine_review_invalid_answer/)
  }
  assert.throws(() => parseRoutineReviewResult(answer, item, 'cancel'), /routine_review_invalid_answer/)
})

test('somente sinal explícito de rotina externa pausada oferece liberação', () => {
  assert.equal(parseRoutineCreations({ items: [{ ...item, externalReleasePending: undefined }], blocked: [] })?.items[0]?.externalReleasePending, false)
  assert.equal(parseRoutineCreations({ items: [{ ...item, status: 'paused', externalReleasePending: true }], blocked: [] })?.items[0]?.externalReleasePending, true)
  assert.equal(parseRoutineCreations({ items: [{ ...item, externalReleasePending: true }], blocked: [] })?.items[0]?.externalReleasePending, false)
  assert.equal(parseRoutineCreations({ items: [{ ...item, externalReleasePending: 'true' }], blocked: [] }), null)
})
