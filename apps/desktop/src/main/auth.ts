// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { app, ipcMain } from 'electron'
import { identity } from './environment'
import { openExternalURL } from './navigation'
import { parseNavigationURL } from '../shared/navigation-policy'
import { recoveryDiagnostic as diag } from './fork-account/recovery-diagnostic'

// Browser sign-ins that come back to this app on a custom scheme (ASWebAuthenticationSession's
// job on the iPhone): connector OAuth (`codync://oauth`) and the account (`com.pokai.Codync://callback`).

export const SCHEMES = [identity.scheme, identity.appId]

interface Pending {
  scheme: string
  callback?: string
  resolve: (url: string) => void
  reject: (error: Error) => void
  timer: NodeJS.Timeout
}

let pending: Pending | null = null

/** The browser never came back: another sign-in took its place, or it ran out of time. */
export class AuthenticationEnded extends Error {
  constructor(readonly reason: 'cancelled' | 'timeout', message: string) {
    super(message)
  }
}

/**
 * `keepDockHidden` (D169 N3, ligado em main/index.ts): no macOS um link destes esquemas chega pelo LaunchServices, que
 * põe o app oculto de volta no Dock antes do evento. Só para a visibilidade do Dock: não recebe o URL e corre depois do
 * handleURL, sem impedi-lo.
 */
export function registerSchemes(keepDockHidden: () => void = () => {}) {
  for (const scheme of SCHEMES) {
    if (process.defaultApp && process.argv[1]) app.setAsDefaultProtocolClient(scheme, process.execPath, [process.argv[1]])
    else app.setAsDefaultProtocolClient(scheme)
  }
  // macOS delivers the URL here; Linux starts a second instance with it in argv.
  app.on('open-url', (event, url) => {
    event.preventDefault()
    handleURL(url)
    try { keepDockHidden() } catch {} // A reação do Dock nunca desfaz nem impede o retorno do login.
  })
}

/** A URL on one of our schemes reached the app (open-url, or a second instance's argv). */
export function handleURL(url: string) {
  diag('callback_route', 'entered')
  const scheme = url.split(':')[0]?.toLowerCase()
  let matchesCallback = true
  if (pending?.callback) {
    try {
      const actual = new URL(url)
      const expected = new URL(pending.callback)
      matchesCallback = actual.protocol === expected.protocol && actual.host === expected.host && actual.pathname === expected.pathname &&
        [...expected.searchParams].every(([key, value]) => actual.searchParams.getAll(key).length === 1 && actual.searchParams.get(key) === value)
    } catch { matchesCallback = false }
  }
  if (!pending) diag('callback_route', 'no_waiter')
  else if (scheme !== pending.scheme.toLowerCase()) diag('callback_route', 'scheme_mismatch')
  else if (!matchesCallback) diag('callback_route', 'callback_mismatch')
  else diag('callback_route', 'delivered')
  if (pending && scheme === pending.scheme.toLowerCase() && matchesCallback) {
    clearTimeout(pending.timer)
    pending.resolve(url)
    pending = null
  }
}

export function urlFromArgv(argv: string[]) {
  return argv.find((a) => SCHEMES.some((s) => a.toLowerCase().startsWith(`${s.toLowerCase()}:`)))
}

/** Opens `url` in the browser and waits (up to 10 minutes) for the return on `scheme`. */
export function authenticate(url: string, scheme: string, callback?: string): Promise<string> {
  if (parseNavigationURL(url)?.protocol !== 'https:') { diag('browser_wait', 'invalid_url'); return Promise.reject(new Error('Sign-in needs an https address.')) }
  if (pending) {
    diag('browser_wait', 'cancelled')
    clearTimeout(pending.timer)
    pending.reject(new AuthenticationEnded('cancelled', 'Another sign-in started.'))
  }
  return new Promise((resolve, reject) => {
    const request = {
      scheme,
      callback,
      resolve,
      reject,
      timer: setTimeout(() => {
        diag('browser_wait', 'timeout')
        if (pending === request) pending = null
        reject(new AuthenticationEnded('timeout', 'Sign-in timed out.'))
      }, 10 * 60_000),
    }
    pending = request
    diag('browser_wait', 'started')
    void openExternalURL(url).then((opened) => {
      diag('browser_wait', opened ? 'opened' : 'open_failed')
      if (!opened && pending === request) {
        clearTimeout(request.timer)
        pending = null
        reject(new Error('Could not open the sign-in address.'))
      }
    })
  })
}

export function registerAuthIPC() {
  ipcMain.handle('app:authenticate', (_e, url: string, scheme: string) => {
    // Account callbacks belong exclusively to AccountService in the main process.
    if (typeof scheme !== 'string' || scheme.toLowerCase() !== identity.scheme.toLowerCase()) {
      return Promise.reject(new Error('This callback is not available to connector authentication.'))
    }
    if (pending && (pending.callback || pending.scheme.toLowerCase() !== identity.scheme.toLowerCase())) {
      return Promise.reject(new Error('Account sign-in is already in progress.'))
    }
    return authenticate(url, identity.scheme)
  })
}
