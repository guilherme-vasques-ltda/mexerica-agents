// Recusas da Open chegam antes de gravar a mensagem: dá para dizer o motivo sem dúvida sobre o envio.
export type ErroDoEnvio = 'spaces.compose.falhou' | 'spaces.compose.semContaOpen' | 'spaces.compose.ocupadoOpen'

export function erroDoEnvio(e: unknown): ErroDoEnvio {
  const message = typeof e === 'object' && e !== null && 'message' in e && typeof e.message === 'string' ? e.message : ''
  if (/^open_chat_account_required\b/.test(message)) return 'spaces.compose.semContaOpen'
  if (/^open_chat_busy\b/.test(message)) return 'spaces.compose.ocupadoOpen'
  return 'spaces.compose.falhou'
}

/** O motivo gravado na bolha local que não foi enviada; qualquer outro valor (dado salvo antigo ou torto) vira `null`. */
export function recusaOpen(valor: unknown): Exclude<ErroDoEnvio, 'spaces.compose.falhou'> | null {
  return valor === 'spaces.compose.semContaOpen' || valor === 'spaces.compose.ocupadoOpen' ? valor : null
}
