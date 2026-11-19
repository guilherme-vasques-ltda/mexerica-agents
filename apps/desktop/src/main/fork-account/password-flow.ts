import type { PasswordAttempt } from './password-attempt'

/** Pure classification only. Missing or ambiguous data is never an authorization to replay. */
export type PasswordResult =
  | { kind: 'complete'; sessionId: string }
  | { kind: 'retry' }
  | { kind: 'challenge' }
  | { kind: 'unknown' }

export function classifyPasswordResponse(response: unknown, attempt: PasswordAttempt): PasswordResult {
  if (!response || typeof response !== 'object') return { kind: 'unknown' }
  const item = response as { id?: unknown; status?: unknown; created_session_id?: unknown }
  if (item.id !== attempt.id) return { kind: 'unknown' }
  if (item.status === 'complete' && typeof item.created_session_id === 'string' && item.created_session_id) return { kind: 'complete', sessionId: item.created_session_id }
  if (item.status === 'needs_first_factor' && !item.created_session_id && attempt.phase === 'rejected') return { kind: 'retry' }
  if (typeof item.status === 'string' && item.status !== 'complete' && item.status !== 'needs_first_factor' && !item.created_session_id) return { kind: 'challenge' }
  return { kind: 'unknown' }
}
