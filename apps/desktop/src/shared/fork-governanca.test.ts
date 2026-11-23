import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseGovernancePolicy, parseGovernanceSnapshot } from './fork-governanca.ts'

const expected = { hubId: 'hub-1', spaceId: 'space-1' }
const policy = {
  titleInstruction: '', autoArchiveIdleDays: 3, preservePinnedAndOpenWork: true,
  maxConcurrentSubagents: 12, maxSubagentDepth: 3,
}
const snapshot = { version: 1, ...expected, revision: 0, policy }

test('valida e projeta só os campos da política recebida do Hub', () => {
  const lower = { ...policy, autoArchiveIdleDays: 0, maxConcurrentSubagents: 1, maxSubagentDepth: 1 }
  const upper = { ...policy, titleInstruction: '🍊'.repeat(500), autoArchiveIdleDays: 365, maxConcurrentSubagents: 32, maxSubagentDepth: 5 }
  assert.deepEqual(parseGovernancePolicy(lower), lower)
  assert.deepEqual(parseGovernanceSnapshot(snapshot), snapshot)
  assert.deepEqual(parseGovernanceSnapshot({ ...snapshot, extra: true, policy: { ...upper, extra: true } }, expected),
    { ...snapshot, policy: upper })
})

test('recusa configuração ausente ou fora dos limites em vez de aplicar valores padrão', () => {
  for (const bad of [null, {}, { ...policy, titleInstruction: '🍊'.repeat(501) }, { ...policy, titleInstruction: null },
    { ...policy, autoArchiveIdleDays: -1 }, { ...policy, autoArchiveIdleDays: 366 }, { ...policy, autoArchiveIdleDays: 0.5 },
    { ...policy, preservePinnedAndOpenWork: false }, { ...policy, preservePinnedAndOpenWork: 'true' },
    { ...policy, preservePinnedAndOpenWork: undefined }, { ...policy, maxConcurrentSubagents: 0 },
    { ...policy, maxConcurrentSubagents: 33 }, { ...policy, maxConcurrentSubagents: '12' },
    { ...policy, maxSubagentDepth: 0 }, { ...policy, maxSubagentDepth: 6 }, { ...policy, maxSubagentDepth: NaN }]) {
    assert.equal(parseGovernancePolicy(bad), null)
    assert.equal(parseGovernanceSnapshot({ ...snapshot, policy: bad }, expected), null)
  }
})

test('recusa snapshot de outra origem, versão ou revisão inválida', () => {
  for (const bad of [null, { ...snapshot, version: 2 }, { ...snapshot, hubId: 'hub-2' }, { ...snapshot, spaceId: 'space-2' },
    { ...snapshot, revision: undefined }, { ...snapshot, revision: -1 }, { ...snapshot, revision: 1.5 },
    { ...snapshot, revision: Number.MAX_SAFE_INTEGER + 1 }]) {
    assert.equal(parseGovernanceSnapshot(bad, expected), null)
  }
})
