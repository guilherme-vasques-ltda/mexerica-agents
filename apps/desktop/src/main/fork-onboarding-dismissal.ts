import { createHash } from 'node:crypto'

export function onboardingDismissal(
  prefs: { get(key: string): unknown; set(key: string, value: unknown): boolean },
  installation: readonly (string | number | null)[],
  verifiedUserId: string | undefined,
  expectedUserId: unknown,
  action: unknown,
): boolean | null {
  if (!verifiedUserId || verifiedUserId !== expectedUserId) return null
  const key = `onboardingDismissed.v1.${createHash('sha256').update(JSON.stringify([installation, verifiedUserId])).digest('hex')}`
  if (action === 'read') return prefs.get(key) === true
  if (action !== 'defer' && action !== 'clear') return null
  const deferred = action === 'defer'
  return prefs.set(key, deferred ? true : undefined) ? deferred : null
}
