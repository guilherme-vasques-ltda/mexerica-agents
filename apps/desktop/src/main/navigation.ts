import { app, shell, type WebContents } from 'electron'
import { spotlightKeyboardSettings } from '../shared/fork-berga-shortcut'
import { computerPermissionSettings } from '../shared/computer-access'
import { decideNavigation, navigationLogProtocol, parseNavigationURL } from '../shared/navigation-policy'

function blocked(value: unknown) {
  console.warn('[navigation] URL blocked', { protocol: navigationLogProtocol(value) })
}

function openValidatedURL(url: URL) {
  // The OS error may contain the whole URL, so only retain the protocol on failure.
  return shell.openExternal(url.href).then(() => true, () => {
    console.warn('[navigation] External open failed', { protocol: navigationLogProtocol(url.href) })
    return false
  })
}

export function openExternalURL(value: unknown): Promise<boolean> {
  const url = parseNavigationURL(value)
  if (!url || decideNavigation(value) !== 'external') {
    blocked(value)
    return Promise.resolve(false)
  }
  return openValidatedURL(url)
}

// Only fixed permission/keyboard panels, via app:openSettings.
const systemSettingsURLs = new Set<string>([...Object.values(computerPermissionSettings), ...(process.platform === 'darwin' ? [spotlightKeyboardSettings] : [])])

export function openSystemSettingsURL(value: unknown): Promise<boolean> {
  const url = parseNavigationURL(value)
  if (!url || !systemSettingsURLs.has(url.href)) {
    blocked(value)
    return Promise.resolve(false)
  }
  return openValidatedURL(url)
}

/** Register before creating any windows, including the island, pairing and renderer children. */
export function registerWindowNavigation(appURL: string, externalAllowed = () => true) {
  const protectedContents = new WeakSet<WebContents>()
  function protect(contents: WebContents) {
    if (protectedContents.has(contents)) return
    protectedContents.add(contents)
    const navigate = (event: Electron.Event<{ url: string }>) => {
      const decision = decideNavigation(event.url, appURL)
      if (decision === 'allow') return
      event.preventDefault()
      if (externalAllowed()) void openExternalURL(event.url)
    }
    contents.on('will-navigate', navigate)
    contents.on('will-redirect', navigate)
    contents.setWindowOpenHandler(({ url }) => {
      if (externalAllowed()) void openExternalURL(url)
      return { action: 'deny' }
    })
    // Denied popups do not emit this event. Keep a defensive guard if a future handler allows one.
    contents.on('did-create-window', (child, { url }) => {
      protect(child.webContents)
      child.destroy()
      if (externalAllowed()) void openExternalURL(url)
    })
  }
  app.on('web-contents-created', (_event, contents) => {
    if (contents.getType() === 'window') protect(contents)
  })
}
