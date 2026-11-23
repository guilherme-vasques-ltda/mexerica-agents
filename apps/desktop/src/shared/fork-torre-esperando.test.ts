import assert from 'node:assert/strict'
import test from 'node:test'
import { parseAttentionObservation } from './fork-torre-esperando.ts'

const expected = { hubId: 'hub', spaceId: 'personal' }
const target = { entryId: 'e1', botId: 'target', conversation: { conversationId: 'chat', revision: 2 } }
const row = (kind: string, id: string) => ({ id, dedupeKey: 'permission:e1', kind, severity: 'blocking', spaceId: 'personal', titleKey: 'attention.permission', summary: null, actionKind: 'respond_permission', approverRole: 'owner', createdAt: 10, target: { ...target, ...(kind === 'delegated_permission' ? { requesterBotId: 'requester' } : {}) } })
const answer = (items: unknown[], gaps: unknown[] = [], resolvedToday: unknown[] = []) => ({ availability: 'available', source: 'fixture', coverage: 'fixture', value: { version: 1, ...expected, collectedAt: 11, items, resolvedToday, coverageGaps: gaps } })

test('deduplicates direct permission in favor of delegated checkpoint', () => {
  const parsed = parseAttentionObservation(answer([row('permission','direct'), row('delegated_permission','delegated')]), expected)
  assert.equal(parsed.availability, 'available')
  if (parsed.availability === 'available') assert.deepEqual(parsed.value.items.map((item) => item.kind), ['delegated_permission'])
})

test('preserves explicit coverage gaps and confirmed resolutions', () => {
  const resolved = { id: 'resolved', kind: 'permission', titleKey: 'attention.permission', summary: null, resolvedAt: 9, resolutionKind: 'approved', target }
  const parsed = parseAttentionObservation(answer([], [{ source: 'proposals', code: 'source_unavailable' }], [resolved]), expected)
  assert.equal(parsed.availability, 'available')
  if (parsed.availability === 'available') {
    assert.equal(parsed.value.coverageGaps[0]?.source, 'proposals')
    assert.equal(parsed.value.resolvedToday[0]?.resolutionKind, 'approved')
  }
})

test('fails closed for malformed queue or another Space', () => {
  assert.deepEqual(parseAttentionObservation(answer([row('permission','a')]), { ...expected, spaceId: 'other' }), { availability: 'unavailable' })
  assert.deepEqual(parseAttentionObservation(answer([{ ...row('permission','a'), severity: 'unknown' }]), expected), { availability: 'unavailable' })
  assert.deepEqual(parseAttentionObservation(answer([{ ...row('delegated_permission','a'), target }]), expected), { availability: 'unavailable' })
  assert.deepEqual(parseAttentionObservation(answer([row('permission','a'), { ...row('delegated_permission','b'), target: { ...target, botId: 'different', requesterBotId: 'requester' } }]), expected), { availability: 'unavailable' })
  assert.deepEqual(parseAttentionObservation(undefined, expected), { availability: 'unavailable' })
})
