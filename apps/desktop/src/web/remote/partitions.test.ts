import assert from 'node:assert/strict'
import test from 'node:test'
import { createPartitions } from './partitions.ts'
import type { WorkspaceContext } from './workspace-session.ts'

const context = {
  schema: 1, hubId: 'fixture-hub', workspaceId: 'org:fixture', userId: 'fixture-user',
  workspaceEpoch: 4, kind: 'team', role: 'member', capabilities: ['chat'],
} satisfies WorkspaceContext

test('partitions by Hub, workspace and user; seals without deleting after 24 hours', () => {
  let now = 100
  const partitions = createPartitions(() => now)
  const store = partitions.authenticate(context)
  store.setItem('draft', 'fixture')
  for (const next of [
    { ...context, hubId: 'other-hub' }, { ...context, workspaceId: 'org:other' }, { ...context, userId: 'another-user' },
  ]) assert.equal(partitions.authenticate(next).getItem('draft'), null)
  now += 24 * 60 * 60 * 1000
  assert.equal(store.length, 0)
  store.removeItem('draft')
  assert.equal(partitions.authenticate(context).getItem('draft'), 'fixture')
  assert.equal(partitions.authenticate({ ...context, workspaceEpoch: 5 }).getItem('draft'), null)
})

test('signed cut retains the epoch floor and clears only the affected partition', () => {
  const partitions = createPartitions()
  const before = partitions.authenticate(context)
  before.setItem('draft', 'fixture')
  const other = partitions.authenticate({ ...context, userId: 'another-user' })
  other.setItem('draft', 'other')
  partitions.cut(context, 7)
  assert.equal(before.getItem('draft'), null)
  assert.throws(() => partitions.authenticate({ ...context, workspaceEpoch: 6 }), /epoch_rollback/)
  assert.equal(partitions.authenticate({ ...context, workspaceEpoch: 7 }).getItem('draft'), null)
  assert.equal(other.getItem('draft'), 'other')
})
