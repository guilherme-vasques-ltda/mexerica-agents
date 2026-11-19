import { recoveryDiagnostic as diag, type RecoveryStage } from './recovery-diagnostic.ts'

type ObjectValue = Record<string, unknown>
const object = (value: unknown): value is ObjectValue => value !== null && typeof value === 'object' && !Array.isArray(value)
const has = (value: ObjectValue, key: string) => Object.prototype.hasOwnProperty.call(value, key)

/** Classifies the already consumed resource. No returned field contains account data. */
export function classifyAttempt(value: unknown, stage: RecoveryStage, present = true): void {
  try {
    if (!present) return diag(stage, 'attempt_absent')
    if (value === null) return diag(stage, 'attempt_null')
    if (!object(value) || typeof value.status !== 'string') return diag(stage, 'attempt_invalid')
    const status = value.status
    diag(stage, status === 'complete' ? (typeof value.created_session_id === 'string' && value.created_session_id ? 'attempt_complete_target' : 'attempt_complete') : ['abandoned', 'expired', 'canceled', 'cancelled', 'failed'].includes(status) ? 'attempt_terminal' : ['needs_identifier', 'needs_first_factor', 'needs_second_factor', 'needs_new_password', 'missing_requirements', 'pending'].includes(status) ? 'attempt_pending' : 'attempt_invalid')
  } catch { /* Classification is observational. */ }
}

/** Examines wrappers separately from the client the existing caller actually selected. */
export function classifyClient(payload: unknown, selected: unknown): void {
  try {
    if (!object(payload)) { diag('classify_wrapper', 'wrapper_invalid'); return }
    if (!has(payload, 'response')) diag('classify_wrapper', 'response_absent')
    else if (payload.response === null) diag('classify_wrapper', 'response_null')
    else diag('classify_wrapper', 'response_wrapper')
    const response = object(payload.response) ? payload.response : null
    const meta = object(payload.meta) ? payload.meta : null
    const root = has(payload, 'sessions') || has(payload, 'last_active_session_id')
    const client = has(payload, 'client') && payload.client !== null
    const metaClient = !!meta && has(meta, 'client') && meta.client !== null
    if (root) diag('classify_wrapper', 'root_client')
    if (client) diag('classify_wrapper', 'client_sidecar')
    if (metaClient) diag('classify_wrapper', 'meta_client_sidecar')
    if (Number(!!response) + Number(root) + Number(client) + Number(metaClient) > 1) diag('classify_wrapper', 'multiple_wrappers')
    if (!object(selected)) { diag('classify_sessions', 'sessions_invalid'); return }
    classifyAttempt(selected.sign_in, 'attempt_signin', has(selected, 'sign_in'))
    classifyAttempt(selected.sign_up, 'attempt_signup', has(selected, 'sign_up'))
    if (!has(selected, 'sessions')) { diag('classify_sessions', 'sessions_absent'); return }
    if (!Array.isArray(selected.sessions)) { diag('classify_sessions', 'sessions_invalid'); return }
    diag('classify_sessions', selected.sessions.length ? 'sessions_present' : 'sessions_empty')
    if (!has(selected, 'last_active_session_id')) { diag('classify_pointer', 'pointer_absent'); return }
    const pointer = selected.last_active_session_id
    if (pointer === null) { diag('classify_pointer', 'pointer_null'); return }
    if (typeof pointer !== 'string' || !pointer) { diag('classify_pointer', 'pointer_invalid'); return }
    const matches = selected.sessions.filter((item: unknown) => object(item) && item.id === pointer)
    if (!matches.length) { diag('classify_pointer', 'pointer_unmatched'); return }
    if (matches.length !== 1) { diag('classify_pointer', 'pointer_duplicate'); return }
    diag('classify_pointer', 'pointer_matched')
    const session = matches[0] as ObjectValue
    diag('classify_selected', session.status === 'active' ? 'selected_active' : session.status === 'pending' ? 'selected_pending' : (typeof session.status === 'string' && ['ended', 'removed', 'revoked', 'expired', 'abandoned'].includes(session.status)) ? 'selected_terminal' : 'selected_other')
    if (!object(session.user) || typeof session.user.id !== 'string' || !session.user.id) diag('classify_selected', 'selected_user_missing')
  } catch { /* Malformed or hostile values cannot affect authentication. */ }
}
