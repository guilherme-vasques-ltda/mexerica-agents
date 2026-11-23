import { clearWorkspaceStorage } from './storage.ts'
import type { Workspace } from './workspaces.ts'

export interface WorkspaceContext {
  schema: 1
  hubId: string
  kind: 'personal' | 'team'
  workspaceId: string
  userId: string
  role: 'owner' | 'admin' | 'member'
  workspaceEpoch: number
  capabilities: readonly string[]
}

interface WorkspaceEvent {
  type: string
  workspaceId: string
  workspaceEpoch: number
}

export const hubOffline = { code: 'hub_offline', actionKind: 'start_hub' } as const

export function isWorkspaceAccessLost(status: number, code: unknown): boolean {
  return (status === 401 && (code === 'auth_invalid' || code === 'device_unpaired' || code === 'device_revoked'))
    || (status === 403 && code === 'workspace_forbidden')
    || (status === 409 && code === 'workspace_epoch_mismatch')
}

export interface WorkspaceLease {
  readonly signal: AbortSignal
  acceptContext(context: WorkspaceContext): boolean
  acceptEvent(event: WorkspaceEvent): boolean
  acceptsResponse(): boolean
  onDispose(cleanup: () => void): void
  revoke(): Promise<void>
}

export function createWorkspaceSession(
  clearStorage: (workspaceId: string) => Promise<void> = clearWorkspaceStorage,
) {
  let current: { workspaceId: string; dispose: () => void } | undefined
  let generation = 0
  const dirty = new Set<string>()
  let cleanup = Promise.resolve()

  const close = async () => {
    generation++
    const previous = current
    current = undefined
    previous?.dispose()
    const storage = cleanup.catch(() => {}).then(async () => {
      const results = await Promise.allSettled(Array.from(dirty, async (workspaceId) => {
        await clearStorage(workspaceId)
        dirty.delete(workspaceId)
      }))
      const failed = results.find((result) => result.status === 'rejected')
      if (failed?.status === 'rejected') throw failed.reason
    })
    cleanup = storage
    await storage
  }

  const open = async (workspace: Workspace, userId: string, onRevoked: () => void): Promise<WorkspaceLease | null> => {
    const closing = close()
    const ownGeneration = generation
    await closing
    if (ownGeneration !== generation) return null
    const controller = new AbortController()
    const disposers = new Set<() => void>()
    let confirmed: WorkspaceContext | undefined
    const isCurrent = () => generation === ownGeneration
    const acceptsResponse = () => isCurrent() && confirmed !== undefined
    current = {
      workspaceId: workspace.id,
      dispose: () => {
        confirmed = undefined
        controller.abort()
        for (const dispose of disposers) {
          try { dispose() } catch { /* Keep disposing the rest. */ }
        }
        disposers.clear()
      },
    }
    dirty.add(current.workspaceId)

    const revoke = async () => {
      if (!isCurrent()) return
      const closing = close()
      try { onRevoked() } finally { await closing }
    }
    return {
      signal: controller.signal,
      acceptContext(context) {
        if (!isCurrent() || confirmed) return false
        const expectedId = workspace.kind === 'personal' ? `personal:${userId}` : `org:${workspace.organizationId}`
        if (context.schema !== 1 || !context.hubId?.trim() || !userId || context.userId !== userId
          || context.kind !== workspace.kind || context.workspaceId !== workspace.id || context.workspaceId !== expectedId
          || (workspace.kind === 'personal' ? context.role !== 'owner' : !['owner', 'admin', 'member'].includes(context.role))
          || !Number.isSafeInteger(context.workspaceEpoch) || context.workspaceEpoch < 0) return false
        confirmed = { ...context }
        return true
      },
      acceptEvent(event) {
        if (!isCurrent() || !confirmed || event.workspaceId !== confirmed.workspaceId) return false
        if (event.type === 'workspace_revoked' || event.type === 'role_changed' || event.type === 'device_revoked') {
          if (Number.isSafeInteger(event.workspaceEpoch) && (event.workspaceEpoch > confirmed.workspaceEpoch
            || (event.type === 'device_revoked' && event.workspaceEpoch === confirmed.workspaceEpoch))) {
            void revoke().catch(() => {})
          }
          return false
        }
        return event.workspaceEpoch === confirmed.workspaceEpoch
      },
      acceptsResponse,
      onDispose(dispose) {
        if (isCurrent()) disposers.add(dispose)
        else dispose()
      },
      revoke,
    }
  }
  return { open, close }
}
