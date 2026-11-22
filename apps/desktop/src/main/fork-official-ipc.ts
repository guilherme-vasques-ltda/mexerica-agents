import type { IpcMain, IpcMainEvent, IpcMainInvokeEvent } from 'electron'

const entryChannels = new Set([
  'app:name', 'app:scheme', 'app:version', 'app:suggestedLanguage', 'app:setLanguage',
  'app:windowControls', 'app:isFullScreen', 'app:openedAtLogin', 'app:debugOpen',
  'host:snapshot',
  'speech:available', 'island:supported',
  'account:state', 'account:signIn', 'account:signInWithPassword', 'account:reconcileSignInWithPassword',
  'account:attemptSecondFactor', 'account:signUpWithPassword', 'account:signInWithEmailCode', 'account:verifyEmailCode',
  'account:forgotPassword', 'account:resetPassword', 'account:resendEmailCode', 'account:cancelPending',
  'account:openWaitlist', 'account:signOut', 'account:reconciliationState', 'account:reconcile', 'account:reconcileSignIn',
])

export function requireOfficialSession(
  ipc: IpcMain,
  authenticated: () => boolean,
  entryWindow: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean,
) {
  const allowed = (channel: string, event: IpcMainEvent | IpcMainInvokeEvent) =>
    channel.startsWith('account:') && entryChannels.has(channel)
      ? entryWindow(event)
      : authenticated() || (entryChannels.has(channel) && entryWindow(event))
  const handle = ipc.handle.bind(ipc)
  ipc.handle = (channel, listener) => handle(channel, (event, ...args) => {
    if (!allowed(channel, event)) throw new Error('account_required')
    return listener(event, ...args)
  })
  const on = ipc.on.bind(ipc)
  ipc.on = (channel, listener) => {
    on(channel, (event, ...args) => {
      if (!allowed(channel, event)) { event.returnValue = null; return }
      listener(event, ...args)
    })
    return ipc
  }
}
