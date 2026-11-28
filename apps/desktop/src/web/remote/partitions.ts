import type { WorkspaceContext } from './workspace-session.ts'

const freshnessMs = 24 * 60 * 60 * 1000
export const partitionId = (hubId: string, workspaceId: string, userId: string) => JSON.stringify([hubId, workspaceId, userId])

export function createPartitions(now = Date.now) {
  const partitions = new Map<string, { hubId: string; epoch: number; freshUntil: number; data: Map<string, string> }>()
  return {
    authenticate(context: WorkspaceContext) {
      const id = partitionId(context.hubId, context.workspaceId, context.userId)
      let partition = partitions.get(id)
      if (partition && context.workspaceEpoch < partition.epoch) throw new Error('epoch_rollback')
      if (!partition || context.workspaceEpoch > partition.epoch) {
        partition?.data.clear()
        partition = { hubId: context.hubId, epoch: context.workspaceEpoch, freshUntil: 0, data: new Map() }
        partitions.set(id, partition)
      }
      partition.freshUntil = now() + freshnessMs
      const active = partition
      return {
        get length() { return now() < active.freshUntil ? active.data.size : 0 },
        key: (index: number) => now() < active.freshUntil ? Array.from(active.data.keys())[index] ?? null : null,
        getItem: (key: string) => now() < active.freshUntil ? active.data.get(key) ?? null : null,
        setItem(key: string, value: string) { if (now() < active.freshUntil) active.data.set(key, String(value)) },
        removeItem(key: string) { if (now() < active.freshUntil) active.data.delete(key) },
        clear() { if (now() < active.freshUntil) active.data.clear() },
      } satisfies Storage
    },
    cut(context: WorkspaceContext, nextEpoch: number) {
      const id = partitionId(context.hubId, context.workspaceId, context.userId)
      const partition = partitions.get(id)
      if (partition?.epoch !== context.workspaceEpoch) return
      partition.freshUntil = 0
      partition.data.clear()
      partitions.set(id, { ...partition, epoch: nextEpoch, data: new Map() })
    },
    revokeHub(hubId: string) {
      for (const partition of partitions.values()) {
        if (partition.hubId !== hubId) continue
        partition.freshUntil = 0
        partition.data.clear()
      }
    },
  }
}
