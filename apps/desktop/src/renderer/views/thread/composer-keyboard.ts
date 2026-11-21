// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
/**
 * Quando o Enter envia (DON-907, Ajustes → Conversa → Atalho de envio):
 * - `enter`: Enter envia; Shift/Alt+Enter quebram a linha (o padrão de sempre);
 * - `mod-multiline`: Enter envia enquanto o rascunho tem uma linha só; com uma quebra de linha, Enter quebra;
 * - `mod`: Enter sempre quebra a linha.
 * Em qualquer modo ⌘+Enter (Ctrl+Enter fora do Mac) envia e Shift+Enter quebra.
 */
export const SEND_MODES = ['enter', 'mod-multiline', 'mod'] as const
export type SendMode = (typeof SEND_MODES)[number]
export const isSendMode = (value: unknown): value is SendMode => SEND_MODES.includes(value as SendMode)

export function composerEnter(event: {
  key: string; shiftKey: boolean; altKey: boolean; metaKey?: boolean; ctrlKey?: boolean; repeat: boolean
  isComposing: boolean; keyCode: number
}, mode: SendMode = 'enter', multiline = false): 'send' | 'newline' | 'ignore' {
  if (event.key !== 'Enter' || event.isComposing || event.keyCode === 229) return 'ignore'
  if (event.shiftKey || event.altKey) return 'newline'
  const modifier = event.metaKey || event.ctrlKey
  if (!modifier && (mode === 'mod' || (mode === 'mod-multiline' && multiline))) return 'newline'
  return event.repeat ? 'ignore' : 'send'
}

/** O `aria-keyshortcuts` do campo no modo escolhido. */
export function sendShortcuts(mode: SendMode, mac: boolean): string {
  const mod = mac ? 'Meta+Enter' : 'Control+Enter'
  return mode === 'enter' ? `Enter ${mod} Shift+Enter` : `${mod} Enter Shift+Enter`
}
