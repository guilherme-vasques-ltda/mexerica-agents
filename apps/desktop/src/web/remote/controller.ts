import { createWorkspaceSession, hubOffline, isWorkspaceAccessLost, type WorkspaceContext, type WorkspaceLease } from './workspace-session.ts'
import { filterAuthorizedWorkspaces, type ClerkOrganization, type HubWorkspaceGrant, type Workspace } from './workspaces.ts'
import type { RemoteViewState } from './view.ts'

export interface RemoteHub {
  confirm(workspace: Workspace, callbacks: {
    signal: AbortSignal
    offline(): void
    accessLost(status: number, code: unknown): void
    acceptContext: WorkspaceLease['acceptContext']
    acceptEvent: WorkspaceLease['acceptEvent']
    acceptsResponse: WorkspaceLease['acceptsResponse']
    onDispose(cleanup: () => void): void
  }): Promise<WorkspaceContext>
}

export function createRemoteController(
  render: (state: RemoteViewState) => void,
  hub?: RemoteHub,
  session = createWorkspaceSession(),
) {
  let userId: string | null = null
  let workspaces: Workspace[] = []
  let selected: string | null = null
  let attempt = 0
  let status: RemoteViewState['status'] = 'loading'

  const show = (next: RemoteViewState['status']) => {
    status = next
    render({ status, workspaces, activeId: selected, signedIn: userId !== null })
  }

  const select = async (id: string, recovering = false) => {
    if (!recovering && selected === id && (status === 'active' || status === 'switching')) return
    const workspace = workspaces.find((candidate) => candidate.id === id)
    if (!userId || !workspace || !hub) return
    const run = ++attempt
    selected = id
    try {
      const opening = session.open(workspace, userId, () => {
        if (run !== attempt) return
        workspaces = workspaces.filter((entry) => entry.id !== id)
        selected = null
        show('revoked')
      })
      show('switching')
      const lease = await opening
      if (!lease || run !== attempt) return
      const offline = () => {
        if (lease.signal.aborted || run !== attempt) return
        const disconnect = ++attempt
        void session.close().catch(() => {
          if (disconnect !== attempt) return
          selected = null
          show('unavailable')
        })
        show(hubOffline.code)
      }
      const context = await hub.confirm(workspace, {
        signal: lease.signal,
        offline,
        accessLost: (status, code) => {
          if (lease.signal.aborted || run !== attempt) return
          if (status === 409 && code === 'workspace_epoch_mismatch') {
            if (!lease.acceptsResponse()) return
            if (!recovering) {
              void select(id, true)
              return
            }
            ++attempt
            void session.close().catch(() => {})
            selected = null
            show('unavailable')
          } else if (isWorkspaceAccessLost(status, code)) void lease.revoke().catch(() => {})
        },
        acceptContext: lease.acceptContext,
        acceptEvent: lease.acceptEvent,
        acceptsResponse: lease.acceptsResponse,
        onDispose: lease.onDispose,
      })
      if (run !== attempt || lease.signal.aborted) return
      if (lease.acceptsResponse() || lease.acceptContext(context)) show('active')
      else await lease.revoke()
    } catch {
      if (run === attempt && status !== 'revoked') {
        void session.close().catch(() => {})
        selected = null
        show('unavailable')
      }
    }
  }

  return {
    async setIdentity(id: string | null, organizations: readonly ClerkOrganization[], grants: readonly HubWorkspaceGrant[]) {
      const run = ++attempt
      userId = id
      workspaces = []
      selected = null
      try {
        const closing = session.close()
        show('loading')
        await closing
        if (run !== attempt) return
        if (!id) return show('signed_out')
        if (!hub) return show('unavailable')
        workspaces = filterAuthorizedWorkspaces(grants, organizations)
        if (workspaces.length) await select(workspaces[0].id)
        else show('empty')
      } catch {
        if (run === attempt) show('unavailable')
      }
    },
    select: (id: string) => select(id),
    retry() {
      if (selected && status === 'hub_offline') return select(selected)
    },
  }
}
