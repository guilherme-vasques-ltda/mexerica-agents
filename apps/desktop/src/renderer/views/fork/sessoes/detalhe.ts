// Proposta do dono, 07/11 (docs/reference/design/painel-recente-20261107): o detalhe de cada sessão no painel, só com o que o
// Hub já manda (título, pendências, entradas e o estado derivado em estado.ts). Módulo do fork (D71), puro.
import type { Bot, Entry } from '../../../../shared/models.ts'
import { messagePreview } from '../../../lib/message-preview.ts'
import { genuiChoiceOf } from '../genui/choice-entry.ts'
import { entriesOfSession, type HubSession, type SessionState } from './estado.ts'

/** O que a sessão espera de você: aprovação, pergunta ao dono (MB-1b) ou cartão de escolha. */
export interface SessionNeed { kind: 'permission' | 'question' | 'choice'; text: string }
/** Na fila: uma mensagem sua na sessão ainda espera o bot (`status: queued`). */
export type DetailState = SessionState | 'queued'
export interface SessionDetail {
  /** O título do Hub, o 1º pedido resumido ou null (sem nada: "Nova sessão"). */
  title: string | null
  state: DetailState
  need: SessionNeed | null
  /** A última mensagem com texto (sua ou do bot), se estiver no histórico carregado. */
  last: { at: number; mine: boolean; text: string } | null
  /** A hora da última interação: a da última mensagem ou, sem ela, `lastUsedAt`. */
  at: number
}

const texto = (e: Entry) => (e.data.text?.trim() ? messagePreview(e.data.text) : '')

export function sessionDetail(chat: Bot, session: Pick<HubSession, 'id' | 'title' | 'principal' | 'lastUsedAt'>, state: SessionState,
  entries: readonly Entry[], known: ReadonlySet<string>): SessionDetail {
  const own = entriesOfSession([...entries], chat.id, session.principal || session.id === chat.id ? null : session.id, known)
  const firstAsk = own.find((e) => e.kind === 'user' && texto(e))
  // A principal também (R1-02, #509): o título do Hub, o 1º pedido resumido ou null ("Nova sessão").
  const title = session.title.trim() || (firstAsk ? texto(firstAsk) : null)
  let need: SessionNeed | null = null
  for (const e of own) {
    if (e.kind === 'permission' && e.data.status === 'pending') need = { kind: 'permission', text: e.data.title ?? '' }
    else if (genuiChoiceOf(e)?.hub.state === 'pending') need = { kind: 'choice', text: texto(e) }
  }
  const sid = session.principal ? chat.id : session.id
  const q = chat.pendingQuestions.filter((p) => p.chatId === chat.id && (p.sessionId ?? chat.id) === sid).at(-1)
  if (q) need = { kind: 'question', text: messagePreview(q.text) }
  const lastEntry = own.filter((e) => (e.kind === 'user' || e.kind === 'agent') && texto(e)).at(-1)
  if (state === 'archived') need = null
  // Pendência achada aqui (o cartão de escolha não entra na derivação de estado.ts) também é "pede você"; na fila só
  // quando a sessão está livre (rodando ou pedindo você diz mais).
  const queued = state === 'idle' && own.some((e) => e.kind === 'user' && e.data.status === 'queued')
  return {
    title,
    state: need ? 'needsYou' : queued ? 'queued' : state,
    need,
    last: lastEntry ? { at: lastEntry.createdAt, mine: lastEntry.kind === 'user', text: texto(lastEntry) } : null,
    at: lastEntry?.createdAt ?? session.lastUsedAt,
  }
}
