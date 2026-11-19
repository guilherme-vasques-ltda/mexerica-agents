/** Fixed, local-only diagnostic vocabulary; never pass account data to this emitter. */
export const RECOVERY_STAGES = [
  'background_exit', 'check_entry', 'check_branch', 'check_projection', 'restore', 'secure_availability', 'account_load', 'account_decrypt',
  'account_envelope', 'restore_client_get', 'restore_session', 'restore_write', 'password_load',
  'restore_finalize', 'secure_encrypt', 'social_attempt_get', 'social_client_get', 'password_attempt_get',
  'recovery_anchor', 'recovery_quarantine', 'classify_wrapper', 'classify_sessions', 'classify_pointer', 'classify_selected', 'attempt_social', 'attempt_signin', 'attempt_signup',
  'password_client_get', 'recovery_write', 'confirm_empty', 'resume_guard', 'callback_route', 'callback_validate', 'browser_wait',
] as const
export const RECOVERY_REASONS = [
  'begin', 'end', 'entered', 'refused_busy', 'refused_not_ready', 'social_tracked', 'social_legacy',
  'social_empty_exit', 'password_readable', 'password_unreadable', 'initial_empty', 'resume_guard', 'restore',
  'pending', 'unavailable', 'no_base', 'already_running', 'available', 'not_available', 'failed', 'empty',
  'active', 'uncertain', 'recoverable', 'exit_retained', 'succeeded', 'invalid', 'social_marker', 'stored_session',
  'response', 'http_refused', 'transport_failed', 'credential_format', 'credential_cleared', 'credential_changed',
  'client_not_empty', 'shape_invalid', 'attempt_mismatch', 'attempt_incomplete', 'missing_target', 'session_missing',
  'session_not_active', 'last_active_mismatch', 'principal_mismatch', 'binding_mismatch', 'session_confirmed',
  'password_override', 'epoch_superseded', 'no_waiter', 'scheme_mismatch', 'callback_mismatch', 'delivered',
  'none', 'unknown', 'session_unverified', 'target_unavailable', 'needs_authentication', 'checking', 'revoked',
  'binding_invalid', 'principal_invalid', 'anchor_conflict', 'binding_valid', 'principal_only', 'unbound', 'original_unbound', 'invalid_target', 'pending_written', 'write_failed',
  'response_absent', 'response_null', 'response_wrapper', 'root_client', 'client_sidecar', 'meta_client_sidecar', 'multiple_wrappers', 'wrapper_invalid',
  'sessions_absent', 'sessions_invalid', 'sessions_empty', 'sessions_present', 'pointer_absent', 'pointer_null', 'pointer_invalid', 'pointer_unmatched', 'pointer_duplicate', 'pointer_matched',
  'selected_active', 'selected_pending', 'selected_terminal', 'selected_other', 'selected_user_missing',
  'attempt_absent', 'attempt_null', 'attempt_invalid', 'attempt_pending', 'attempt_complete', 'attempt_complete_target', 'attempt_terminal',
  'invalid_url', 'cancelled', 'timeout', 'open_failed', 'opened', 'started',
] as const
export type RecoveryStage = typeof RECOVERY_STAGES[number]
export type RecoveryReason = typeof RECOVERY_REASONS[number]

/** Invalid names are discarded; status is an HTTP integer or null. Logging never changes control flow. */
export function recoveryDiagnostic(stage: unknown, reason: unknown, status: unknown = null): void {
  try {
    if (typeof stage !== 'string' || !(RECOVERY_STAGES as readonly string[]).includes(stage)) return
    if (typeof reason !== 'string' || !(RECOVERY_REASONS as readonly string[]).includes(reason)) return
    const safeStatus = typeof status === 'number' && Number.isInteger(status) && status >= 100 && status <= 599 ? status : null
    console.warn('account:recovery-diagnostic', JSON.stringify({ stage, reason, status: safeStatus }))
  } catch { /* Diagnostics must never change authentication or recovery. */ }
}

/** Read only the numeric status from an existing response; no payload or error reaches the emitter. */
export function recoveryStatus(response: { status?: unknown }): unknown {
  try { return response.status } catch { return null }
}
