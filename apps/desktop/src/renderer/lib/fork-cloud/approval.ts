import type { AccessRequest, DecideResult } from '../../../shared/models.ts'

interface Target {
  ready(): Promise<{ call(method: string, body: unknown, timeout: number): Promise<unknown> }>
}

// Canonical unpadded base64url for 32 bytes. The Hub verifies the transcript and authority.
const KEY = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/
const ID = /^[A-Za-z0-9_-]{1,64}$/
const unconfirmed = () => new Error('Resultado não confirmado. Confira o estado antes de tentar novamente.')

/** What the person does next: with a `confirmCode` the computer shows it and the phone asks for it (MX-149). */
export interface DecidedAccess { confirmCode?: string }

/** Capture before the first await; never choose another request or retry an uncertain decision. */
export async function decideCapturedAccess(target: Target, request: AccessRequest, approve: boolean): Promise<DecidedAccess> {
  const { requestId, deviceKey, code, transcript, deadline, expiresAt } = request
  if (typeof requestId !== 'string' || !ID.test(requestId)) throw new Error('Pedido de acesso inválido.')
  if (approve && (!KEY.test(deviceKey) || typeof code !== 'string' || !/^[0-9]{6}$/.test(code) ||
    typeof transcript !== 'string' || !KEY.test(transcript) || !Number.isSafeInteger(deadline) ||
    typeof deadline !== 'number' || deadline < Date.now() || expiresAt !== deadline)) {
    throw new Error('Pedido incompleto ou vencido. Confira o código no aparelho e o pedido atual.')
  }
  const body = approve
    ? Object.freeze({ requestId, approve: true, expect: Object.freeze({ deviceKey, code, transcript }) })
    : Object.freeze({ requestId, approve: false })
  const client = await target.ready()
  if (approve && typeof deadline === 'number' && deadline < Date.now()) throw new Error('O pedido de acesso venceu.')
  let result: unknown
  try { result = await client.call('decideAccessRequest', body, 30_000) }
  catch { throw unconfirmed() }
  if (!result || typeof result !== 'object' || Array.isArray(result)) throw unconfirmed()
  const answer = result as Partial<DecideResult> & { deviceKey?: unknown; grantId?: unknown; reason?: unknown; confirmCode?: unknown }
  // Approved counts only with a valid receipt AND a confirmCode that is absent (older Hub: the key is missing) or
  // exactly 6 digits; null, numbers and any other string are "not confirmed" and never reach the screen.
  if (approve && answer.result === 'approved' && answer.deviceKey === deviceKey &&
    typeof answer.grantId === 'string' && /^grt_[A-Za-z0-9_-]{1,60}$/.test(answer.grantId)) {
    if (!Object.prototype.hasOwnProperty.call(result, 'confirmCode')) return {}
    if (typeof answer.confirmCode === 'string' && /^[0-9]{6}$/.test(answer.confirmCode)) return { confirmCode: answer.confirmCode }
  }
  if (!approve && answer.result === 'denied') return {}
  if (answer.result === 'refused' && typeof answer.reason === 'string' &&
    ['transcriptMismatch', 'requestExpired', 'consuming', 'notFound'].includes(answer.reason)) {
    throw new Error(`Pedido recusado (${answer.reason}). Confira o pedido atual.`)
  }
  throw unconfirmed()
}
