// Recusas da Open chegam antes de gravar a mensagem: dá para dizer o motivo sem dúvida sobre o envio.
export type ErroDoEnvio = 'spaces.compose.falhou' | 'spaces.compose.semContaOpen' | 'spaces.compose.ocupadoOpen' | 'spaces.compose.reconectandoOpen'

export function erroDoEnvio(e: unknown): ErroDoEnvio {
  const message = typeof e === 'object' && e !== null && 'message' in e && typeof e.message === 'string' ? e.message : ''
  if (/^open_chat_account_required\b/.test(message)) return 'spaces.compose.semContaOpen'
  if (/^open_chat_busy\b/.test(message)) return 'spaces.compose.ocupadoOpen'
  if (/^open_chat_reconnecting\b/.test(message)) return 'spaces.compose.reconectandoOpen'
  return 'spaces.compose.falhou'
}

const AVISOS_OPEN = {
  antigravity_default_mode_unconfirmed: 'conversa.avisos.antigravity.modoNaoConfirmado',
  open_setup: 'conversa.avisos.open.inicio',
  open_chat_cancelled: 'conversa.avisos.open.interrompida',
  open_chat_tool_refused: 'conversa.avisos.open.ferramenta',
  open_chat_timeout: 'conversa.avisos.open.demorou',
  open_chat_too_long: 'conversa.avisos.open.longa',
  open_chat_policy: 'conversa.avisos.open.politica',
  open_codex_unavailable: 'conversa.avisos.open.indisponivel',
  open_codex_platform: 'conversa.avisos.open.indisponivel',
  open_chat_failed: 'conversa.avisos.open.falhou',
} as const

/** O aviso do Hub da Open no idioma da janela, pelo código; código desconhecido fica com o texto gravado. */
export function avisoOpen(code: unknown): (typeof AVISOS_OPEN)[keyof typeof AVISOS_OPEN] | null {
  return typeof code === 'string' && Object.hasOwn(AVISOS_OPEN, code) ? AVISOS_OPEN[code as keyof typeof AVISOS_OPEN] : null
}

/** O motivo gravado na bolha local que não foi enviada; qualquer outro valor (dado salvo antigo ou torto) vira `null`. */
export function recusaOpen(valor: unknown): Exclude<ErroDoEnvio, 'spaces.compose.falhou'> | null {
  return valor === 'spaces.compose.semContaOpen' || valor === 'spaces.compose.ocupadoOpen' || valor === 'spaces.compose.reconectandoOpen'
    ? valor
    : null
}

/** A recusa `open_chat_busy` do Hub: resposta da Open em andamento (apagar bot também é recusado com ela). */
export function respostaEmAndamento(e: unknown): boolean {
  const message = typeof e === 'object' && e !== null && 'message' in e && typeof e.message === 'string' ? e.message : ''
  return /^open_chat_busy\b/.test(message)
}
