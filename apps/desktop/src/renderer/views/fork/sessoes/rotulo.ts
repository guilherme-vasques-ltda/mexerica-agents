// UI01b / D372: o que o cabeçalho mostra sobre o modelo da conversa aberta. Puro, sem I/O nem React.
// Só rótulo: o seletor único é da C9 (rodapé do compositor). Mostra a seleção CONFIGURADA da conversa, que vale
// para os próximos turnos; qual modelo respondeu cada mensagem é do recibo dela (#401), não deste rótulo.
import type { SessionEngine } from '../../../../shared/models.ts'
import type { HubSession } from './estado.ts'

export type RotuloModeloView =
  /** Hub sem o anúncio `engine` neste link/época, ou catálogo ainda não lido: nada, e nunca o modelo do bot. */
  | { kind: 'hidden' }
  /** O Hub anuncia, mas não há projeção legível desta conversa: indisponível, sem copiar o padrão do bot. */
  | { kind: 'unavailable' }
  | { kind: 'engine'; engine: SessionEngine }

/**
 * `sessionId` é o destino visível exato: `null` = a principal (`chatId`), senão o id da extra. Nunca cai na
 * principal quando a extra não está no catálogo.
 */
export function rotuloModelo(announced: boolean, sessions: readonly HubSession[] | null, chatId: string, sessionId: string | null): RotuloModeloView {
  if (!announced || !sessions) return { kind: 'hidden' }
  const id = sessionId ?? chatId
  const row = sessions.find((s) => s.id === id && (sessionId === null ? s.principal : !s.principal))
  return row?.engine ? { kind: 'engine', engine: row.engine } : { kind: 'unavailable' }
}

/** O motor como aparece no rótulo: o id do Hub com a inicial maiúscula (nome de produto, não traduzido). */
export const motorVisivel = (backend: string) => (backend ? backend.charAt(0).toUpperCase() + backend.slice(1) : backend)
