import { createRemoteController } from './controller.ts'
import { mountRemoteView } from './view.ts'

declare const __REMOTE_CLERK_TEST_KEY__: string

let signIn = () => {}
let signOut = () => {}
const render = mountRemoteView(document.getElementById('root')!, {
  signIn: () => signIn(),
  signOut: () => signOut(),
  select: (id) => { void controller.select(id) },
  retry: () => { void controller.retry() },
})
const controller = createRemoteController(render)

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

    signIn = () => { void clerk.redirectToSignIn({ signInFallbackRedirectUrl: location.origin }).catch(() => {
      render({ status: 'unavailable', workspaces: [], activeId: null })
    }) }
    signOut = () => {
      signingOut = true
      lastIdentity = undefined
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
        void controller.setIdentity(null, [], [])
        return
      }
      const organizations = user.organizationMemberships.map(({ organization: { id, name } }) => ({ id, name }))
      const identity = JSON.stringify([user.id, session.id, organizations.map(({ id, name }) => JSON.stringify([id, name])).sort()])
      if (identity === lastIdentity) return
      lastIdentity = identity
      void controller.setIdentity(user.id, organizations, [])
    })
  } catch {
    render({ status: 'unavailable', workspaces: [], activeId: null })
  }
}
