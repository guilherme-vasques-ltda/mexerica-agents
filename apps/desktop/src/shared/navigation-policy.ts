export type NavigationDecision = 'allow' | 'external' | 'block'

const externalProtocols = new Set(['http:', 'https:', 'mailto:'])

export function parseNavigationURL(value: unknown): URL | null {
  if (typeof value !== 'string') return null
  try { return new URL(value) } catch { return null }
}

/** Only the renderer entry point may keep the app's preload; query and fragment select UI state. */
export function decideNavigation(value: unknown, appURL?: string): NavigationDecision {
  const target = parseNavigationURL(value)
  if (!target) return 'block'
  const entry = parseNavigationURL(appURL)
  if (entry && ['file:', 'http:', 'https:'].includes(entry.protocol)
    && !target.username && !target.password && !entry.username && !entry.password
    && target.protocol === entry.protocol && target.host === entry.host && target.pathname === entry.pathname) {
    return 'allow'
  }
  return externalProtocols.has(target.protocol) ? 'external' : 'block'
}

/** Never log a path, recipient, credentials, query or fragment, even for malformed input. */
export function navigationLogProtocol(value: unknown): string {
  const protocol = parseNavigationURL(value)?.protocol
  return protocol && protocol.length <= 64 ? protocol : 'invalid'
}
