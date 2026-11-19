// Phone/host compatibility: each side names the oldest version of the other it still works
// with (docs/reference/compatibility.md). Mirrors host/src/compat.rs.

export type VersionMismatch =
  | { kind: 'updateApp'; minimum: string }
  | { kind: 'updateHost'; version: string; minimum: string }

/** The oldest host this app works with. */
export const MIN_HOST = '2.20.1'

/** `major.minor.patch`, ignoring a leading `v` and any pre-release or build suffix. */
export function parseVersion(version: string): number[] | null {
  let core = version.trim()
  if (core.startsWith('v')) core = core.slice(1)
  core = core.split('-')[0]!.split('+')[0]!
  const parts = core.split('.')
  if (parts.length < 1 || parts.length > 3) return null
  const out: number[] = []
  for (const part of parts) {
    if (!/^[0-9]+$/.test(part)) return null
    out.push(Number(part))
  }
  while (out.length < 3) out.push(0)
  return out
}

/**
 * Release order across lines: major 0 is the Mexerica line, which comes after every inherited
 * Codync version (2.x), so `0.1.0` is newer than `2.20.3`. Within a line the usual numeric
 * order applies. Leaving major 0 needs this rule revisited (mirrors `rank` in host/src/compat.rs).
 */
function rank(v: number[]): number[] {
  return [v[0] === 0 ? 1 : 0, ...v]
}

/** Whether `version` is below `minimum` in release order; an unreadable version never is. */
export function isBelow(version: string, minimum: string): boolean {
  const pv = parseVersion(version)
  const pm = parseVersion(minimum)
  if (!pv || !pm) return false
  const v = rank(pv)
  const m = rank(pm)
  for (let i = 0; i < v.length; i++) {
    if (v[i]! !== m[i]!) return v[i]! < m[i]!
  }
  return false
}

export function checkVersions(app: string, hostVersion: string, minApp: string | null | undefined): VersionMismatch | null {
  if (minApp && isBelow(app, minApp)) return { kind: 'updateApp', minimum: minApp }
  if (isBelow(hostVersion, MIN_HOST)) return { kind: 'updateHost', version: hostVersion, minimum: MIN_HOST }
  return null
}
