import type { CallError } from './ipc'

/** Decodifica o envelope aditivo sem exigir que Hubs antigos enviem código. */
export function hostCallError(status: number, text: string): CallError {
  let message = `Host error ${status}`
  let code: string | undefined
  try {
    const body = JSON.parse(text) as { error?: unknown; code?: unknown }
    if (typeof body.error === 'string') message = body.error
    if (typeof body.code === 'string') code = body.code
  } catch { /* resposta antiga ou inválida */ }
  return { status, message, ...(code === undefined ? {} : { code }) }
}
