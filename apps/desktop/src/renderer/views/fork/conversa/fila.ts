// DON-907 (D431): a fila de mensagens como bandeja em cima do campo (docs/reference/design/don-907-fila-proposta). Só regras
// puras: quais entradas estão na fila desta conversa e em que ordem, os atalhos e o erro do Hub em código estável.
// As ações vão ao Hub (`queuedRemove`/`queuedEdit`/`queuedMove`/`queuedSendNow`, host/src/agent/bot/fork_fila.rs).
import { isHostMessage, type Entry } from '../../../../shared/models.ts'

/**
 * D431 provisória: Shift+Enter com o campo VAZIO manda a 1ª da fila (o mesmo que "Mandar agora" nela). Com texto,
 * Shift+Enter continua quebrando a linha (C1). Para desligar, troque para `false`.
 */
export const SHIFT_ENTER_MANDA_PRIMEIRA = true

export type MetodoFila = 'queuedRemove' | 'queuedEdit' | 'queuedMove' | 'queuedSendNow'

/** O pedido que uma ação da linha manda ao Hub por `queuedAction` (fork_fila.rs): método e corpo. */
export function pedidoDaAcao(method: MetodoFila, botId: string, entry: Entry, extra: Record<string, unknown> = {}): { method: MetodoFila; body: Record<string, unknown> } {
  return { method, body: { botId, entryId: entry.id, ...extra } }
}

/** Mensagem que ainda espera na fila: aparece na bandeja, não na conversa (entra nela quando é mandada). */
export const naFila = (e: Entry): boolean => e.kind === 'user' && !isHostMessage(e) && e.data.status === 'queued'

/** As mensagens ainda na fila desta conversa (a principal ou a extra aberta), na ordem que o Hub vai mandar. */
export function filaDaConversa(entries: readonly Entry[], chatId: string, sessionId: string | null): Entry[] {
  const sessao = sessionId ?? chatId
  return entries
    .filter((e) => naFila(e) && (e.sessionId ?? e.botId) === sessao)
    .sort((a, b) => (a.data.queueOrder ?? a.seq) - (b.data.queueOrder ?? b.seq) || a.seq - b.seq)
}

export interface TeclaFila {
  key: string
  shiftKey: boolean
  altKey: boolean
  metaKey: boolean
  ctrlKey: boolean
  isComposing: boolean
}

/** Atalhos do campo vazio: Shift+Enter manda a 1ª (D431 provisória); ↑ edita a última. */
export function atalhoDaFila(e: TeclaFila, campoVazio: boolean, tamanho: number): 'mandarPrimeira' | 'editarUltima' | null {
  if (!campoVazio || tamanho === 0 || e.isComposing || e.altKey || e.metaKey || e.ctrlKey) return null
  if (e.key === 'Enter' && e.shiftKey && SHIFT_ENTER_MANDA_PRIMEIRA) return 'mandarPrimeira'
  if (e.key === 'ArrowUp' && !e.shiftKey) return 'editarUltima'
  return null
}

/** Para qual posição a linha `de` vai: quantas outras linhas têm o meio (`centros`, na ordem atual) acima do ponteiro. */
export function destinoDoArraste(centros: readonly number[], y: number, de: number): number {
  return centros.filter((c, i) => i !== de && c < y).length
}

/** A lista com o item `de` levado para `para` (a prévia do arraste, antes do Hub confirmar). */
export function mover<T>(lista: readonly T[], de: number, para: number): T[] {
  const nova = [...lista]
  const [item] = nova.splice(de, 1)
  if (item !== undefined) nova.splice(Math.max(0, Math.min(para, nova.length)), 0, item)
  return nova
}

/** Por que a ação não valeu. O Hub põe o código no começo da mensagem; o resto do texto nunca vai à tela. */
export type FalhaFila = 'saiu' | 'mudou' | 'permissao' | 'pendente' | 'falhou'

export function falhaDaFila(error: unknown): FalhaFila {
  const message = (error as { message?: unknown } | null)?.message
  const code = typeof message === 'string' ? /^[a-z_]+/.exec(message)?.[0] : undefined
  switch (code) {
    case 'queued_entry_dispatched':
    case 'queued_entry_not_found': return 'saiu'
    case 'queued_revision_conflict': return 'mudou'
    case 'not_allowed': return 'permissao'
    case 'queued_control_pending': return 'pendente'
    default: return 'falhou'
  }
}
