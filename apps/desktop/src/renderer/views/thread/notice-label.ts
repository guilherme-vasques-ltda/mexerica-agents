// D169 R5 (MessageBubble): the routine run notice. The Hub still writes the app's own prefix in English
// (`Routine · <name>`, host/src/routines/runner.rs); the app shows it in its language ("Rotina · <name>" em pt-BR).
// Only that prefix changes: the routine's name is the user's and stays exactly as it came.
import type { Tradutor } from '../../lib/fork-i18n/estado'

// textos-ok: prefixo fixo que o Hub grava no aviso (protocolo), só comparado; o texto mostrado vem do catálogo.
const HUB_PREFIX = 'Routine · '

export function routineNoticeText(t: Tradutor, text: string | undefined): string {
  if (!text) return t('conversa.avisos.rotina')
  return text.startsWith(HUB_PREFIX) ? t('conversa.avisos.rotinaNome', { nome: text.slice(HUB_PREFIX.length) }) : text
}
