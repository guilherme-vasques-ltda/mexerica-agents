/** A response proves termination only for the captured session, never by absence or HTTP code. */
export function terminalSession(payload: unknown, sessionId: string): boolean {
  if (!payload || typeof payload !== 'object') return false
  const wrapper = payload as { response?: unknown; client?: unknown; id?: unknown; status?: unknown; sessions?: unknown }
  const resource = wrapper.response && typeof wrapper.response === 'object' ? wrapper.response : payload
  const candidates: unknown[] = [resource]
  for (const item of [resource, wrapper.client]) {
    if (item && typeof item === 'object') {
      const sessions = (item as { sessions?: unknown }).sessions
      if (Array.isArray(sessions)) candidates.push(...sessions)
    }
  }
  const matching = candidates.filter((item) => !!item && typeof item === 'object' && (item as { id?: unknown }).id === sessionId)
  const terminal = new Set(['ended', 'expired', 'removed', 'replaced', 'revoked', 'abandoned'])
  return matching.length === 1 && terminal.has(String((matching[0] as { status?: unknown }).status))
}

/** Exactly one row for a cryptographically verified existing key, never name or first entry. */
export function findDeviceForKey(payload: unknown, key: string): { id: string; revoked: boolean } | null {
  if (!payload || typeof payload !== 'object') return null
  const list = (payload as { devices?: unknown }).devices
  if (!Array.isArray(list)) return null
  const matches = list.filter((row) => row && typeof row === 'object' && row.deviceKey === key)
  if (matches.length !== 1 || typeof matches[0].deviceId !== 'string' || !/^dev_[A-Za-z0-9_-]+$/.test(matches[0].deviceId) || typeof matches[0].revoked !== 'boolean') return null
  return { id: matches[0].deviceId, revoked: matches[0].revoked }
}

/** Only a well-formed complete list without a matching key permits an explicit registration. */
export function noDeviceForKey(payload: unknown, key: string): boolean {
  if (!payload || typeof payload !== 'object') return false
  const rows = (payload as { devices?: unknown }).devices
  return Array.isArray(rows) && rows.every((row) => row && typeof row === 'object' && typeof row.deviceKey === 'string' && typeof row.deviceId === 'string' && typeof row.revoked === 'boolean') && !rows.some((row) => row.deviceKey === key)
}
