// Recusas da Open chegam antes de gravar a mensagem: dá para dizer o motivo sem dúvida sobre o envio.
export type ErroDoEnvio = 'spaces.compose.falhou' | 'spaces.compose.semContaOpen' | 'spaces.compose.ocupadoOpen'

export function erroDoEnvio(e: unknown): ErroDoEnvio {
  const message = e instanceof Error ? e.message : ''
  if (/^open_chat_account_required\b/.test(message)) return 'spaces.compose.semContaOpen'
  if (/^open_chat_busy\b/.test(message)) return 'spaces.compose.ocupadoOpen'
  return 'spaces.compose.falhou'
}
