import { createRemoteController } from './controller.ts'
import { mountRemoteView } from './view.ts'
import './view.css'

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

    signIn = () => { void clerk.redirectToSignIn({ signInFallbackRedirectUrl: location.origin }).catch(() => {
      render({ status: 'unavailable', workspaces: [], activeId: null })
    }) }
    signOut = () => {
      signingOut = true
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
        void controller.setIdentity(null, [], [])
        return
      }
      const organizations = user.organizationMemberships.map(({ organization: { id, name } }) => ({ id, name }))
      void controller.setIdentity(user.id, organizations, [])
    })
  } catch {
    render({ status: 'unavailable', workspaces: [], activeId: null })
  }
}
