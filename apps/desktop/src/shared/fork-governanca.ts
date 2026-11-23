export interface GovernancePolicy {
  titleInstruction: string
  autoArchiveIdleDays: number
  preservePinnedAndOpenWork: true
  maxConcurrentSubagents: number
  maxSubagentDepth: number
}

export interface GovernanceSnapshot {
  version: 1
  hubId: string
  spaceId: string
  revision: number
  policy: GovernancePolicy
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const integerIn = (v: unknown, min: number, max: number): v is number =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= min && v <= max

export function parseGovernancePolicy(v: unknown): GovernancePolicy | null {
  if (!isObject(v) || typeof v.titleInstruction !== 'string' || [...v.titleInstruction].length > 500
    || !integerIn(v.autoArchiveIdleDays, 0, 365) || v.preservePinnedAndOpenWork !== true
    || !integerIn(v.maxConcurrentSubagents, 1, 32) || !integerIn(v.maxSubagentDepth, 1, 5)) return null
  return {
    titleInstruction: v.titleInstruction,
    autoArchiveIdleDays: v.autoArchiveIdleDays,
    preservePinnedAndOpenWork: v.preservePinnedAndOpenWork,
    maxConcurrentSubagents: v.maxConcurrentSubagents,
    maxSubagentDepth: v.maxSubagentDepth,
  }
}

export function parseGovernanceSnapshot(v: unknown, expected?: { hubId: string; spaceId: string }): GovernanceSnapshot | null {
  if (!isObject(v) || v.version !== 1 || typeof v.hubId !== 'string' || !v.hubId.trim()
    || typeof v.spaceId !== 'string' || !v.spaceId.trim()
    || (expected && (v.hubId !== expected.hubId || v.spaceId !== expected.spaceId))
    || !integerIn(v.revision, 0, Number.MAX_SAFE_INTEGER)) return null
  const policy = parseGovernancePolicy(v.policy)
  return policy && { version: 1, hubId: v.hubId, spaceId: v.spaceId, revision: v.revision, policy }
}
