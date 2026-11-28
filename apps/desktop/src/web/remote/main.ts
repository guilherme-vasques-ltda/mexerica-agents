import { createRemoteController } from './controller.ts'
import { createRemoteHub } from './fork-transport.ts'
import type { ChannelTarget } from './socket.ts'
import { mountRemoteView } from './view.ts'

declare const __REMOTE_CLERK_TEST_KEY__: string
declare const __REMOTE_HUBS__: (ChannelTarget & { workspaceId: string })[]

let signIn = () => {}
let signOut = () => {}
let userId: string | null = null
let getSessionToken: (org: string | undefined) => Promise<string | null> = async () => null
const hubs = __REMOTE_HUBS__
const hub = createRemoteHub((workspace) => {
  const configured = hubs.find(({ workspaceId }) => workspaceId === workspace.id)
  return configured && userId ? {
    ...configured,
    userId,
    getSessionToken: () => getSessionToken(workspace.kind === 'team' ? workspace.organizationId : undefined),
  } : null
})
let frame: HTMLIFrameElement | undefined
const access = mountRemoteView(document.getElementById('root')!, {
  signIn: () => signIn(),
  signOut: () => signOut(),
  select: (id) => { void controller.select(id) },
  retry: () => { void controller.retry() },
  pair: (code) => { void controller.pair(code) },
})
const render: typeof access = (state) => {
  if (state.status !== 'active') {
    frame?.remove()
    frame = undefined
  }
  access(state)
  if (state.status === 'active' && !frame) {
    frame = document.createElement('iframe')
    frame.className = 'rv-renderer'
    frame.title = document.title
    frame.src = './renderer.html'
    document.body.append(frame)
  }
}
Object.assign(window, {
  remoteRenderer(caller: Window) {
    return frame?.contentWindow === caller ? hub.rendererContext() : undefined
  },
})
const controller = createRemoteController(render, hub)

if (!__REMOTE_CLERK_TEST_KEY__) {
  render({ status: 'unconfigured', workspaces: [], activeId: null })
} else {
  render({ status: 'loading', workspaces: [], activeId: null })
  try {
    const { Clerk } = await import('@clerk/clerk-js/no-rhc')
    const clerk = new Clerk(__REMOTE_CLERK_TEST_KEY__)
    await clerk.load({ telemetry: { disabled: true } })
    let signingOut = false
    let lastIdentity: string | null | undefined
    let activeSessionId: string | null = null
    getSessionToken = async (organizationId) => {
      if (!activeSessionId || !userId || signingOut) return null
      const expected = activeSessionId
      if (!organizationId && clerk.organization) await clerk.setActive({ organization: null })
      const session = clerk.session
      if (session?.id !== expected || clerk.user?.id !== userId) return null
      return session.getToken({ organizationId, skipCache: true })
    }

    signIn = () => { void clerk.redirectToSignIn({ signInFallbackRedirectUrl: location.origin }).catch(() => {
      render({ status: 'unavailable', workspaces: [], activeId: null })
    }) }
    signOut = () => {
      signingOut = true
      lastIdentity = undefined
      userId = activeSessionId = null
      void controller.setIdentity(null, [], []).then(() => clerk.signOut()).then(() => {
        signingOut = false
      }).catch(() => {
        signingOut = false
        render({ status: 'unavailable', workspaces: [], activeId: null, signedIn: true })
      })
    }
    clerk.addListener(({ user, session }) => {
      if (signingOut) return
      if (!user || !session) {
        if (lastIdentity === null) return
        lastIdentity = null
        userId = activeSessionId = null
        void controller.setIdentity(null, [], [])
        return
      }
      const organizations = user.organizationMemberships.map(({ organization: { id, name } }) => ({ id, name }))
      const identity = JSON.stringify([user.id, session.id, organizations.map(({ id, name }) => JSON.stringify([id, name])).sort()])
      if (identity === lastIdentity) return
      lastIdentity = identity
      userId = user.id
      activeSessionId = session.id
      const grants = hubs.flatMap(({ workspaceId }): import('./workspaces.ts').HubWorkspaceGrant[] => {
        if (workspaceId === `personal:${user.id}`) return [{ id: workspaceId, kind: 'personal' }]
        if (workspaceId.startsWith('org:')) return [{ id: workspaceId, kind: 'team', organizationId: workspaceId.slice(4) }]
        return []
      })
      void controller.setIdentity(user.id, organizations, grants)
    })
  } catch {
    render({ status: 'unavailable', workspaces: [], activeId: null })
  }
}
